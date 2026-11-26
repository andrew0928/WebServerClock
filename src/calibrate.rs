use clock_ping::calibration::{CalibrationEvent, CalibrationOptions, calibrate};
use reqwest::{Client, Url};
use std::{
    io::{self, Write},
    process::ExitCode,
};

pub async fn run(
    client: &Client,
    url: &Url,
    count: u32,
    min_step_ms: Option<u16>,
) -> io::Result<ExitCode> {
    let options = CalibrationOptions::new(count, min_step_ms).map_err(io::Error::other)?;
    let success = calibrate(client, url, options, |event| {
        let mut out = io::stdout().lock();
        match event {
            CalibrationEvent::Preparing => writeln!(out, "準備中")?,
            CalibrationEvent::Calibrating { number, step_ms } => {
                writeln!(out, "校正中 count = {number}（{step_ms} ms）")?
            }
            CalibrationEvent::Result {
                number,
                result: Some(result),
            } => writeln!(
                out,
                "結果 count = {number}（{} ms）：{:+.1} ms",
                result.step_ms, result.diff_ms
            )?,
            CalibrationEvent::Result {
                number,
                result: None,
            } => writeln!(out, "結果 count = {number}：未取得結果")?,
        }
        out.flush()
    })
    .await?;
    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
