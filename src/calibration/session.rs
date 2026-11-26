//! Five baseline probes, followed by independently randomized boundary searches.
use super::http::HttpRuntime;
use super::{BUDGET, BoundarySearch, RttStats};
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use std::io;
use std::{num::NonZeroU32, ops::RangeInclusive, time::Duration};

const SECOND: Duration = Duration::from_secs(1);
const BASELINE_PROBES: u32 = 5;

#[derive(Debug, Clone, Copy)]
pub struct CalibrationOptions {
    count: Option<NonZeroU32>,
    manual_step: Option<Duration>,
}

impl CalibrationOptions {
    pub fn new(count: u32, min_step_ms: Option<u16>) -> Result<Self, &'static str> {
        let count = NonZeroU32::new(count).ok_or("校正輪數必須大於零")?;
        Self::with_limit(Some(count), min_step_ms)
    }

    pub fn continuous(min_step_ms: Option<u16>) -> Result<Self, &'static str> {
        Self::with_limit(None, min_step_ms)
    }

    fn with_limit(
        count: Option<NonZeroU32>,
        min_step_ms: Option<u16>,
    ) -> Result<Self, &'static str> {
        if min_step_ms.is_some_and(|ms| !(1..=100).contains(&ms)) {
            return Err("細掃步長須介於 1 到 100 ms");
        }
        Ok(Self {
            count,
            manual_step: min_step_ms.map(|ms| Duration::from_millis(ms.into())),
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct ProbeReply {
    /// A successful HEAD counts for RTT even if it has no usable Date.
    pub date: Option<i64>,
}

/// All elapsed times and deadlines use one monotonic origin. A probe must return
/// by its deadline; None denotes a timeout or an unsuccessful HEAD request.
pub(super) trait CalibrationRuntime {
    fn elapsed(&self) -> Duration;
    fn utc_now(&self) -> DateTime<Utc>;
    fn draw(&mut self, range: RangeInclusive<u16>) -> u16;
    async fn wait_until(&mut self, time: Duration);
    async fn probe_until(&mut self, deadline: Duration) -> Option<ProbeReply>;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundaryResult {
    /// Signed HTTP Date minus the local request midpoint.
    pub diff_ms: f64,
    /// Step of the scan that actually saw the crossing.
    pub step_ms: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalibrationEvent {
    Preparing,
    Calibrating {
        number: u64,
        step_ms: u16,
    },
    Result {
        number: u64,
        result: Option<BoundaryResult>,
    },
}

/// Emit only user-visible progress and results. A round without a result or a
/// failed preparation makes the returned success flag false.
pub async fn calibrate(
    client: &Client,
    url: &Url,
    options: CalibrationOptions,
    emit: impl FnMut(CalibrationEvent) -> io::Result<()>,
) -> io::Result<bool> {
    let mut runtime = HttpRuntime {
        client,
        url,
        origin: tokio::time::Instant::now(),
    };
    run(&mut runtime, options, emit).await
}

async fn run(
    runtime: &mut impl CalibrationRuntime,
    options: CalibrationOptions,
    mut emit: impl FnMut(CalibrationEvent) -> io::Result<()>,
) -> io::Result<bool> {
    emit(CalibrationEvent::Preparing)?;
    let baseline = prepare(runtime).await;
    if baseline.count() == 0 {
        return Ok(false);
    }
    let step = options
        .manual_step
        .or_else(|| baseline.fine_step())
        .filter(|s| *s < Duration::from_millis(100));
    let mut success = true;
    for number in 1u64.. {
        emit(CalibrationEvent::Calibrating {
            number,
            step_ms: 100,
        })?;
        let start = runtime.elapsed();
        let delay = runtime.draw(0..=1000);
        runtime
            .wait_until(start + Duration::from_millis(delay.into()))
            .await;
        let result = find_boundary(runtime, step, start + BUDGET, |s| {
            emit(CalibrationEvent::Calibrating { number, step_ms: s })
        })
        .await?;
        success &= result.is_some();
        emit(CalibrationEvent::Result { number, result })?;
        if options
            .count
            .is_some_and(|count| number == u64::from(count.get()))
        {
            break;
        }
    }
    Ok(success)
}

async fn prepare(runtime: &mut impl CalibrationRuntime) -> RttStats {
    let mut stats = RttStats::default();
    let mut scheduled = runtime.elapsed();
    for _ in 0..BASELINE_PROBES {
        runtime.wait_until(scheduled).await;
        let sent = runtime.elapsed();
        scheduled = sent + SECOND;
        let reply = runtime.probe_until(scheduled).await;
        let received = runtime.elapsed();
        if reply.is_some() && received <= scheduled {
            stats.add(received - sent);
        }
    }
    stats
}

async fn find_boundary(
    runtime: &mut impl CalibrationRuntime,
    step: Option<Duration>,
    deadline: Duration,
    mut fine_started: impl FnMut(u16) -> io::Result<()>,
) -> io::Result<Option<BoundaryResult>> {
    let origin = runtime.elapsed();
    let mut search = BoundarySearch::new(runtime.utc_now(), step);
    while let Some(next) = search.next_probe() {
        if origin + next >= deadline {
            break;
        }
        runtime.wait_until(origin + next).await;
        let sent = runtime.elapsed();
        if sent >= deadline {
            break;
        }
        let reply = runtime.probe_until(deadline.min(sent + SECOND)).await;
        let received = runtime.elapsed();
        if received >= deadline {
            break;
        }
        let was_coarse = search.fine.is_none();
        if search
            .observe(sent - origin, received - origin, reply.and_then(|r| r.date))
            .is_err()
        {
            return Ok(None);
        }
        if was_coarse && let Some(scan) = &search.fine {
            fine_started(scan.step.as_millis() as u16)?;
        }
    }
    Ok(search.selected_diff_ns.map(|ns| BoundaryResult {
        diff_ms: ns as f64 / 1_000_000.0,
        step_ms: search
            .fine
            .as_ref()
            .filter(|scan| scan.crossed)
            .map_or(100, |scan| scan.step.as_millis() as u16),
    }))
}

#[cfg(test)]
mod tests;
