//! A calibration round is driven by its caller's monotonic elapsed time.
//! It performs no I/O and never reads a clock or sleeps.
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::time::Duration;

pub const BUDGET: Duration = Duration::from_secs(20);
const SECOND_NS: i128 = 1_000_000_000;
const COARSE_PERIOD: Duration = Duration::from_millis(1100);

#[derive(Clone, Copy, Debug)]
pub struct OffsetRange {
    /// Remote minus local, in nanoseconds. The lower bound is inclusive.
    pub lower_ns: i128,
    /// Exclusive upper bound.
    pub upper_ns: i128,
}

impl OffsetRange {
    pub fn midpoint_ms(self) -> f64 {
        (self.upper_ns + self.lower_ns) as f64 / 2_000_000.0
    }

    pub fn half_width_ms(self) -> f64 {
        (self.upper_ns - self.lower_ns) as f64 / 2_000_000.0
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RttStats {
    count: u32,
    total: Duration,
}

impl RttStats {
    fn add(&mut self, rtt: Duration) {
        self.count += 1;
        self.total += rtt;
    }

    pub fn count(self) -> u32 {
        self.count
    }

    pub fn mean_half_ms(self) -> Option<f64> {
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
        self.index += 1;
        let advance = (self.index * step).min(self.width_ns);
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
pub struct Calibration {
    anchor_ns: i128,
    next: Option<Duration>,
    range: Option<OffsetRange>,
    coarse: RttStats,
    all: RttStats,
    previous: Option<(Duration, i64)>,
    crossed: bool,
    fine: Option<FineScan>,
    recent: VecDeque<Duration>,
}

impl Calibration {
    /// `anchor` is local UTC at elapsed time zero. The first probe is due immediately.
    pub fn new(anchor: DateTime<Utc>) -> Self {
        Self {
            anchor_ns: i128::from(anchor.timestamp()) * SECOND_NS
                + i128::from(anchor.timestamp_subsec_nanos()),
            next: Some(Duration::ZERO),
            range: None,
            coarse: RttStats::default(),
            all: RttStats::default(),
            previous: None,
            crossed: false,
            fine: None,
            recent: VecDeque::with_capacity(5),
        }
    }

    pub fn next_probe(&self) -> Option<Duration> {
        self.next
    }
    pub fn range(&self) -> Option<OffsetRange> {
        self.range
    }
    pub fn coarse_rtt(&self) -> RttStats {
        self.coarse
    }
    pub fn all_rtt(&self) -> RttStats {
        self.all
    }
    pub fn fine_step(&self) -> Option<Duration> {
        self.fine.as_ref().map(|f| f.step)
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
                return Err("樣本的時差區間沒有交集");
            }
            self.range = Some(bounds);
            let rtt = received - sent;
            self.all.add(rtt);
            if self.recent.len() == 5 {
                self.recent.pop_front();
            }
            self.recent.push_back(rtt);
            if self.fine.is_none() {
                self.coarse.add(rtt);
                self.crossed |= self.previous.is_some_and(|(prev_sent, prev_date)| {
                    sent.saturating_sub(prev_sent) < Duration::from_secs(2)
                        && date.checked_sub(prev_date) == Some(2)
                });
                self.previous = Some((sent, date));
                if self.crossed && self.coarse.count >= 3 {
                    let Some(step) = self.coarse.fine_step() else {
                        self.next = None;
                        return Ok(());
                    };
                    self.fine = Some(FineScan {
                        first_boundary_ns: -bounds.upper_ns,
                        width_ns: bounds.upper_ns - bounds.lower_ns,
                        step,
                        index: 0,
                    });
                }
            }
        } else {
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
