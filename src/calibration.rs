//! HTTP Date calibration sessions with an injectable runtime and a clock-free boundary estimator.
mod http;
mod session;
pub use session::{BoundaryResult, CalibrationEvent, CalibrationOptions, calibrate};

use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::time::Duration;

const BUDGET: Duration = Duration::from_secs(20);
const SECOND_NS: i128 = 1_000_000_000;
const COARSE_PERIOD: Duration = Duration::from_millis(1100);

#[derive(Clone, Copy, Debug)]
struct OffsetRange {
    /// Remote minus local, in nanoseconds. The lower bound is inclusive.
    pub lower_ns: i128,
    /// Exclusive upper bound.
    pub upper_ns: i128,
}

#[derive(Clone, Copy)]
struct SamplePoint {
    midpoint_ns: i128,
    diff_ns: i128,
}

impl SamplePoint {
    /// On a forward scan the ordinary drift is -step; a rollover adds one second.
    /// Classify only adjacent one-tick observations, using the actual RTT midpoints.
    fn crossing_diff(self, next: Self) -> Option<i128> {
        let one_tick = SECOND_NS / 2..3 * SECOND_NS / 2;
        let elapsed = next.midpoint_ns - self.midpoint_ns;
        let jump = next.diff_ns - self.diff_ns;
        if one_tick.contains(&elapsed) && one_tick.contains(&jump) {
            Some(if self.diff_ns.abs() < next.diff_ns.abs() {
                self.diff_ns
            } else {
                next.diff_ns
            })
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct RttStats {
    count: u32,
    total: Duration,
}

impl RttStats {
    fn add(&mut self, rtt: Duration) {
        self.count += 1;
        self.total += rtt;
    }

    fn mean_half_ms(self) -> Option<f64> {
        (self.count > 0).then(|| self.total.as_secs_f64() * 500.0 / f64::from(self.count))
    }

    fn fine_step(self) -> Option<Duration> {
        // Compare the sum directly so rounding the displayed mean cannot change a tier.
        [(20, 10), (50, 20), (100, 25)]
            .into_iter()
            .find(|(limit, _)| {
                self.total < Duration::from_millis(2 * limit * u64::from(self.count))
            })
            .map(|(_, step)| Duration::from_millis(step))
    }
}

struct FineScan {
    first_boundary_ns: i128,
    width_ns: i128,
    step: Duration,
    index: i128,
    previous_point: Option<SamplePoint>,
    crossed: bool,
}

impl FineScan {
    fn next_send(
        &mut self,
        anchor_ns: i128,
        near: Duration,
        earliest: Duration,
        half_rtt_ns: i128,
    ) -> Duration {
        let step = self.step.as_nanos() as i128;
        if self.index == 0 {
            self.previous_point = None;
        }
        let advance = (self.index * step).min(self.width_ns);
        self.index += 1;
        if advance == self.width_ns {
            self.index = 0;
        }
        let phase =
            (self.first_boundary_ns + advance - half_rtt_ns - anchor_ns).rem_euclid(SECOND_NS);
        let near = near.as_nanos() as i128;
        let mut target =
            near + (phase - near + SECOND_NS / 2).rem_euclid(SECOND_NS) - SECOND_NS / 2;
        while target < earliest.as_nanos() as i128 {
            target += SECOND_NS;
        }
        Duration::from_nanos(target as u64)
    }
}

/// State of one round. Request the next probe time, then feed its observation back.
struct BoundarySearch {
    selected_step: Option<Duration>,
    anchor_ns: i128,
    next: Option<Duration>,
    range: Option<OffsetRange>,
    selected_diff_ns: Option<i128>,
    previous: Option<SamplePoint>,
    fine: Option<FineScan>,
    recent: VecDeque<Duration>,
}

impl BoundarySearch {
    /// `anchor` is local UTC at elapsed time zero. The first probe is due immediately.
    fn new(anchor: DateTime<Utc>, selected_step: Option<Duration>) -> Self {
        Self {
            selected_step,
            anchor_ns: i128::from(anchor.timestamp()) * SECOND_NS
                + i128::from(anchor.timestamp_subsec_nanos()),
            next: Some(Duration::ZERO),
            range: None,
            selected_diff_ns: None,
            previous: None,
            fine: None,
            recent: VecDeque::with_capacity(5),
        }
    }

    pub fn next_probe(&self) -> Option<Duration> {
        self.next
    }
    /// Record one attempted request. Times are elapsed since the anchor; `date`
    /// is the HTTP Date in Unix seconds, or None for a failed probe.
    pub fn observe(
        &mut self,
        sent: Duration,
        received: Duration,
        date: Option<i64>,
    ) -> Result<(), &'static str> {
        let Some(planned) = self.next else {
            return Ok(());
        };
        if received < sent || sent < planned {
            return Err("取樣時間順序錯誤");
        }
        if received >= BUDGET {
            self.next = None;
            return Ok(());
        }
        if let Some(date) = date {
            let relative = i128::from(date) * SECOND_NS - self.anchor_ns;
            let mut bounds = OffsetRange {
                lower_ns: relative - received.as_nanos() as i128,
                upper_ns: relative + SECOND_NS - sent.as_nanos() as i128,
            };
            if let Some(old) = self.range {
                bounds.lower_ns = bounds.lower_ns.max(old.lower_ns);
                bounds.upper_ns = bounds.upper_ns.min(old.upper_ns);
            }
            if bounds.lower_ns >= bounds.upper_ns {
                self.next = None;
                self.range = None;
                self.selected_diff_ns = None;
                return Err("樣本的時差區間沒有交集");
            }
            self.range = Some(bounds);
            let rtt = received - sent;
            let midpoint_ns = (sent.as_nanos() + received.as_nanos()) as i128 / 2;
            let point = SamplePoint {
                midpoint_ns,
                diff_ns: relative - midpoint_ns,
            };
            if self.recent.len() == 5 {
                self.recent.pop_front();
            }
            self.recent.push_back(rtt);
            if let Some(scan) = &mut self.fine {
                if let Some(diff) = scan
                    .previous_point
                    .and_then(|prev| prev.crossing_diff(point))
                {
                    self.selected_diff_ns = Some(diff);
                    scan.crossed = true;
                    self.next = None;
                    return Ok(());
                }
                scan.previous_point = Some(point);
            } else {
                let diff = self.previous.and_then(|prev| prev.crossing_diff(point));
                self.previous = Some(point);
                if let Some(diff) = diff {
                    self.selected_diff_ns = Some(diff);
                    let Some(step) = self.selected_step else {
                        self.next = None;
                        return Ok(());
                    };
                    self.fine = Some(FineScan {
                        first_boundary_ns: -bounds.upper_ns,
                        width_ns: bounds.upper_ns - bounds.lower_ns,
                        step,
                        index: 0,
                        previous_point: None,
                        crossed: false,
                    });
                }
            }
        } else {
            if let Some(scan) = &mut self.fine {
                scan.previous_point = None;
            }
            self.previous = None;
        }

        let next = if let Some(fine) = &mut self.fine {
            let mut recent: Vec<_> = self.recent.iter().map(|r| r.as_nanos() as i128).collect();
            recent.sort_unstable();
            let half_rtt = (recent[(recent.len() - 1) / 2] + recent[recent.len() / 2]) / 4;
            fine.next_send(
                self.anchor_ns,
                planned + Duration::from_secs(1),
                received,
                half_rtt,
            )
        } else {
            let mut next = planned + COARSE_PERIOD;
            while next < received {
                next += COARSE_PERIOD;
                self.previous = None;
            }
            next
        };
        self.next = (next < BUDGET).then_some(next);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
