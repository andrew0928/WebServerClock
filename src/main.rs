mod cli;
mod monitor;
mod page_title;
mod probe;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;
use cli::Cli;
use probe::{INTERVAL, Sampler};
use reqwest::Client;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(topic) = cli.skill {
        print!("{}", topic.guide());
        return ExitCode::SUCCESS;
    }
    let url = cli.url.expect("clap requires URL when --skill is absent");
    let client = match Client::builder().timeout(INTERVAL).build() {
        Ok(client) => client,
        Err(error) => {
            eprintln!("無法建立 HTTP 用戶端：{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut sampler = Sampler::new(&client, &url);
    let run = async {
        if cli.monitor {
            monitor::run(&mut sampler, &client, &url).await?;
            Ok(ExitCode::SUCCESS)
        } else {
            ping(&mut sampler, cli.continuous).await
        }
    };
    let result = tokio::select! {
        result = run => result,
        signal = tokio::signal::ctrl_c() => signal.map(|()| ExitCode::SUCCESS),
    };
    match result {
        Ok(code) => code,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("clock-ping：{error}");
            ExitCode::FAILURE
        }
    }
}

async fn ping(sampler: &mut Sampler<'_>, continuous: bool) -> io::Result<ExitCode> {
    let mut received_date = false;
    for count in 1_u64.. {
        match sampler.next().await {
            Ok(sample) => {
                writeln!(io::stdout().lock(), "{sample}")?;
                received_date = true;
            }
            Err(error) => eprintln!("量測失敗：{error}"),
        }
        if !continuous && count == 4 {
            break;
        }
    }
    Ok(if received_date {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
