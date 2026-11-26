use std::collections::VecDeque;
use std::io::{self, BufWriter, Stdout, Write};
use std::time::{Duration, Instant};

use chrono::{Local, TimeDelta, Utc};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute, queue,
    style::{Attribute, ContentStyle, PrintStyledContent, SetAttribute, Stylize},
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use reqwest::{Client, Url};
use tokio::time::{MissedTickBehavior, interval};
use unicode_width::UnicodeWidthChar;

use crate::probe::{Sample, Sampler, TIME_FORMAT};

const WINDOW: Duration = Duration::from_secs(5);
const REFRESH: Duration = Duration::from_millis(10);
const EMPTY_TIME: &str = "--:--:--.---";

#[derive(Default)]
struct Window {
    samples: VecDeque<Sample>,
    last_success: Option<Sample>,
    last_error: Option<String>,
}

struct Stats {
    mean: TimeDelta,
    min: TimeDelta,
    max: TimeDelta,
}

impl Window {
    fn record(&mut self, result: Result<Sample, String>) {
        match result {
            Ok(sample) => {
                self.samples.push_back(sample);
                self.last_success = Some(sample);
                self.last_error = None;
            }
            Err(error) => self.last_error = Some(error),
        }
    }

    fn expire(&mut self, now: Instant) -> bool {
        let before = self.samples.len();
        while self
            .samples
            .front()
            .is_some_and(|s| now - s.received >= WINDOW)
        {
            self.samples.pop_front();
        }
        self.samples.len() != before
    }

    fn stats(&self) -> Option<Stats> {
        let first = self.samples.front()?.offset;
        let mut min = first;
        let mut max = first;
        let mut sum = TimeDelta::zero();
        for sample in &self.samples {
            min = min.min(sample.offset);
            max = max.max(sample.offset);
            sum += sample.offset;
        }
        Some(Stats {
            mean: sum / self.samples.len() as i32,
            min,
            max,
        })
    }
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
        window: &Window,
        stats: Option<&Stats>,
        url: &Url,
        title: Option<&str>,
    ) -> io::Result<()> {
        let out = &mut self.0;
        queue!(out, MoveTo(0, 2), Clear(ClearType::FromCursorDown))?;
        let last = window.last_success;
        let offset = |value: Option<TimeDelta>| match value {
            Some(v) => format!("{:+.1} ms", v.as_seconds_f64() * 1000.0),
            None => "—".to_owned(),
        };
        let rtt = last.map(|s| format!("{:.1} ms", s.half_trip.as_secs_f64() * 1000.0));
        for (row, label, value) in [
            (2, "最新時差", offset(last.map(|s| s.offset))),
            (3, "平均時差（5 秒）", offset(stats.map(|s| s.mean))),
            (4, "RTT/2", rtt.unwrap_or_else(|| "—".to_owned())),
        ] {
            let value = if stats.is_none() {
                value.dim()
            } else {
                value.bold()
            };
            queue!(
                out,
                MoveTo(0, row),
                PrintStyledContent(label.cyan()),
                MoveTo(18, row),
                PrintStyledContent(value)
            )?;
        }

        let size = crossterm::terminal::size()
            .ok()
            .filter(|&(w, h)| w > 0 && h > 0)
            .unwrap_or((80, 24));
        let quiet = ContentStyle::new().dim();
        let mut row = 6;
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
        row += 1;
        let updated = last
            .map(|s| {
                s.local_at_receipt
                    .with_timezone(&Local)
                    .format(TIME_FORMAT)
                    .to_string()
            })
            .unwrap_or_else(|| EMPTY_TIME.to_owned());
        let range = format!(
            "最近 5 秒  最小 {}   最大 {}   有效樣本 {} 筆",
            offset(stats.map(|s| s.min)),
            offset(stats.map(|s| s.max)),
            window.samples.len()
        );
        wrapped(
            out,
            &mut row,
            size,
            &format!("最後更新  {updated}（本機）"),
            quiet,
        )?;
        wrapped(out, &mut row, size, &range, quiet)?;
        if let Some(error) = &window.last_error {
            wrapped(
                out,
                &mut row,
                size,
                &format!("量測失敗  {error}"),
                ContentStyle::new().yellow(),
            )?;
        }
        row += 1;
        wrapped(out, &mut row, size, "Ctrl+C 結束", quiet)
    }

    fn clock(&mut self, mean: Option<TimeDelta>) -> io::Result<()> {
        queue!(self.0, MoveTo(18, 0))?;
        if let Some(offset) = mean {
            let remote = (Utc::now() + offset).with_timezone(&Local);
            write!(self.0, "{}", remote.format(TIME_FORMAT))?;
        } else {
            write!(self.0, "{EMPTY_TIME}")?;
        }
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

pub async fn run(sampler: &mut Sampler<'_>, client: &Client, url: &Url) -> io::Result<()> {
    let mut screen = Screen::enter()?;
    let lookup = crate::page_title::fetch(client, url);
    tokio::pin!(lookup);
    let mut lookup_done = false;
    let mut title = None;
    let mut window = Window::default();
    let mut frames = interval(REFRESH);
    frames.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        let next_sample = sampler.next();
        tokio::pin!(next_sample);
        let mut redraw_details = true;
        let mut mean = None;
        loop {
            tokio::select! {
                result = &mut lookup, if !lookup_done => {
                    title = result;
                    lookup_done = true;
                    redraw_details = true;
                }
                result = &mut next_sample => {
                    window.record(result);
                    break;
                }
                _ = frames.tick() => {
                    if window.expire(Instant::now()) || redraw_details {
                        let stats = window.stats();
                        screen.details(&window, stats.as_ref(), url, title.as_deref())?;
                        mean = stats.map(|s| s.mean);
                        redraw_details = false;
                    }
                    screen.clock(mean)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_ages_during_failures_then_recovers_with_fresh_samples() {
        let start = Instant::now();
        let sample = |seconds, offset_ms| Sample {
            received: start + Duration::from_secs(seconds),
            local_at_receipt: Utc::now(),
            offset: TimeDelta::milliseconds(offset_ms),
            half_trip: Duration::ZERO,
        };
        let mut window = Window::default();
        assert!(window.stats().is_none());
        window.record(Ok(sample(0, -1000)));
        window.record(Ok(sample(1, 500)));
        window.record(Err("timeout".into()));
        let stats = window.stats().unwrap();
        assert_eq!(stats.min, TimeDelta::milliseconds(-1000));
        assert_eq!(stats.max, TimeDelta::milliseconds(500));
        assert_eq!(stats.mean, TimeDelta::milliseconds(-250));

        assert!(window.expire(start + WINDOW));
        assert_eq!(window.stats().unwrap().mean, TimeDelta::milliseconds(500));
        assert!(window.expire(start + Duration::from_secs(6)));
        assert!(window.stats().is_none());
        assert_eq!(
            window.last_success.unwrap().received,
            start + Duration::from_secs(1)
        );
        assert_eq!(window.last_error.as_deref(), Some("timeout"));

        window.record(Ok(sample(7, 2200)));
        assert!(window.last_error.is_none());
        assert_eq!(window.samples.len(), 1);
        assert_eq!(window.stats().unwrap().mean, TimeDelta::milliseconds(2200));
    }
}
