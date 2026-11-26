use std::io::{self, BufWriter, Stdout, Write};
use std::time::Duration;

use crate::calibrate::{Progress, RoundProgress};
use chrono::{DateTime, Local, TimeDelta, Utc};
use clock_ping::calibration::{BoundaryResult, CalibrationEvent, CalibrationOptions, calibrate};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute, queue,
    style::{Attribute, ContentStyle, PrintStyledContent, SetAttribute, Stylize},
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use reqwest::{Client, Url};
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval};
use unicode_width::UnicodeWidthChar;

const REFRESH: Duration = Duration::from_millis(10);
const TIME_FORMAT: &str = "%H:%M:%S%.3f";

struct Reading {
    mean_half_rtt_ms: Option<f64>,
    result: BoundaryResult,
    updated: DateTime<Utc>,
}

impl Reading {
    fn remote_at(&self, local: DateTime<Utc>) -> DateTime<Utc> {
        local + TimeDelta::microseconds((self.result.diff_ms * 1000.0).round() as i64)
    }
}

fn rtt_text(ms: Option<f64>) -> String {
    ms.map_or_else(
        || "—".to_owned(),
        |ms| format!("{}ms", (ms * 10.0).round() / 10.0),
    )
}

struct Screen(BufWriter<Stdout>);

impl Screen {
    fn enter() -> io::Result<Self> {
        let mut screen = Self(BufWriter::new(io::stdout()));
        execute!(
            screen.0,
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All),
            MoveTo(0, 0),
            PrintStyledContent("遠端時鐘".cyan())
        )?;
        Ok(screen)
    }

    fn details(
        &mut self,
        latest: &Reading,
        round: &RoundProgress,
        last_failed: Option<u64>,
        url: &Url,
        title: Option<&str>,
    ) -> io::Result<()> {
        let out = &mut self.0;
        queue!(out, MoveTo(0, 2), Clear(ClearType::FromCursorDown))?;
        let size = crossterm::terminal::size()
            .ok()
            .filter(|&(w, h)| w > 0 && h > 0)
            .unwrap_or((80, 24));
        let quiet = ContentStyle::new().dim();
        let mut row = 2;
        wrapped(
            out,
            &mut row,
            size,
            &format!(
                "最新時差: {:+.1} ms, RTT/2: {}",
                latest.result.diff_ms,
                rtt_text(latest.mean_half_rtt_ms)
            ),
            ContentStyle::new().bold(),
        )?;
        wrapped(
            out,
            &mut row,
            size,
            &format!(
                "檢測中:   count = {}, {}, RTT/2: {}",
                round.number,
                round.stages(", "),
                rtt_text(round.mean_half_rtt_ms)
            ),
            ContentStyle::new().cyan(),
        )?;
        if let Some(n) = last_failed {
            wrapped(
                out,
                &mut row,
                size,
                &format!("第 {n} 輪未取得結果"),
                ContentStyle::new().yellow(),
            )?;
        }
        row += 1;
        wrapped(
            out,
            &mut row,
            size,
            &format!("網址  {url}"),
            ContentStyle::new(),
        )?;
        if let Some(title) = title {
            wrapped(
                out,
                &mut row,
                size,
                &format!("標題  {title}"),
                ContentStyle::new(),
            )?;
        }
        let updated = latest.updated.with_timezone(&Local).format(TIME_FORMAT);
        wrapped(
            out,
            &mut row,
            size,
            &format!("最後更新  {updated}（本機）"),
            quiet,
        )?;
        wrapped(out, &mut row, size, "Ctrl+C 結束", quiet)
    }

    fn clock(&mut self, latest: &Reading) -> io::Result<()> {
        let remote = latest.remote_at(Utc::now()).with_timezone(&Local);
        queue!(self.0, MoveTo(18, 0))?;
        write!(self.0, "{}", remote.format(TIME_FORMAT))?;
        self.0.flush()
    }
}

// The URL can span several terminal rows. Never let wrapped metadata scroll the clock away.
fn wrapped(
    out: &mut impl Write,
    row: &mut u16,
    (width, height): (u16, u16),
    text: &str,
    style: ContentStyle,
) -> io::Result<()> {
    let mut start = 0;
    let mut columns = 0;
    for (index, ch) in text.char_indices() {
        let cells = ch.width().unwrap_or(0);
        if columns + cells > usize::from(width) && index > start {
            if *row >= height {
                return Ok(());
            }
            queue!(
                out,
                MoveTo(0, *row),
                PrintStyledContent(style.apply(&text[start..index]))
            )?;
            *row += 1;
            start = index;
            columns = 0;
        }
        columns += cells;
    }
    if *row < height {
        queue!(
            out,
            MoveTo(0, *row),
            PrintStyledContent(style.apply(&text[start..]))
        )?;
        *row += 1;
    }
    Ok(())
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(
            self.0,
            SetAttribute(Attribute::Reset),
            LeaveAlternateScreen,
            Show
        );
    }
}

pub async fn run(client: &Client, url: &Url, options: CalibrationOptions) -> io::Result<bool> {
    let (events, receiver) = mpsc::unbounded_channel();
    let calibration = calibrate(client, url, options, move |event| {
        events.send(event).map_err(io::Error::other)
    });
    let (success, ()) = tokio::try_join!(calibration, display(receiver, client, url))?;
    Ok(success)
}

async fn display(
    mut events: mpsc::UnboundedReceiver<CalibrationEvent>,
    client: &Client,
    url: &Url,
) -> io::Result<()> {
    let mut progress = Progress::default();
    let mut round = RoundProgress::default();
    let mut latest = loop {
        let Some(event) = events.recv().await else {
            progress.clear()?;
            eprintln!("準備失敗。");
            return Ok(());
        };
        progress.stage(event)?;
        round.observe(event);
        if let CalibrationEvent::Result {
            number,
            result,
            mean_half_rtt_ms,
        } = event
        {
            if let Some(result) = result {
                break Reading {
                    mean_half_rtt_ms,
                    result,
                    updated: Utc::now(),
                };
            }
            eprintln!("結果 count = {number}：未取得結果");
        }
    };

    let mut screen = Screen::enter()?;
    let lookup = crate::page_title::fetch(client, url);
    tokio::pin!(lookup);
    let mut lookup_done = false;
    let mut title = None;
    let mut last_failed = None;
    let mut frames = interval(REFRESH);
    frames.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        screen.details(&latest, &round, last_failed, url, title.as_deref())?;
        screen.clock(&latest)?;
        loop {
            tokio::select! {
                event = events.recv() => {
                    let Some(event) = event else { return Ok(()) };
                    round.observe(event);
                    if let CalibrationEvent::Result { number, result, mean_half_rtt_ms } = event {
                        if let Some(result) = result {
                            latest = Reading { mean_half_rtt_ms, result, updated: Utc::now() };
                            last_failed = None;
                        } else {
                            last_failed = Some(number);
                        }
                    }
                    break;
                }
                result = &mut lookup, if !lookup_done => {
                    title = result;
                    lookup_done = true;
                    break;
                }
                _ = frames.tick() => screen.clock(&latest)?,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_clock_applies_the_latest_signed_offset_across_second_boundaries() {
        let local = DateTime::from_timestamp_millis(1_735_689_599_998).unwrap();
        let mut reading = Reading {
            mean_half_rtt_ms: Some(1.0),
            result: BoundaryResult {
                diff_ms: 4.5,
                step_ms: 10,
            },
            updated: local,
        };
        assert_eq!(
            reading.remote_at(local) - local,
            TimeDelta::microseconds(4500)
        );
        reading.result.diff_ms = -12.3;
        let later = local + TimeDelta::seconds(8);
        assert_eq!(
            reading.remote_at(later) - later,
            TimeDelta::microseconds(-12300)
        );
    }
}
