use std::process::{Command, Output};

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_clock-ping"))
        .args(args)
        .output()
        .expect("run clock-ping")
}

#[test]
fn help_and_guides_work_without_a_target() {
    for flag in ["-h", "--help"] {
        let help = invoke(&[flag]);
        assert!(help.status.success());
        let text = String::from_utf8(help.stdout).unwrap();
        assert!(text.contains(concat!("clock-ping ", env!("CARGO_PKG_VERSION"))));
        assert!(text.contains("clock-ping https://example.com -t"));
        assert!(text.contains("-m, --monitor"));
        assert!(text.contains("--calibrate"));
    }

    let index = invoke(&["--skill"]);
    let explicit = invoke(&["--skill", "index"]);
    assert!(index.status.success() && explicit.status.success());
    assert_eq!(index.stdout, explicit.stdout);
    let topics = String::from_utf8(index.stdout).unwrap();
    assert!(topics.contains("clock-ping --skill measure"));
    assert!(topics.contains("clock-ping --skill target"));
    assert!(topics.contains("clock-ping --skill author"));

    let guide = invoke(&["--skill", "measure"]);
    assert!(guide.status.success());
    assert!(
        String::from_utf8(guide.stdout)
            .unwrap()
            .contains("clock-ping https://example.com")
    );
    let author = invoke(&["--skill", "author"]);
    assert!(author.status.success() && author.stderr.is_empty());
    let text = String::from_utf8(author.stdout).unwrap();
    for url in [
        "https://www.facebook.com/andrew.blog.0928/",
        "https://columns.chicken-house.net/",
        "https://columns.chicken-house.net/2000/01/01/about-me/",
        "https://columns.chicken-house.net/2017/01/05/webserverclock-release/",
        "https://github.com/andrew0928/WebServerClock",
    ] {
        assert!(
            text.contains(&format!("]({url})")),
            "missing author link {url}"
        );
    }
}

#[test]
fn target_guide_is_available_without_a_url() {
    let guide = invoke(&["--skill", "target"]);
    assert!(guide.status.success() && guide.stderr.is_empty());
    assert_eq!(guide.stdout, include_bytes!("../guides/target.md"));
}

#[test]
fn clap_rejects_invalid_invocations_before_probing() {
    let unknown = invoke(&["--skill", "missing"]);
    assert_eq!(unknown.status.code(), Some(2));
    let error = String::from_utf8(unknown.stderr).unwrap();
    assert!(error.contains("index") && error.contains("measure"));
    for args in [
        &[][..],
        &["-t"],
        &["-m"],
        &["--monitor"],
        &["--calibrate"],
        &["--calibrate", "--skill"],
        &["https://example.com", "--count", "2"],
        &["https://example.com", "--min-step-ms", "10"],
        &["https://example.com", "--calibrate", "--count", "0"],
        &["https://example.com", "--calibrate", "--min-step-ms", "0"],
        &["https://example.com", "--calibrate", "--min-step-ms", "101"],
        &["https://example.com", "--calibrate", "-m"],
        &["https://example.com", "--calibrate", "-t"],
        &["--bad-flag"],
        &["ftp://example.com"],
        &["https://example.com", "--skill"],
        &["--skill", "measure", "-t"],
        &["--skill", "measure", "-m"],
    ] {
        assert_eq!(invoke(args).status.code(), Some(2), "{args:?}");
    }
}
