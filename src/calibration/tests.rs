use super::{BUDGET, BoundarySearch, RttStats};
use chrono::{TimeZone, Utc};
use std::time::Duration;

fn simulated_round(phase_ms: i64, offset_ms: i64, rtt_ms: u64) -> BoundarySearch {
    let anchor = Utc
        .timestamp_millis_opt(1_700_000_000_000 + phase_ms)
        .unwrap();
    let mut baseline = RttStats::default();
    baseline.add(Duration::from_millis(rtt_ms));
    let mut round = BoundarySearch::new(anchor, baseline.fine_step());
    let rtt = Duration::from_millis(rtt_ms);
    let mut attempts = 0;
    while let Some(sent) = round.next_probe() {
        let received = sent + rtt;
        let stamp = anchor.timestamp_millis() + (sent + rtt / 2).as_millis() as i64 + offset_ms;
        round
            .observe(sent, received, Some(stamp.div_euclid(1000)))
            .unwrap();
        let range = round.range.unwrap();
        let truth = i128::from(offset_ms) * 1_000_000;
        assert!(range.lower_ns <= truth && truth < range.upper_ns);
        attempts += 1;
        assert!(attempts <= 25);
    }
    round
}

#[test]
fn low_latency_round_brackets_the_truth_and_selects_the_nearer_side() {
    for phase in [173, 473, 773] {
        for (offset, nearer_side_ms) in [(-1600, -1600), (0, 0), (600, -400)] {
            let round = simulated_round(phase, offset, 2);
            assert_eq!(
                round.fine.as_ref().map(|f| f.step),
                Some(Duration::from_millis(10))
            );
            let bounds = round.range.unwrap();
            assert!(bounds.upper_ns - bounds.lower_ns <= 20_000_000);
            let selected = round.selected_diff_ns.unwrap();
            assert!((selected - nearer_side_ms * 1_000_000_i128).abs() <= 10_000_000);
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
        assert_eq!(
            round.fine.as_ref().map(|f| f.step),
            step.map(Duration::from_millis)
        );
        assert!(round.selected_diff_ns.is_some());
        if step.is_none() {
            assert!(round.fine.is_none());
        }
    }
}

#[test]
fn failures_and_deadline_do_not_contribute_bounds_or_rtt() {
    let mut round = BoundarySearch::new(
        Utc.timestamp_opt(0, 0).unwrap(),
        Some(Duration::from_millis(10)),
    );
    round
        .observe(Duration::ZERO, Duration::from_millis(10), Some(0))
        .unwrap();
    let first = round.range.unwrap();
    let sent = round.next_probe().unwrap();
    round
        .observe(sent, sent + Duration::from_millis(10), None)
        .unwrap();
    let sent = round.next_probe().unwrap();
    round
        .observe(sent, sent + Duration::from_millis(10), Some(2))
        .unwrap();
    assert!(round.fine.as_ref().map(|f| f.step).is_none());
    let before = round.range.unwrap();
    let sent = round.next_probe().unwrap();
    round.observe(sent, BUDGET, Some(100)).unwrap();
    assert!(round.next_probe().is_none());
    assert_eq!(round.range.unwrap().upper_ns, before.upper_ns);
    assert_eq!(round.range.unwrap().lower_ns, first.lower_ns);
}

#[test]
fn incompatible_dates_invalidate_the_estimate() {
    let mut round = BoundarySearch::new(
        Utc.timestamp_opt(0, 0).unwrap(),
        Some(Duration::from_millis(10)),
    );
    round
        .observe(Duration::ZERO, Duration::ZERO, Some(0))
        .unwrap();
    let sent = round.next_probe().unwrap();
    assert!(round.observe(sent, sent, Some(15)).is_err());
    assert!(round.next_probe().is_none());
    assert!(round.range.is_none());
}

#[test]
fn crossing_selects_the_smaller_magnitude_and_keeps_its_sign() {
    use super::SamplePoint;
    for (elapsed_ms, before_ms, after_ms, expected) in [
        (1000, -996, 4, Some(4)),
        (1010, -994, -4, Some(-4)),
        (1025, -375, 600, Some(-375)),
        (1100, 20, 920, Some(20)),
        (1100, -450, 450, Some(450)),
        (1010, -50, -60, None),   // Ordinary step, no rollover.
        (1900, -500, -400, None), // Two header seconds, but the probe was late.
    ] {
        let point = |time, diff| SamplePoint {
            midpoint_ns: time * 1_000_000_i128,
            diff_ns: diff * 1_000_000_i128,
        };
        let result = point(0, before_ms).crossing_diff(point(elapsed_ms, after_ms));
        assert_eq!(result, expected.map(|ms| ms * 1_000_000_i128));
    }
}
