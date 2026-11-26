use super::session::{CalibrationRuntime, ProbeReply};
use chrono::{DateTime, Utc};
use reqwest::{
    Client, Url,
    header::{AGE, DATE},
};
use std::{ops::RangeInclusive, time::Duration};
use tokio::time::{Instant, sleep_until, timeout_at};

pub(super) struct HttpRuntime<'a> {
    pub client: &'a Client,
    pub url: &'a Url,
    pub origin: Instant,
}

impl CalibrationRuntime for HttpRuntime<'_> {
    fn elapsed(&self) -> Duration {
        self.origin.elapsed()
    }
    fn utc_now(&self) -> DateTime<Utc> {
        Utc::now()
    }
    fn draw(&mut self, range: RangeInclusive<u16>) -> u16 {
        fastrand::u16(range)
    }
    async fn wait_until(&mut self, time: Duration) {
        sleep_until(self.origin + time).await;
    }
    async fn probe_until(&mut self, deadline: Duration) -> Option<ProbeReply> {
        let response = timeout_at(
            self.origin + deadline,
            self.client.head(self.url.clone()).send(),
        )
        .await
        .ok()?
        .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let headers = response.headers();
        let fresh = headers
            .get_all(AGE)
            .iter()
            .all(|v| v.to_str().ok().and_then(|s| s.parse::<u64>().ok()) == Some(0));
        let date = if fresh && headers.get_all(DATE).iter().count() == 1 {
            headers
                .get(DATE)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| httpdate::parse_http_date(s).ok())
                .map(|t| DateTime::<Utc>::from(t).timestamp())
        } else {
            None
        };
        Some(ProbeReply { date })
    }
}
