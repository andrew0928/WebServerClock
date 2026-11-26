use std::error::Error;
use std::fmt;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, TimeDelta, Utc};
use reqwest::{Client, Response, Url, header::DATE};
use tokio::time::{Interval, MissedTickBehavior, interval};

pub const INTERVAL: Duration = Duration::from_secs(1);
pub const TIME_FORMAT: &str = "%H:%M:%S%.3f";

#[derive(Clone, Copy)]
pub struct Sample {
    pub received: Instant,
    pub local_at_receipt: DateTime<Utc>,
    pub offset: TimeDelta,
    pub half_trip: Duration,
}

impl fmt::Display for Sample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let remote = (self.local_at_receipt + self.offset).with_timezone(&Local);
        write!(
            f,
            "時間差 {:+.1} ms  遠端時間 {}  RTT/2 {:>4.1} ms",
            self.offset.as_seconds_f64() * 1000.0,
            remote.format(TIME_FORMAT),
            self.half_trip.as_secs_f64() * 1000.0,
        )
    }
}

pub struct Sampler<'a> {
    client: &'a Client,
    root: Url,
    ticks: Interval,
}

impl<'a> Sampler<'a> {
    pub fn new(client: &'a Client, url: &Url) -> Self {
        let mut ticks = interval(INTERVAL);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // The original Windows client probes HEAD /, regardless of the page path.
        let root = url.join("/").expect("validated HTTP(S) URL");
        Self {
            client,
            root,
            ticks,
        }
    }

    pub async fn next(&mut self) -> Result<Sample, String> {
        self.ticks.tick().await;
        let request = self.client.head(self.root.clone());
        let sent_at = Utc::now();
        let start = Instant::now();
        let response = request.send().await.map_err(request_error)?;
        let received = Instant::now();
        let round_trip = received.duration_since(start);
        let half_trip = round_trip / 2;
        let server_date = server_date(&response)?;
        Ok(Sample {
            received,
            local_at_receipt: sent_at + round_trip,
            offset: server_date - (sent_at + half_trip),
            half_trip,
        })
    }
}

pub fn request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        return "請求逾時（1 秒）。".to_owned();
    }
    let error = error.without_url();
    let mut cause: &dyn Error = &error;
    while let Some(source) = cause.source() {
        cause = source;
    }
    cause.to_string()
}

pub fn server_date(response: &Response) -> Result<DateTime<Utc>, String> {
    response
        .headers()
        .get(DATE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| httpdate::parse_http_date(value).ok())
        .map(DateTime::<Utc>::from)
        .ok_or_else(|| format!("HTTP {} 回應缺少有效的 Date 標頭。", response.status()))
}
