use clap::{Parser, ValueEnum};
use reqwest::Url;

#[derive(Parser)]
#[command(
    version,
    about = "從 HTTP Date 標頭估計網站與本機的時差",
    arg_required_else_help = true,
    help_template = "{name} {version} — {about}\n\n{usage-heading} {usage}\n\n{all-args}{after-help}",
    after_help = "範例\n  clock-ping https://example.com\n  clock-ping https://example.com -t\n  clock-ping https://example.com -m\n  clock-ping --skill measure"
)]
pub struct Cli {
    /// 要校正的 HTTP 或 HTTPS 網址（含路徑）
    #[arg(value_name = "URL", required_unless_present = "skill", value_parser = parse_url)]
    pub url: Option<Url>,

    /// 持續逐輪校正，直到按下 Ctrl+C（預設四輪）
    #[arg(short = 't')]
    pub continuous: bool,

    /// 以最新校正時差監看遠端時鐘，每 10 ms 更新畫面
    #[arg(short = 'm', long)]
    pub monitor: bool,

    /// 逐行模式的校正輪數（預設 4）
    #[arg(long, conflicts_with_all = ["continuous", "monitor", "skill"], value_parser = clap::value_parser!(u32).range(1..))]
    pub count: Option<u32>,

    /// 手動指定細掃格距（1–100 ms），省略時由 RTT 基準決定
    #[arg(long, conflicts_with = "skill", value_parser = clap::value_parser!(u16).range(1..=100))]
    pub min_step_ms: Option<u16>,

    /// 讀取離線操作指南，省略主題時列出索引
    #[arg(long, value_enum, value_name = "TOPIC", num_args = 0..=1,
          default_missing_value = "index", conflicts_with_all = ["url", "continuous", "monitor"])]
    pub skill: Option<Skill>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Skill {
    Index,
    Measure,
    Target,
    Author,
}

impl Skill {
    pub fn guide(self) -> &'static str {
        match self {
            Self::Index => include_str!("../guides/index.md"),
            Self::Measure => include_str!("../guides/measure.md"),
            Self::Target => include_str!("../guides/target.md"),
            Self::Author => include_str!("../guides/author.md"),
        }
    }
}

fn parse_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|error| error.to_string())?;
    if !matches!(url.scheme(), "http" | "https") || !url.has_host() {
        return Err("請指定含主機名稱的 HTTP 或 HTTPS 網址".to_owned());
    }
    Ok(url)
}
