use super::*;
use chrono::{TimeDelta, TimeZone};
use std::collections::VecDeque;

struct Fake {
    now: Duration,
    baseline_rtts: [Option<u64>; 5],
    calls: Vec<Duration>,
    delays: VecDeque<u16>,
    round_starts: Vec<(Duration, usize)>,
    missing_round: Option<usize>,
    valid_dates_per_round: Option<usize>,
    conflicting_round: Option<usize>,
}

impl Fake {
    fn new(rtt: u64, delays: &[u16]) -> Self {
        Self {
            now: Duration::ZERO,
            baseline_rtts: [Some(rtt); 5],
            calls: vec![],
            delays: delays.iter().copied().collect(),
            round_starts: vec![],
            missing_round: None,
            valid_dates_per_round: None,
            conflicting_round: None,
        }
    }
}

impl CalibrationRuntime for Fake {
    fn elapsed(&self) -> Duration {
        self.now
    }
    fn utc_now(&self) -> DateTime<Utc> {
        Utc.timestamp_millis_opt(1_700_000_000_373).unwrap()
            + TimeDelta::from_std(self.now).unwrap()
    }
    fn draw(&mut self, range: RangeInclusive<u16>) -> u16 {
        assert_eq!(range, 0..=1000);
        assert!(self.calls.len() >= 5);
        self.round_starts.push((self.now, self.calls.len()));
        self.delays.pop_front().expect("one draw per result")
    }
    async fn wait_until(&mut self, time: Duration) {
        self.now = self.now.max(time);
    }
    async fn probe_until(&mut self, deadline: Duration) -> Option<ProbeReply> {
        let i = self.calls.len();
        self.calls.push(self.now);
        let rtt = if i < 5 {
            self.baseline_rtts[i]
        } else {
            Some(2)
        };
        let Some(rtt) = rtt else {
            self.now = deadline;
            return None;
        };
        self.now += Duration::from_millis(rtt);
        if self.now > deadline {
            self.now = deadline;
            return None;
        }
        let number = self.round_starts.len();
        // First round has +600 ms; subsequent rounds have -600 ms. The
        // baseline has no Date, which is sufficient for measuring RTT.
        let offset = if number == 1 { 600 } else { -600 };
        let mut stamp =
            (self.utc_now().timestamp_millis() - rtt as i64 / 2 + offset).div_euclid(1000);
        if self.conflicting_round == Some(number)
            && self.calls.len() > self.round_starts[number - 1].1 + 1
        {
            stamp += 30;
        }
        let valid = i >= 5
            && self.missing_round != Some(number)
            && self
                .valid_dates_per_round
                .is_none_or(|limit| i - self.round_starts[number - 1].1 < limit);
        Some(ProbeReply {
            date: valid.then_some(stamp),
        })
    }
}

async fn record(io: &mut Fake, count: u32, manual: Option<u16>) -> (bool, Vec<CalibrationEvent>) {
    let mut events = Vec::new();
    let options = CalibrationOptions::new(count, manual).unwrap();
    let ok = run(io, options, |event| {
        events.push(event);
        Ok(())
    })
    .await
    .unwrap();
    (ok, events)
}

#[tokio::test]
async fn five_baseline_slots_then_one_delay_and_independent_search_per_result() {
    use CalibrationEvent::*;
    let mut io = Fake::new(60, &[0, 1000, 397]);
    let (ok, events) = record(&mut io, 3, None).await;
    assert!(ok);
    assert_eq!(events.len(), 10);
    assert_eq!(events[0], Preparing);
    assert_eq!(io.calls[..5], [0, 1, 2, 3, 4].map(Duration::from_secs));
    assert_eq!(io.round_starts[0].1, 5);
    assert!(io.delays.is_empty());
    for (i, delay) in [0, 1000, 397].into_iter().enumerate() {
        let number = i as u64 + 1;
        let index = 1 + 3 * i;
        assert_eq!(
            events[index],
            Calibrating {
                number,
                step_ms: 100
            }
        );
        assert_eq!(
            events[index + 1],
            Calibrating {
                number,
                step_ms: 20
            }
        );
        let Result {
            number: n,
            result: Some(reading),
        } = events[index + 2]
        else {
            panic!("missing result")
        };
        assert_eq!(n, number);
        assert_eq!(reading.step_ms, 20);
        // +600 ms produces a smaller magnitude on the early side (~-400 ms).
        let expected = if number == 1 { -400.0 } else { -600.0 };
        assert!((reading.diff_ms - expected).abs() <= 20.0);
        let (before, first) = io.round_starts[i];
        assert_eq!(io.calls[first], before + Duration::from_millis(delay));
        let end = io.round_starts.get(i + 1).map_or(io.now, |r| r.0);
        assert!(end - before <= BUDGET);
    }
}

#[tokio::test]
async fn manual_step_overrides_high_rtt_but_still_runs_the_baseline() {
    for (manual, achieved) in [(None, 100), (Some(37), 37), (Some(100), 100)] {
        let mut io = Fake::new(220, &[0]);
        let (ok, events) = record(&mut io, 1, manual).await;
        assert!(ok);
        assert_eq!(io.round_starts[0].1, 5);
        assert_eq!(events.len(), if achieved == 100 { 3 } else { 4 });
        assert!(matches!(events.last(), Some(CalibrationEvent::Result {
            number: 1, result: Some(r)
        }) if r.step_ms == achieved));
    }
}

#[tokio::test]
async fn baseline_failures_and_invalid_options() {
    assert!(CalibrationOptions::new(0, None).is_err());
    for step in [0, 101] {
        assert!(CalibrationOptions::new(1, Some(step)).is_err());
    }
    let mut io = Fake::new(0, &[]);
    io.baseline_rtts = [None, Some(60), Some(100), None, Some(200)];
    let baseline = prepare(&mut io).await;
    assert_eq!(baseline.count(), 3);
    assert_eq!(baseline.mean_half_ms(), Some(60.0));
    let mut failed = Fake::new(0, &[]);
    failed.baseline_rtts = [None; 5];
    assert_eq!(
        record(&mut failed, 1, None).await,
        (false, vec![CalibrationEvent::Preparing])
    );
    assert_eq!(failed.calls.len(), 5);
    assert!(failed.round_starts.is_empty());
}

#[tokio::test]
async fn failed_rounds_count_and_the_next_round_can_recover() {
    let mut io = Fake::new(2, &[1000, 1000, 1000]);
    io.missing_round = Some(1);
    io.conflicting_round = Some(2);
    let (ok, events) = record(&mut io, 3, None).await;
    assert!(!ok);
    let results: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            CalibrationEvent::Result { number, result } => Some((*number, result.is_some())),
            _ => None,
        })
        .collect();
    assert_eq!(results, [(1, false), (2, false), (3, true)]);
}

#[tokio::test]
async fn time_limit_returns_only_a_confirmed_crossing_and_includes_jitter() {
    for (valid, expected) in [(1, None), (2, Some(100))] {
        let mut io = Fake::new(20, &[1000]);
        io.valid_dates_per_round = Some(valid);
        let (ok, events) = record(&mut io, 1, Some(1)).await;
        assert_eq!(ok, expected.is_some());
        let Some(CalibrationEvent::Result { result, .. }) = events.last() else {
            panic!("missing result")
        };
        assert_eq!(result.map(|r| r.step_ms), expected);
        assert!(io.now - io.round_starts[0].0 <= BUDGET);
    }
}
