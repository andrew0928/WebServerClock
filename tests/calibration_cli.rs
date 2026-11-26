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
    let mut lines = stdout.lines();
    for number in 1..=4 {
        let line = lines.next().unwrap();
        let value = [10, 20, 25, 100]
            .into_iter()
            .find_map(|step| line.strip_prefix(&format!("結果 count = {number}（{step} ms）：")))
            .and_then(|s| s.strip_suffix(" ms"))
            .unwrap();
        assert!(value.starts_with(['+', '-']) && value.parse::<f64>().unwrap().is_finite());
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

    fn interrupt_after(flag: &str, ready: impl Fn(&str) -> bool) -> String {
        let site = Site::start();
        let mut child = Running(
            site.command()
                .args([flag, "--min-step-ms", "100"])
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
        let text = interrupt_after("-t", |s| s.contains("結果 count = 5（100 ms）"));
        assert_eq!(text.lines().count(), 5);
    }

    #[test]
    fn monitor_ticks_between_results_and_restores_the_terminal_on_ctrl_c() {
        let text = interrupt_after("--monitor", |s| s.contains("結果 count = 2（100 ms）"));
        let (_, screen) = text.split_once("\x1b[?1049h").unwrap();
        let (first, _) = screen.split_once("結果 count = 2（100 ms）").unwrap();
        assert!(first.contains("結果 count = 1（100 ms）"));
        assert!(first.contains("最新時差") && first.contains("校正中 count = 2"));
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
