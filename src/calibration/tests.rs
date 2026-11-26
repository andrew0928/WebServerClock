use super::{BUDGET, Calibration};
use chrono::{TimeZone, Utc};
use std::time::Duration;

fn simulated_round(phase_ms: i64, offset_ms: i64, rtt_ms: u64) -> Calibration {
    let anchor = Utc
        .timestamp_millis_opt(1_700_000_000_000 + phase_ms)
        .unwrap();
    let mut round = Calibration::new(anchor);
    let rtt = Duration::from_millis(rtt_ms);
    let mut attempts = 0;
    while let Some(sent) = round.next_probe() {
        let received = sent + rtt;
        let stamp = anchor.timestamp_millis() + (sent + rtt / 2).as_millis() as i64 + offset_ms;
        round
            .observe(sent, received, Some(stamp.div_euclid(1000)))
            .unwrap();
        let range = round.range().unwrap();
        let truth = i128::from(offset_ms) * 1_000_000;
        assert!(range.lower_ns <= truth && truth < range.upper_ns);
        attempts += 1;
        assert!(attempts <= 25);
    }
    round
}

#[test]
fn low_latency_run_converges_for_positive_negative_and_zero_offsets() {
    for phase in [173, 473, 773] {
        for offset in [-1600, 0, 600] {
            let round = simulated_round(phase, offset, 2);
            assert_eq!(round.fine_step(), Some(Duration::from_millis(10)));
            assert!(round.range().unwrap().half_width_ms() <= 10.0);
            assert!(round.all_rtt().count() > round.coarse_rtt().count());
        }
    }
}

#[test]
fn strict_tiers_and_high_latency_coarse_only() {
    for (rtt, step) in [
        (39, Some(10)),
        (40, Some(20)),
        (99, Some(20)),
        (100, Some(25)),
        (199, Some(25)),
        (200, None),
    ] {
        let round = simulated_round(473, 600, rtt);
        assert_eq!(round.fine_step(), step.map(Duration::from_millis));
        assert_eq!(round.coarse_rtt().mean_half_ms(), Some(rtt as f64 / 2.0));
        if step.is_none() {
            assert_eq!(round.all_rtt().count(), round.coarse_rtt().count());
        }
    }
}

#[test]
fn selects_once_using_all_coarse_rtts_including_first_and_waits_for_three() {
    let mut round = Calibration::new(Utc.timestamp_opt(0, 0).unwrap());
    for (date, rtt, step) in [
        (0, 100, None),
        (2, 40, None),
        (3, 40, Some(20)),
        (4, 200, Some(20)),
    ] {
        let sent = round.next_probe().unwrap();
        round
            .observe(sent, sent + Duration::from_millis(rtt), Some(date))
            .unwrap();
        assert_eq!(round.fine_step(), step.map(Duration::from_millis));
    }
    assert_eq!(round.coarse_rtt().count(), 3);
    assert_eq!(round.coarse_rtt().mean_half_ms(), Some(30.0));
    assert_eq!(round.all_rtt().mean_half_ms(), Some(47.5));
}

#[test]
fn failures_and_deadline_do_not_contribute_bounds_or_rtt() {
    let mut round = Calibration::new(Utc.timestamp_opt(0, 0).unwrap());
    round
        .observe(Duration::ZERO, Duration::from_millis(10), Some(0))
        .unwrap();
    let first = round.range().unwrap();
    let sent = round.next_probe().unwrap();
    round
        .observe(sent, sent + Duration::from_millis(10), None)
        .unwrap();
    let sent = round.next_probe().unwrap();
    round
        .observe(sent, sent + Duration::from_millis(10), Some(2))
        .unwrap();
    assert_eq!(round.coarse_rtt().count(), 2);
    assert!(round.fine_step().is_none());
    let before = round.range().unwrap();
    let sent = round.next_probe().unwrap();
    round.observe(sent, BUDGET, Some(100)).unwrap();
    assert!(round.next_probe().is_none());
    assert_eq!(round.range().unwrap().upper_ns, before.upper_ns);
    assert_eq!(round.range().unwrap().lower_ns, first.lower_ns);
    assert_eq!(round.all_rtt().count(), 2);
}

#[test]
fn incompatible_dates_invalidate_the_estimate() {
    let mut round = Calibration::new(Utc.timestamp_opt(0, 0).unwrap());
    round
        .observe(Duration::ZERO, Duration::ZERO, Some(0))
        .unwrap();
    let sent = round.next_probe().unwrap();
    assert!(round.observe(sent, sent, Some(15)).is_err());
    assert!(round.next_probe().is_none());
    assert!(round.range().is_none());
}
