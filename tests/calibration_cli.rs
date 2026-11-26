use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::SystemTime;

struct Site {
    addr: SocketAddr,
    stopped: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Site {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                if done.load(Ordering::Acquire) {
                    break;
                }
                let mut request = BufReader::new(&stream).lines();
                let path_ok = request
                    .next()
                    .and_then(Result::ok)
                    .is_some_and(|line| line.split_whitespace().nth(1) == Some("/clock"));
                for line in request {
                    if line.unwrap_or_default().is_empty() {
                        break;
                    }
                }
                let status = if path_ok { "200 OK" } else { "404 Not Found" };
                let date = httpdate::fmt_http_date(SystemTime::now());
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nDate: {date}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            }
        });
        Self {
            addr,
            stopped,
            worker: Some(worker),
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_clock-ping"));
        command
            .arg(format!("http://{}/clock", self.addr))
            .env("NO_PROXY", "127.0.0.1");
        command
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.addr);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn default_prints_one_signed_result_per_round_then_exits_after_four() {
    let site = Site::start();
    let output = site.command().output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let mut lines = stdout.lines().map(|line| {
        let (timestamp, body) = line.strip_prefix("- [").unwrap().split_once("] ").unwrap();
        assert_eq!(timestamp.len(), 8);
        chrono::NaiveTime::parse_from_str(timestamp, "%H:%M:%S").unwrap();
        body
    });
    let (rtt, plan) = lines
        .next()
        .unwrap()
        .strip_prefix("初始化，RTT/2: ")
        .unwrap()
        .split_once("ms，位移: ")
        .unwrap();
    assert!(rtt.parse::<f64>().unwrap() >= 0.0);
    let mut steps = plan.split(" | ");
    assert_eq!(steps.next(), Some("100ms"));
    let fine = steps.next();
    assert!(matches!(fine, None | Some("10ms" | "20ms" | "25ms")));
    assert_eq!(steps.next(), None);
    for number in 1..=4 {
        let line = lines.next().unwrap();
        let (stages, fields) = line
            .strip_prefix(&format!("檢測中，count = {number}，100ms - OK"))
            .unwrap()
            .split_once("，時間差: ")
            .unwrap();
        if let Some(step) = fine {
            assert!(stages == format!("，{step} - OK") || stages == format!("，{step} - 未確認"));
        } else {
            assert!(stages.is_empty());
        }
        let (diff, rtt) = fields.split_once(" ms，RTT/2: ").unwrap();
        assert!(diff.starts_with(['+', '-']) && diff.parse::<f64>().unwrap().is_finite());
        assert!(rtt.strip_suffix("ms").unwrap().parse::<f64>().unwrap() >= 0.0);
    }
    assert_eq!(lines.next(), None);
}

#[cfg(unix)]
mod continuous {
    use super::*;
    use std::{
        io::Read,
        process::{Child, Stdio},
        sync::mpsc,
        time::{Duration, Instant},
    };

    struct Running(Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn interrupt_after(flag: &str, step: &str, ready: impl Fn(&str) -> bool) -> String {
        let site = Site::start();
        let mut child = Running(
            site.command()
                .args([flag, "--min-step-ms", step])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut stdout = child.0.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut buf = [0; 4096];
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut bytes = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut signalled = false;
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(chunk) => bytes.extend(chunk),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => panic!("timed out waiting for {flag}"),
            }
            if !signalled && ready(&String::from_utf8_lossy(&bytes)) {
                assert!(
                    Command::new("kill")
                        .arg("-INT")
                        .arg(child.0.id().to_string())
                        .status()
                        .unwrap()
                        .success()
                );
                signalled = true;
            }
        }
        reader.join().unwrap();
        assert!(signalled && child.0.wait().unwrap().success());
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn continuous_line_mode_reaches_fifth_round_and_stops_on_ctrl_c() {
        let text = interrupt_after("-t", "100", |s| s.contains("檢測中，count = 5，100ms - OK"));
        assert_eq!(text.lines().count(), 6);
        assert!(text.lines().next().unwrap().ends_with("位移: 100ms"));
    }

    #[test]
    fn monitor_ticks_between_results_and_restores_the_terminal_on_ctrl_c() {
        let reset = "檢測中:   count = 2, 100ms - 檢測中, 25ms - 待測, RTT/2: —";
        let next = "檢測中:   count = 3, 100ms - 檢測中, 25ms - 待測, RTT/2: —";
        let text = interrupt_after("--monitor", "25", |s| s.contains(next));
        let (_, screen) = text.split_once("\x1b[?1049h").unwrap();
        let (_, round_two) = screen.split_once(reset).unwrap();
        let (first, after) = ["OK", "未確認"]
            .into_iter()
            .find_map(|fine| {
                round_two.split_once(&format!(
                    "檢測中:   count = 2, 100ms - OK, 25ms - {fine}, RTT/2: "
                ))
            })
            .unwrap();
        let live = first
            .split_once("count = 2, 100ms - OK, 25ms - 檢測中, RTT/2: ")
            .unwrap()
            .1;
        assert!(live.split_once("ms").unwrap().0.parse::<f64>().unwrap() >= 0.0);
        assert!(screen.contains("最新時差: "));
        let final_rtt = after.split_once("ms").unwrap().0;
        assert!(final_rtt.parse::<f64>().unwrap() >= 0.0);
        let (before_reset, _) = after.split_once(next).unwrap();
        assert!(before_reset.contains(&format!(" ms, RTT/2: {final_rtt}ms")));
        let clocks: std::collections::HashSet<_> = first
            .split("\x1b[1;19H")
            .skip(1)
            .filter_map(|s| s.get(..12))
            .filter(|s| chrono::NaiveTime::parse_from_str(s, "%H:%M:%S%.3f").is_ok())
            .collect();
        assert!(
            clocks.len() > 10,
            "clock must keep ticking during the next calibration"
        );
        assert!(text.contains("\x1b[?1049l") && text.contains("\x1b[?25h"));
    }
}
