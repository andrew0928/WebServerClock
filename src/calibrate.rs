use chrono::Local;
use clock_ping::calibration::{CalibrationEvent, CalibrationOptions, calibrate};
use crossterm::{
    cursor::MoveToColumn,
    execute, queue,
    style::Print,
    terminal::{Clear, ClearType},
};
use reqwest::{Client, Url};
use std::io::{self, IsTerminal, Write};
use unicode_width::UnicodeWidthChar;

// Monitor warm-up uses a temporary status line until the first valid reading.
#[derive(Default)]
pub struct Progress {
    visible: bool,
}

impl Progress {
    pub fn show(&mut self, text: impl std::fmt::Display) -> io::Result<()> {
        if io::stderr().is_terminal() {
            self.visible = true;
            execute!(
                io::stderr(),
                MoveToColumn(0),
                Clear(ClearType::CurrentLine),
                Print(text)
            )?;
        }
        Ok(())
    }
    pub fn clear(&mut self) -> io::Result<()> {
        if self.visible {
            execute!(io::stderr(), MoveToColumn(0), Clear(ClearType::CurrentLine))?;
            self.visible = false;
        }
        Ok(())
    }
    pub fn stage(&mut self, event: CalibrationEvent) -> io::Result<()> {
        match event {
            CalibrationEvent::Preparing => self.show("準備中"),
            CalibrationEvent::Initialized { .. } | CalibrationEvent::RoundRtt { .. } => Ok(()),
            CalibrationEvent::Calibrating { number, step_ms } => {
                self.show(format_args!("校正中 count = {number}（{step_ms} ms）"))
            }
            CalibrationEvent::Result { .. } => self.clear(),
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        let _ = self.clear();
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Stage {
    #[default]
    Waiting,
    Running,
    Confirmed,
    Unconfirmed,
    Skipped,
}

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Self::Waiting => "待測",
            Self::Running => "檢測中",
            Self::Confirmed => "OK",
            Self::Unconfirmed => "未確認",
            Self::Skipped => "未執行",
        }
    }
}

#[derive(Default)]
pub struct RoundProgress {
    pub number: u64,
    pub mean_half_rtt_ms: Option<f64>,
    fine_step_ms: Option<u16>,
    coarse: Stage,
    fine: Stage,
}

impl RoundProgress {
    pub fn observe(&mut self, event: CalibrationEvent) {
        match event {
            CalibrationEvent::Preparing => {}
            CalibrationEvent::Initialized { fine_step_ms, .. } => self.fine_step_ms = fine_step_ms,
            CalibrationEvent::Calibrating {
                number,
                step_ms: 100,
            } => {
                self.number = number;
                self.mean_half_rtt_ms = None;
                self.coarse = Stage::Running;
                self.fine = Stage::Waiting;
            }
            CalibrationEvent::Calibrating { .. } => {
                self.coarse = Stage::Confirmed;
                self.fine = Stage::Running;
            }
            CalibrationEvent::RoundRtt {
                mean_half_rtt_ms, ..
            } => self.mean_half_rtt_ms = Some(mean_half_rtt_ms),
            CalibrationEvent::Result {
                result,
                mean_half_rtt_ms,
                ..
            } => {
                self.mean_half_rtt_ms = mean_half_rtt_ms;
                self.coarse = if result.is_some() || self.coarse == Stage::Confirmed {
                    Stage::Confirmed
                } else {
                    Stage::Unconfirmed
                };
                self.fine = if result.is_some_and(|r| Some(r.step_ms) == self.fine_step_ms) {
                    Stage::Confirmed
                } else if self.fine == Stage::Running {
                    Stage::Unconfirmed
                } else {
                    Stage::Skipped
                };
            }
        }
    }

    pub fn stages(&self, separator: &str) -> String {
        let coarse = format!("100ms - {}", self.coarse.label());
        match self.fine_step_ms {
            Some(step) => format!("{coarse}{separator}{step}ms - {}", self.fine.label()),
            None => coarse,
        }
    }
}

struct LineOutput {
    round: RoundProgress,
    interactive: bool,
    preview_visible: bool,
}

impl LineOutput {
    fn new(interactive: bool) -> Self {
        Self {
            round: RoundProgress::default(),
            interactive,
            preview_visible: false,
        }
    }
    fn clear_preview(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.preview_visible {
            queue!(out, MoveToColumn(0), Clear(ClearType::CurrentLine))?;
            self.preview_visible = false;
        }
        Ok(())
    }

    fn preview(&mut self, out: &mut impl Write, text: &str) -> io::Result<()> {
        self.clear_preview(out)?;
        // Keep the temporary text on one physical row so replacing it cannot leave a stale row.
        let width = crossterm::terminal::size()
            .ok()
            .map(|(width, _)| width)
            .filter(|width| *width > 0)
            .unwrap_or(80);
        let mut remaining = usize::from(width.saturating_sub(1));
        let end = text
            .char_indices()
            .find_map(|(i, ch)| {
                let cells = ch.width().unwrap_or(0);
                if cells > remaining {
                    Some(i)
                } else {
                    remaining -= cells;
                    None
                }
            })
            .unwrap_or(text.len());
        self.preview_visible = true;
        write!(out, "{}", &text[..end])
    }

    fn event(&mut self, event: CalibrationEvent, out: &mut impl Write) -> io::Result<()> {
        self.round.observe(event);
        let timestamp = Local::now();
        let time = timestamp.format("%H:%M:%S");
        match event {
            CalibrationEvent::Preparing | CalibrationEvent::RoundRtt { .. } => return Ok(()),
            CalibrationEvent::Calibrating { number, step_ms } => {
                if !self.interactive {
                    return Ok(());
                }
                let confirmed = if step_ms == 100 { "" } else { "100ms - OK，" };
                self.preview(
                    out,
                    &format!("- [{time}] 檢測中，count = {number}，{confirmed}{step_ms}ms - ..."),
                )?;
            }
            CalibrationEvent::Initialized {
                mean_half_rtt_ms,
                fine_step_ms,
            } => {
                write!(
                    out,
                    "- [{time}] 初始化，RTT/2: {mean_half_rtt_ms:.1}ms，位移: 100ms"
                )?;
                if let Some(step) = fine_step_ms {
                    write!(out, " | {step}ms")?;
                }
                writeln!(out)?;
            }
            CalibrationEvent::Result {
                number,
                result,
                mean_half_rtt_ms,
            } => {
                self.clear_preview(out)?;
                write!(
                    out,
                    "- [{time}] 檢測中，count = {number}，{}",
                    self.round.stages("，")
                )?;
                match result {
                    Some(r) => write!(out, "，時間差: {:+.1} ms", r.diff_ms)?,
                    None => write!(out, "，時間差: 未取得")?,
                }
                match mean_half_rtt_ms {
                    Some(ms) => writeln!(out, "，RTT/2: {ms:.1}ms")?,
                    None => writeln!(out, "，RTT/2: 未取得")?,
                }
            }
        }
        out.flush()
    }
}

impl Drop for LineOutput {
    fn drop(&mut self) {
        if self.preview_visible {
            let mut out = io::stdout().lock();
            let _ = self.clear_preview(&mut out).and_then(|()| out.flush());
        }
    }
}

pub async fn run(client: &Client, url: &Url, options: CalibrationOptions) -> io::Result<bool> {
    let mut lines = LineOutput::new(io::stdout().is_terminal());
    let mut initialized = false;
    let success = calibrate(client, url, options, |event| {
        initialized |= matches!(event, CalibrationEvent::Initialized { .. });
        lines.event(event, &mut io::stdout().lock())
    })
    .await?;
    if !initialized {
        writeln!(
            io::stderr().lock(),
            "- [{}] 初始化失敗，RTT/2: 未取得，位移: 未決定",
            Local::now().format("%H:%M:%S")
        )?;
    }
    Ok(success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clock_ping::calibration::BoundaryResult;

    #[test]
    fn terminal_replaces_each_stage_in_place_and_adds_result_fields_only_at_completion() {
        let mut lines = LineOutput::new(true);
        let mut out = Vec::new();
        lines
            .event(
                CalibrationEvent::Initialized {
                    mean_half_rtt_ms: 60.0,
                    fine_step_ms: Some(25),
                },
                &mut out,
            )
            .unwrap();
        for (step_ms, expected) in [(100, "100ms - ..."), (25, "100ms - OK，25ms - ...")] {
            out.clear();
            lines
                .event(
                    CalibrationEvent::Calibrating { number: 1, step_ms },
                    &mut out,
                )
                .unwrap();
            let text = std::str::from_utf8(&out).unwrap();
            assert!(text.ends_with(&format!("] 檢測中，count = 1，{expected}")));
            assert!(
                !text.contains(['\n', '\r']) && !text.contains("RTT/2") && !text.contains("時間差")
            );
            out.clear();
            lines
                .event(
                    CalibrationEvent::RoundRtt {
                        number: 1,
                        mean_half_rtt_ms: 71.0,
                    },
                    &mut out,
                )
                .unwrap();
            assert!(out.is_empty());
        }
        lines
            .event(
                CalibrationEvent::Result {
                    number: 1,
                    result: Some(BoundaryResult {
                        diff_ms: -26.2,
                        step_ms: 25,
                    }),
                    mean_half_rtt_ms: Some(71.0),
                },
                &mut out,
            )
            .unwrap();
        assert!(
            std::str::from_utf8(&out)
                .unwrap()
                .ends_with("100ms - OK，25ms - OK，時間差: -26.2 ms，RTT/2: 71.0ms\n")
        );
        assert!(!lines.preview_visible);
    }

    #[test]
    fn reports_which_stages_confirmed_a_crossing_and_handles_missing_rtt() {
        let mut lines = LineOutput::new(false);
        let mut out = Vec::new();
        lines
            .event(
                CalibrationEvent::Initialized {
                    mean_half_rtt_ms: 60.0,
                    fine_step_ms: Some(25),
                },
                &mut out,
            )
            .unwrap();
        for (number, fine_started, step_ms, rtt, expected) in [
            (
                1,
                true,
                Some(25),
                Some(100.0),
                "100ms - OK，25ms - OK，時間差: -64.3 ms，RTT/2: 100.0ms",
            ),
            (
                2,
                true,
                Some(100),
                Some(80.0),
                "100ms - OK，25ms - 未確認，時間差: -64.3 ms，RTT/2: 80.0ms",
            ),
            (
                3,
                false,
                None,
                None,
                "100ms - 未確認，25ms - 未執行，時間差: 未取得，RTT/2: 未取得",
            ),
        ] {
            out.clear();
            lines
                .event(
                    CalibrationEvent::Calibrating {
                        number,
                        step_ms: 100,
                    },
                    &mut out,
                )
                .unwrap();
            if fine_started {
                lines
                    .event(
                        CalibrationEvent::Calibrating {
                            number,
                            step_ms: 25,
                        },
                        &mut out,
                    )
                    .unwrap();
            }
            assert!(out.is_empty());
            lines
                .event(
                    CalibrationEvent::Result {
                        number,
                        result: step_ms.map(|step_ms| BoundaryResult {
                            diff_ms: -64.3,
                            step_ms,
                        }),
                        mean_half_rtt_ms: rtt,
                    },
                    &mut out,
                )
                .unwrap();
            let text = std::str::from_utf8(&out).unwrap();
            assert!(text.ends_with(&format!("] 檢測中，count = {number}，{expected}\n")));
        }
    }
}
