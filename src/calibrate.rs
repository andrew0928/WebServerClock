use clock_ping::calibration::{CalibrationEvent, CalibrationOptions, calibrate};
use crossterm::{
    cursor::MoveToColumn,
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};
use reqwest::{Client, Url};
use std::io::{self, IsTerminal, Write};

// A terminal can show preparation and scan progress on one temporary line.
// Redirected output contains one complete line per round.
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

pub async fn run(client: &Client, url: &Url, options: CalibrationOptions) -> io::Result<bool> {
    let mut progress = Progress::default();
    let mut saw_result = false;
    let success = calibrate(client, url, options, |event| {
        progress.stage(event)?;
        if let CalibrationEvent::Result { number, result } = event {
            saw_result = true;
            let mut out = io::stdout().lock();
            match result {
                Some(r) => writeln!(
                    out,
                    "結果 count = {number}（{} ms）：{:+.1} ms",
                    r.step_ms, r.diff_ms
                )?,
                None => writeln!(out, "結果 count = {number}：未取得結果")?,
            }
            out.flush()?;
        }
        Ok(())
    })
    .await?;
    progress.clear()?;
    if !saw_result {
        eprintln!("準備失敗。");
    }
    Ok(success)
}
