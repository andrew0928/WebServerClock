use crate::probe::{request_error, server_date};
use chrono::Utc;
use clock_ping::calibration::{BUDGET, Calibration};
use reqwest::{
    Client, Response, Url,
    header::{AGE, DATE},
};
use std::{
    io::{self, Write},
    process::ExitCode,
};
use tokio::time::{Instant, sleep_until, timeout_at};

fn fresh_date(response: &Response) -> Result<i64, String> {
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    if response
        .headers()
        .get_all(AGE)
        .iter()
        .any(|age| age.to_str().ok().and_then(|s| s.parse::<u64>().ok()) != Some(0))
    {
        return Err("Age 標頭顯示回應經過快取或格式無效".into());
    }
    if response.headers().get_all(DATE).iter().count() > 1 {
        return Err("回應包含多個 Date 標頭".into());
    }
    Ok(server_date(response)?.timestamp())
}

pub async fn run(client: &Client, url: &Url) -> io::Result<ExitCode> {
    let start = Instant::now();
    let mut round = Calibration::new(Utc::now());
    let deadline = start + BUDGET;
    while let Some(elapsed) = round.next_probe() {
        sleep_until(start + elapsed).await;
        let sent = start.elapsed();
        if sent >= BUDGET {
            break;
        }
        let request = client.head(url.clone()).send();
        let Ok(result) = timeout_at(deadline, request).await else {
            break;
        };
        let received = start.elapsed();
        let date = match result.map_err(request_error).and_then(|r| fresh_date(&r)) {
            Ok(date) => Some(date),
            Err(error) => {
                eprintln!("量測失敗：{error}");
                None
            }
        };
        if let Err(error) = round.observe(sent, received, date) {
            eprintln!("校正失敗：{error}");
            return Ok(ExitCode::FAILURE);
        }
    }
    let Some(range) = round.range() else {
        eprintln!("校正失敗：未取得有效的 Date 樣本");
        return Ok(ExitCode::FAILURE);
    };
    let coarse = round.coarse_rtt();
    let all = round.all_rtt();
    let step = round.fine_step().map_or_else(
        || "僅粗掃".to_owned(),
        |s| format!("細掃步長 {} ms", s.as_millis()),
    );
    writeln!(
        io::stdout().lock(),
        "估計時差 {:+.1} ±{:.1} ms  粗掃平均 RTT/2 {:.2} ms  整輪平均 RTT/2 {:.2} ms  {}  樣本 {}＋{}",
        range.midpoint_ms(),
        range.half_width_ms(),
        coarse.mean_half_ms().unwrap(),
        all.mean_half_ms().unwrap(),
        step,
        coarse.count(),
        all.count() - coarse.count()
    )?;
    Ok(ExitCode::SUCCESS)
}
