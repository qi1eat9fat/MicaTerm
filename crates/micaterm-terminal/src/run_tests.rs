// 来自 since2006/shell-rs @ b2a307a: src/terminal/run_tests.rs（Agent CLI 工单 06）
// 只拷与平台无关的部分：用本机 `sh` 和 Docker 里的真 shell 跑的两组要上游的测试工具，
// MicaTerm 的 crate 里没有；真 shell 由工单 06 的 sshd 冒烟验。

use std::sync::mpsc;
use std::time::Duration;

use super::*;

/// Timing for runs whose terminal is a thread of the test.
const QUICK: RunTiming = RunTiming {
    settle: Duration::ZERO,
    probe: Duration::from_millis(200),
    start: Duration::from_millis(200),
    tick: Duration::from_millis(10),
};

fn nonce(run: &TerminalRun) -> String {
    lock(&run.state).nonce.clone()
}

fn marker(kind: &str, nonce: &str, payload: &str) -> String {
    format!("\x1b]6973;{kind};{nonce};{payload}\x07")
}

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// What was typed into the terminal so far, as text.
fn typed(input: &mpsc::Receiver<TerminalTransportCommand>) -> String {
    input
        .try_iter()
        .map(|command| match command {
            TerminalTransportCommand::Write(bytes) => String::from_utf8(bytes).unwrap(),
            _ => String::new(),
        })
        .collect()
}

/// A run a terminal took up, with the terminal's input.
fn accepted(
    command: &str,
) -> (
    TerminalRun,
    mpsc::Receiver<RunEvent>,
    mpsc::Receiver<TerminalTransportCommand>,
) {
    let (run, events) = TerminalRun::new(command.to_owned());
    let (input, typed) = mpsc::channel();
    assert!(run.handle().accept(input));
    assert_eq!(events.try_recv(), Ok(RunEvent::Accepted));
    (run, events, typed)
}

/// What the scanner found, with the output of successive chunks joined.
fn joined(found: Vec<Scanned>) -> Vec<Scanned> {
    let mut joined: Vec<Scanned> = Vec::new();
    for scanned in found {
        match (joined.last_mut(), scanned) {
            (Some(Scanned::Output(before)), Scanned::Output(more)) => before.extend(more),
            (_, scanned) => joined.push(scanned),
        }
    }
    joined
}

#[test]
fn markers_are_found_however_the_output_is_split() {
    let mut scanner = RunScanner::new("abc");
    let begin = marker("B", "abc", "6869");
    let mut found = scanner.scan(format!("$ echo\r\n{begin}out\r\nline2\r").as_bytes());
    found.extend(scanner.scan(b"\n\x1b]69"));
    found.extend(scanner.scan(b"73;E;abc;3\x07$ "));
    assert_eq!(
        joined(found),
        vec![
            Scanned::Begin(b"6869".to_vec()),
            Scanned::Output(b"out\nline2\n".to_vec()),
            Scanned::End(3),
        ]
    );
    // After E, the prompt is not the command's.
    assert!(scanner.scan(b"more\r\n").is_empty());
}

/// 真实服务器上录下的 bash 会话（提示符、标题、颜色、OSC 7）夹着这次运行的标记：
/// 只认标记，B 和 E 之间的才是命令的输出，切成多小的块都一样（MicaTerm 工单 06）。
#[test]
fn markers_are_found_in_recorded_bash_sessions() {
    for replay in [
        &include_bytes!("../testdata/remote/ubuntu-26.04-bash.bin")[..],
        &include_bytes!("../testdata/remote/debian-13-root-bash.bin")[..],
        &include_bytes!("../testdata/remote/rocky-9-bash.bin")[..],
    ] {
        let mut stream = replay.to_vec();
        stream.extend_from_slice(marker("P", "n0nce", "/usr/bin/bash").as_bytes());
        stream.extend_from_slice(replay);
        stream.extend_from_slice(marker("B", "n0nce", "6869").as_bytes());
        stream.extend_from_slice(b"out\r\nline2\r\n");
        stream.extend_from_slice(marker("E", "n0nce", "0").as_bytes());
        stream.extend_from_slice(replay);
        for chunk in [1, 7, 4096] {
            let mut scanner = RunScanner::new("n0nce");
            let found: Vec<Scanned> = stream
                .chunks(chunk)
                .flat_map(|bytes| scanner.scan(bytes))
                .collect();
            assert_eq!(
                joined(found),
                vec![
                    Scanned::Probe("/usr/bin/bash".into()),
                    Scanned::Begin(b"6869".to_vec()),
                    Scanned::Output(b"out\nline2\n".to_vec()),
                    Scanned::End(0),
                ],
                "chunk {chunk}"
            );
        }
    }
}

#[test]
fn only_this_runs_markers_count_and_either_terminator_ends_them() {
    let mut scanner = RunScanner::new("abc");
    let found = scanner.scan(
        format!(
            "{}{}\x1b]6973;P;abc;/usr/bin/bash\x1b\\",
            marker("P", "other", "/bin/zsh"),
            marker("E", "abc", "oops"),
        )
        .as_bytes(),
    );
    assert_eq!(
        found,
        vec![Scanned::End(255), Scanned::Probe("/usr/bin/bash".into())]
    );
    // The echo of a typed line has the escape as text.
    assert!(
        scanner
            .scan(br#" sh -c 'printf "\033]6973;P;%s" "$1"' sh abc"#)
            .is_empty()
    );
}

#[test]
fn a_lone_carriage_return_stays_and_crlf_becomes_lf() {
    let mut scanner = RunScanner::new("n");
    let found = scanner.scan(
        format!(
            "{}50%\r100%\r\n{}",
            marker("B", "n", ""),
            marker("E", "n", "0")
        )
        .as_bytes(),
    );
    assert_eq!(found[1], Scanned::Output(b"50%\r100%\n".to_vec()));
}

#[test]
fn shells_are_told_by_their_program() {
    assert_eq!(RunShell::of("/usr/bin/bash"), Some(RunShell::Bash));
    // Upgraded under the running shell.
    assert_eq!(
        RunShell::of("/usr/bin/bash (deleted)"),
        Some(RunShell::Bash)
    );
    assert_eq!(RunShell::of("/usr/bin/dash"), Some(RunShell::Dash));
    assert_eq!(RunShell::of("/bin/busybox\n"), Some(RunShell::Busybox));
    assert_eq!(RunShell::of("/usr/bin/zsh"), None);
    assert_eq!(RunShell::of(""), None);
    assert_eq!(program_name("/tmp/mysh"), "mysh");
    assert_eq!(RunShell::Busybox.line_limit(), 1000);
    assert_eq!(RunShell::Dash.line_limit(), 4000);
}

#[test]
fn the_context_keeps_a_directory_with_newlines_whole() {
    let context = RunContext::read(hex("root\nweb-01\n/srv/a\nb\nbash").as_bytes(), "dash");
    assert_eq!(
        context,
        RunContext {
            user: "root".into(),
            host: "web-01".into(),
            cwd: "/srv/a\nb".into(),
            shell: "dash".into(),
            interp: "bash".into(),
        }
    );
    // `od` missing: nothing known, the run goes on.
    assert_eq!(RunContext::read(b"", "bash").user, "");
}

#[test]
fn a_run_probes_then_types_its_command_and_reads_its_output() {
    let (run, events, input) = accepted("uname -a");
    let handle = run.handle();
    let nonce = nonce(&run);
    run.type_probe();
    let probe = typed(&input);
    assert!(probe.starts_with(" sh -c '") && probe.ends_with(&format!(" sh {nonce}\r")));

    assert!(handle.feed(marker("P", &nonce, "/usr/bin/bash").as_bytes()));
    assert_eq!(events.try_recv(), Ok(RunEvent::Typed));
    let command = typed(&input);
    assert!(
        command.ends_with(&format!(" sh {nonce} 'uname -a'\r")),
        "{command}"
    );

    let context = hex("root\nweb-01\n/root\nbash");
    let output = format!(
        "{}Linux\r\n{}$ ",
        marker("B", &nonce, &context),
        marker("E", &nonce, "0")
    );
    assert!(!handle.feed(output.as_bytes()));
    let RunEvent::Begun(context) = events.try_recv().unwrap() else {
        panic!("not begun");
    };
    assert_eq!(
        (context.user.as_str(), context.shell.as_str()),
        ("root", "bash")
    );
    assert_eq!(events.try_recv(), Ok(RunEvent::Output(b"Linux\n".to_vec())));
    assert_eq!(events.try_recv(), Ok(RunEvent::Exited(0)));
    // Finished: dropping it sends nothing.
    drop(run);
    assert_eq!(typed(&input), "");
}

#[test]
fn an_answer_after_the_run_was_given_up_types_nothing() {
    let (run, events, input) = accepted("rm -rf build");
    let handle = run.handle();
    run.type_probe();
    typed(&input);
    assert!(run.give_up(BusyReason::NoAnswer));
    assert!(!handle.feed(marker("P", &nonce(&run), "/usr/bin/bash").as_bytes()));
    assert_eq!(typed(&input), "");
    assert!(events.try_recv().is_err());
    assert!(!handle.is_active());
}

#[test]
fn only_a_typed_command_gets_a_ctrl_c() {
    let (run, _events, input) = accepted("true");
    run.type_probe();
    typed(&input);
    // Probing: the probe is harmless where it waits.
    assert!(!run.give_up(BusyReason::NotStarted));
    run.cancel();
    assert_eq!(typed(&input), "");

    let (run, _events, input) = accepted("sleep 9");
    run.type_probe();
    run.handle()
        .feed(marker("P", &nonce(&run), "/bin/busybox").as_bytes());
    typed(&input);
    drop(run);
    assert_eq!(typed(&input), "\x03");
}

#[test]
fn a_shell_other_than_the_three_or_a_line_too_long_types_nothing() {
    let (run, events, input) = accepted("true");
    run.type_probe();
    typed(&input);
    run.handle()
        .feed(marker("P", &nonce(&run), "/usr/bin/zsh").as_bytes());
    assert_eq!(
        events.try_recv(),
        Ok(RunEvent::Failed(RunFailure::UnsupportedShell("zsh".into())))
    );
    assert_eq!(typed(&input), "");

    let (run, events, input) = accepted(&"x".repeat(1000));
    run.type_probe();
    typed(&input);
    run.handle()
        .feed(marker("P", &nonce(&run), "/bin/busybox").as_bytes());
    assert_eq!(
        events.try_recv(),
        Ok(RunEvent::Failed(RunFailure::TooLong { limit: 1000 }))
    );
    assert_eq!(typed(&input), "");
}

#[test]
fn a_terminal_gone_says_whether_the_command_may_have_run() {
    let (run, events, _input) = accepted("true");
    run.type_probe();
    run.handle().disconnected();
    assert_eq!(
        events.try_recv(),
        Ok(RunEvent::Failed(RunFailure::NotConnected))
    );

    let (run, events, _input) = accepted("true");
    run.type_probe();
    run.handle()
        .feed(marker("P", &nonce(&run), "/usr/bin/bash").as_bytes());
    events.try_recv().unwrap();
    run.handle().disconnected();
    assert_eq!(
        events.try_recv(),
        Ok(RunEvent::Failed(RunFailure::Disconnected))
    );
}

/// How a fake shell answers the command line.
#[derive(Clone, Copy)]
enum Answer {
    /// B, this output and E with 7.
    Runs(&'static str),
    /// B alone: still running.
    Starts,
    /// Nothing: held back.
    Nothing,
}

/// Answers a run's lines the way a shell would, on a thread: P at once,
/// then the command line as `answer` says. Stops once nothing is typed
/// for a while (the run's handle keeps the input open); everything typed.
fn fake_shell(
    run: &TerminalRun,
    input: mpsc::Receiver<TerminalTransportCommand>,
    answer: Answer,
) -> std::thread::JoinHandle<String> {
    let (handle, nonce) = (run.handle(), nonce(run));
    std::thread::spawn(move || {
        let mut all = String::new();
        while let Ok(TerminalTransportCommand::Write(bytes)) =
            input.recv_timeout(Duration::from_millis(500))
        {
            let line = String::from_utf8(bytes).unwrap();
            all.push_str(&line);
            let reply = if line.contains("6973;P;") {
                marker("P", &nonce, "/usr/bin/bash")
            } else if line.contains("6973;B;") {
                match answer {
                    Answer::Runs(output) => format!(
                        "{}{output}{}",
                        marker("B", &nonce, ""),
                        marker("E", &nonce, "7")
                    ),
                    Answer::Starts => marker("B", &nonce, ""),
                    Answer::Nothing => continue,
                }
            } else {
                continue;
            };
            handle.feed(reply.as_bytes());
        }
        all
    })
}

#[test]
fn driving_a_run_passes_its_output_on_and_ends_with_its_code() {
    let (run, events, input) = accepted("make");
    let shell = fake_shell(&run, input, Answer::Runs("built\r\n"));
    let mut progress = Vec::new();
    let code = run.drive(&events, QUICK, &mut |step| {
        if step != RunProgress::Idle {
            progress.push(step);
        }
        Ok(())
    });
    assert_eq!(code, Ok(7));
    assert!(matches!(progress[0], RunProgress::Context(_)));
    assert_eq!(progress[1], RunProgress::Output(b"built\n".to_vec()));
    drop(run);
    assert!(!shell.join().unwrap().contains('\x03'));
}

#[test]
fn a_run_nobody_answers_or_that_never_starts_is_busy() {
    let (run, events, _input) = accepted("true");
    assert_eq!(
        run.drive(&events, QUICK, &mut |_| Ok(())),
        Err(RunFailure::Busy(BusyReason::NoAnswer))
    );

    // Held back by a command filter: withdrawn with ^C.
    let (run, events, input) = accepted("reboot");
    let shell = fake_shell(&run, input, Answer::Nothing);
    assert_eq!(
        run.drive(&events, QUICK, &mut |_| Ok(())),
        Err(RunFailure::Busy(BusyReason::NotStarted))
    );
    drop(run);
    assert!(shell.join().unwrap().ends_with('\x03'));
}

#[test]
fn a_caller_gone_cancels_the_command() {
    let (run, events, input) = accepted("sleep 100");
    let shell = fake_shell(&run, input, Answer::Starts);
    let mut begun = false;
    let result = run.drive(&events, QUICK, &mut |step| match step {
        RunProgress::Context(_) => {
            begun = true;
            Ok(())
        }
        RunProgress::Idle if begun => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
        _ => Ok(()),
    });
    assert_eq!(result, Err(RunFailure::Disconnected));
    drop(run);
    assert!(shell.join().unwrap().ends_with('\x03'));
}

#[test]
fn the_encoded_command_has_nothing_a_line_editor_or_quotes_would_take() {
    let encoded = encode_command("echo 'it''s' \"$HOME\" a\\b !x\t中\n".as_bytes());
    assert!(encoded.bytes().all(|byte| (0x20..0x7f).contains(&byte)));
    assert!(!encoded.contains(['\'', '"', '!']));
    assert!(!encoded.contains("\\\\"));
    assert!(encoded.starts_with("echo \\0047it"), "{encoded}");
    assert!(encoded.contains("$HOME"));
}
