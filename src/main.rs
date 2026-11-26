mod calibrate;
mod cli;
mod monitor;
mod page_title;

use std::io;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use cli::Cli;
use clock_ping::calibration::CalibrationOptions;
use reqwest::Client;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(topic) = cli.skill {
        print!("{}", topic.guide());
        return ExitCode::SUCCESS;
    }
    let url = cli.url.expect("clap requires URL when --skill is absent");
    let options = if cli.continuous || cli.monitor {
        CalibrationOptions::continuous(cli.min_step_ms)
    } else {
        CalibrationOptions::new(cli.count.unwrap_or(4), cli.min_step_ms)
    }
    .expect("clap validates round count and step size");
    let client = match Client::builder().timeout(Duration::from_secs(1)).build() {
        Ok(client) => client,
        Err(error) => {
            eprintln!("無法建立 HTTP 用戶端：{error}");
            return ExitCode::FAILURE;
        }
    };
    let run = async {
        if cli.monitor {
            monitor::run(&client, &url, options).await
        } else {
            calibrate::run(&client, &url, options).await
        }
    };
    let result = tokio::select! {
        result = run => result,
        signal = tokio::signal::ctrl_c() => signal.map(|()| true),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("clock-ping：{error}");
            ExitCode::FAILURE
        }
    }
}
