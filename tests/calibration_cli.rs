use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::SystemTime;

#[test]
fn two_rounds_report_only_progress_and_signed_results() {
    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = server.local_addr().unwrap();
    let done = AtomicBool::new(false);
    let output = thread::scope(|scope| {
        scope.spawn(|| {
            for stream in server.incoming() {
                let mut stream = stream.unwrap();
                if done.load(Ordering::Acquire) {
                    break;
                }
                let reader = BufReader::new(&stream);
                for line in reader.lines() {
                    if line.unwrap().is_empty() { break; }
                }
                let date = httpdate::fmt_http_date(SystemTime::now());
                write!(stream, "HTTP/1.1 200 OK\r\nDate: {date}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            }
        });
        let output = Command::new(env!("CARGO_BIN_EXE_clock-ping"))
            .args([
                &format!("http://{addr}/clock"),
                "--calibrate",
                "--count",
                "2",
                "--min-step-ms",
                "25",
            ])
            .env("NO_PROXY", "127.0.0.1")
            .output();
        done.store(true, Ordering::Release);
        TcpStream::connect(addr).unwrap();
        output.unwrap()
    });
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("準備中"));
    for number in [1, 2] {
        for step in [100, 25] {
            assert_eq!(
                lines.next().unwrap(),
                format!("校正中 count = {number}（{step} ms）")
            );
        }
        let line = lines.next().unwrap();
        // A round may end with the confirmed coarse reading when its budget expires.
        let value = [25, 100]
            .into_iter()
            .find_map(|step| line.strip_prefix(&format!("結果 count = {number}（{step} ms）：")))
            .and_then(|s| s.strip_suffix(" ms"))
            .unwrap();
        assert!(value.starts_with(['+', '-']) && value.parse::<f64>().unwrap().is_finite());
    }
    assert_eq!(lines.next(), None);
}
