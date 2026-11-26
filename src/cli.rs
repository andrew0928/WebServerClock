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
    /// HTTP 或 HTTPS 網址，向該主機的根路徑送出 HEAD 請求
    #[arg(value_name = "URL", required_unless_present = "skill", value_parser = parse_url)]
    pub url: Option<Url>,

    /// 每秒持續量測，直到按下 Ctrl+C（預設量測四次）
    #[arg(short = 't')]
    pub continuous: bool,

    /// 監看遠端時鐘，每秒取樣，以最近 5 秒的平均時差每 10 ms 更新畫面
    #[arg(short = 'm', long)]
    pub monitor: bool,

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
