//! 外部 CLI 从命令到应用走一遍：真的在临时端点上听（Unix 上是临时目录里的套接字，
//! Windows 上是一个随机名字的命名管道），命令这一端用 `client::run` 发请求、收回复。
// 参考 since2006/shell-rs @ b2a307a: src/cli/tests.rs（只留主机列表和查看的几条）

use std::io::Write as _;
use std::path::{Path, PathBuf};

use micaterm_hosts::{AuthKind, HostData, HostDraft};

use std::sync::{Arc, Mutex};

use micaterm_ssh::ExecStream;

use super::client::{self, Console, FAILURE_EXIT};
use super::protocol::{
    CliError, ErrorCode, FrameKind, HostDetails, HostInfo, Reply, TransferCounters,
    TransferSummary, parse_json, read_frame, write_frame,
};
use super::{CliBackend, CliServer, CliService, CliStatus, CliTarget, Request, targets};

/// 假的后端：记下收到的命令；`fail` 时一律失败，否则 stdout 回命令本身、stderr 回一句，退出码 3。
#[derive(Default)]
struct FakeBackend {
    commands: Mutex<Vec<(String, String)>>,
    fail: Option<CliError>,
}

impl CliBackend for FakeBackend {
    fn exec(
        &self,
        target: &CliTarget,
        command: &str,
        output: &mut dyn FnMut(ExecStream, &[u8]) -> std::io::Result<()>,
    ) -> Result<i32, CliError> {
        self.commands
            .lock()
            .unwrap()
            .push((target.info.name.clone(), command.to_string()));
        if let Some(error) = &self.fail {
            return Err(error.clone());
        }
        let _ = output(ExecStream::Stdout, format!("{command}\n").as_bytes());
        let _ = output(ExecStream::Stderr, "警告\n".as_bytes());
        Ok(3)
    }

    /// 上传：记下来，报一次进度，一个文件成功。
    fn upload(
        &self,
        target: &CliTarget,
        source: &Path,
        destination: &str,
        progress: &mut dyn FnMut(TransferCounters) -> std::io::Result<()>,
    ) -> Result<TransferSummary, CliError> {
        self.record(target, format!("upload {} {destination}", source.display()));
        let _ = progress(TransferCounters {
            files: 0,
            total_files: 1,
            bytes: 0,
            total_bytes: 3,
        });
        Ok(TransferSummary {
            files: 1,
            bytes: 3,
            ..TransferSummary::default()
        })
    }

    /// 下载：一个成功、一个失败。
    fn download(
        &self,
        target: &CliTarget,
        source: &str,
        destination: &Path,
        _: &mut dyn FnMut(TransferCounters) -> std::io::Result<()>,
    ) -> Result<TransferSummary, CliError> {
        self.record(
            target,
            format!("download {source} {}", destination.display()),
        );
        Ok(TransferSummary {
            files: 1,
            bytes: 3,
            failed: 1,
            failures: vec!["/var/log/secure: 权限不足".into()],
            ..TransferSummary::default()
        })
    }

    /// 同步：没变的跳过，`delete` 时删掉两个。
    fn sync(
        &self,
        target: &CliTarget,
        source: &Path,
        destination: &str,
        delete: bool,
        _: &mut dyn FnMut(TransferCounters) -> std::io::Result<()>,
    ) -> Result<TransferSummary, CliError> {
        self.record(
            target,
            format!("sync {} {destination} {delete}", source.display()),
        );
        Ok(TransferSummary {
            files: 1,
            skipped: 4,
            deleted: if delete { 2 } else { 0 },
            ..TransferSummary::default()
        })
    }
}

impl FakeBackend {
    fn record(&self, target: &CliTarget, what: String) {
        self.commands
            .lock()
            .unwrap()
            .push((target.info.name.clone(), what));
    }
}

/// 一个只给这个测试用的端点，和它所在的临时目录（Unix 上套接字放在里面）。
fn endpoint() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = if cfg!(windows) {
        // 管道名整台机器共用：进程号加一个计数，并行的测试和别的测试进程各用各的。
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        PathBuf::from(format!(
            r"\\.\pipe\micaterm-cli-test-{}-{next}",
            std::process::id()
        ))
    } else {
        dir.path().join("cli.sock")
    };
    (dir, path)
}

/// 两台主机：web 存了密码，db 用无密码登录。
fn data() -> HostData {
    let mut data = HostData::empty();
    data.insert_unnotified(HostDraft::new(
        "web-01",
        "10.0.0.2",
        22,
        "root",
        AuthKind::Password,
    ));
    data.insert_unnotified(HostDraft::new(
        "db",
        "10.0.0.3",
        2222,
        "deploy",
        AuthKind::NoPassword,
    ));
    data
}

/// 发一个请求，拿回退出码、stdout 和 stderr。
fn run(endpoint: &Path, request: Request, json: bool) -> (i32, String, String) {
    run_as(endpoint, request, json, false)
}

/// 同 [`run`]，`exec_json` 是 `exec --json`。
fn run_as(endpoint: &Path, request: Request, json: bool, exec_json: bool) -> (i32, String, String) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = client::run(
        endpoint,
        request,
        &mut Console {
            stdout: &mut stdout,
            stderr: &mut stderr,
            json,
            exec_json,
            stderr_is_terminal: false,
        },
    );
    (
        code,
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

fn started() -> (tempfile::TempDir, PathBuf, CliServer, HostData) {
    let (dir, endpoint) = endpoint();
    let server = CliServer::start(endpoint.clone()).unwrap();
    let data = data();
    let saved = data.hosts()[0].password_secret();
    server.set_targets(targets(&data, |secret| *secret == saved, false));
    (dir, endpoint, server, data)
}

#[test]
fn the_hosts_are_listed_and_shown_once_the_cli_is_enabled() {
    let (_dir, endpoint, server, data) = started();

    // 开关关着：说「没启用」，不是「没运行」，Agent 才知道让用户去开哪个开关。
    let (code, _, stderr) = run(&endpoint, Request::List { query: None }, true);
    assert_eq!(code, FAILURE_EXIT);
    assert!(stderr.contains("[not_enabled]"), "{stderr}");

    server.set_enabled(true);
    let (code, stdout, _) = run(&endpoint, Request::List { query: None }, true);
    assert_eq!(code, 0);
    let hosts: Vec<HostInfo> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        hosts
            .iter()
            .map(|host| host.name.as_str())
            .collect::<Vec<_>>(),
        ["web-01", "db"]
    );
    assert_eq!(hosts[0].id, data.hosts()[0].public_id.as_str());

    let (_, stdout, _) = run(
        &endpoint,
        Request::List {
            query: Some("deploy".into()),
        },
        true,
    );
    let hosts: Vec<HostInfo> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "db");

    let (code, stdout, _) = run(
        &endpoint,
        Request::ShowHost {
            host: hosts[0].id.clone(),
        },
        true,
    );
    assert_eq!(code, 0);
    let db: HostDetails = serde_json::from_str(&stdout).unwrap();
    assert_eq!((db.host.as_str(), db.port), ("10.0.0.3", 2222));
    assert_eq!(db.password_saved, None);

    let web_id = data.hosts()[0].public_id.to_string();
    let (_, stdout, _) = run(&endpoint, Request::ShowHost { host: web_id }, true);
    let web: HostDetails = serde_json::from_str(&stdout).unwrap();
    assert_eq!(web.password_saved, Some(true));
    // 秘密从不出现在输出里，只说存没存。
    assert!(!stdout.contains("password\":"), "{stdout}");
}

#[test]
fn an_unknown_host_is_named_by_its_code() {
    let (_dir, endpoint, server, _) = started();
    server.set_enabled(true);
    let (code, stdout, stderr) = run(
        &endpoint,
        Request::ShowHost {
            host: "NoSuchHost000000".into(),
        },
        false,
    );
    assert_eq!(code, FAILURE_EXIT);
    assert!(stdout.is_empty());
    assert!(stderr.starts_with("micaterm: [host_not_found]"), "{stderr}");
    assert!(stderr.contains("NoSuchHost000000"), "{stderr}");
}

#[test]
fn the_list_follows_the_hosts_the_app_hands_over() {
    let (_dir, endpoint, server, mut data) = started();
    server.set_enabled(true);
    data.insert_unnotified(HostDraft::new(
        "cache",
        "10.0.0.4",
        22,
        "root",
        AuthKind::Password,
    ));
    server.set_targets(targets(&data, |_| false, false));
    let (_, stdout, _) = run(&endpoint, Request::List { query: None }, false);
    // 表格：表头一行，一台主机一行。
    assert_eq!(stdout.lines().count(), 4, "{stdout}");
    assert!(stdout.contains("root@10.0.0.4:22"), "{stdout}");
}

#[test]
fn a_command_from_another_version_hears_so() {
    let (_dir, endpoint, server, _) = started();
    server.set_enabled(true);
    let reply = raw(&endpoint, br#"{"version":999,"request":{"type":"list"}}"#);
    assert!(
        matches!(
            reply,
            Reply::Error {
                code: ErrorCode::VersionMismatch,
                ..
            }
        ),
        "{reply:?}"
    );
    // 同一版本里不认识的请求：说 bad_request，命令那一端会解释成「应用较旧」。
    let reply = raw(&endpoint, br#"{"version":1,"request":{"type":"teleport"}}"#);
    assert!(
        matches!(reply, Reply::Error { code: ErrorCode::BadRequest, ref message, .. }
            if message.contains("unknown variant")),
        "{reply:?}"
    );
}

#[test]
fn nothing_listening_is_not_running() {
    let (_dir, endpoint) = endpoint();
    let (code, _, stderr) = run(&endpoint, Request::List { query: None }, false);
    assert_eq!(code, FAILURE_EXIT);
    assert!(stderr.contains("[not_running]"), "{stderr}");
}

#[test]
fn a_second_app_on_the_same_endpoint_leaves_it_to_the_first() {
    let (_dir, endpoint, _server, _) = started();
    assert_eq!(
        CliService::start_at(endpoint.clone(), Arc::new(FakeBackend::default())).status(),
        CliStatus::Elsewhere
    );
}

#[test]
fn the_endpoint_is_free_again_once_the_app_is_gone() {
    let (_dir, endpoint, server, _) = started();
    drop(server);
    let again = CliService::start_at(endpoint, Arc::new(FakeBackend::default()));
    assert_eq!(again.status(), CliStatus::Listening);
}

#[test]
fn exec_streams_the_output_and_exits_with_the_remote_code() {
    let (_dir, endpoint, server, data) = started();
    let backend = Arc::new(FakeBackend::default());
    server.set_backend(backend.clone());
    server.set_enabled(true);
    let web = data.hosts()[0].public_id.to_string();

    let (code, stdout, stderr) = run(
        &endpoint,
        Request::Exec {
            host: web.clone(),
            command: "uname -a".into(),
        },
        false,
    );
    assert_eq!(code, 3);
    assert_eq!(stdout, "uname -a\n");
    assert_eq!(stderr, "警告\n");
    assert_eq!(
        *backend.commands.lock().unwrap(),
        [("web-01".to_string(), "uname -a".to_string())]
    );

    // `exec --json`：命令结束才打印一个 ASCII 的 JSON 对象，输出分开放。
    let (code, stdout, stderr) = run_as(
        &endpoint,
        Request::Exec {
            host: web,
            command: "df -h".into(),
        },
        true,
        true,
    );
    assert_eq!(code, 3);
    assert!(stderr.is_empty(), "{stderr}");
    assert!(stdout.is_ascii(), "{stdout}");
    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(result["exit_code"], 3);
    assert_eq!(result["stdout"], "df -h\n");
    assert_eq!(result["stderr"], "警告\n");
}

#[test]
fn a_closed_stdout_ends_quietly_like_sigpipe() {
    /// `| head` 读够就走了的 stdout。
    struct Closed;
    impl std::io::Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let (_dir, endpoint, server, data) = started();
    server.set_backend(Arc::new(FakeBackend::default()));
    server.set_enabled(true);
    let mut stderr = Vec::new();
    let code = client::run(
        &endpoint,
        Request::Exec {
            host: data.hosts()[0].public_id.to_string(),
            command: "yes".into(),
        },
        &mut Console {
            stdout: &mut Closed,
            stderr: &mut stderr,
            json: false,
            exec_json: false,
            stderr_is_terminal: false,
        },
    );
    assert_eq!(code, client::OUTPUT_CLOSED_EXIT);
    // 不说「和 MicaTerm 的连接中断」：断的是自己的 stdout。
    assert!(
        !String::from_utf8(stderr)
            .unwrap()
            .contains("connect_failed")
    );
}

#[test]
fn exec_json_reports_errors_as_json_too() {
    let (_dir, endpoint, server, _) = started();
    server.set_backend(Arc::new(FakeBackend {
        fail: Some(CliError::new(ErrorCode::HostKeyUnknown, "尚未信任")),
        ..FakeBackend::default()
    }));
    server.set_enabled(true);
    let (code, stdout, _) = run_as(
        &endpoint,
        Request::Exec {
            host: "NoSuchHost000000".into(),
            command: "id".into(),
        },
        true,
        true,
    );
    assert_eq!(code, FAILURE_EXIT);
    let error: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "host_not_found");
}

#[test]
fn a_locked_vault_holds_exec_back_and_asks_the_app_to_say_so() {
    let (_dir, endpoint) = endpoint();
    let server = CliServer::start(endpoint.clone()).unwrap();
    let backend = Arc::new(FakeBackend::default());
    server.set_backend(backend.clone());
    server.set_enabled(true);
    let data = data();
    let saved = data.hosts()[0].password_secret();
    // web 的密码在保险库里，保险库锁着；db 无密码登录，用不到保险库。
    server.set_targets(targets(&data, |secret| *secret == saved, true));
    let (web, db) = (
        data.hosts()[0].public_id.to_string(),
        data.hosts()[1].public_id.to_string(),
    );

    assert!(!server.take_vault_wanted());
    let (code, _, stderr) = run(
        &endpoint,
        Request::Exec {
            host: web,
            command: "id".into(),
        },
        false,
    );
    assert_eq!(code, FAILURE_EXIT);
    assert!(stderr.contains("[vault_locked]"), "{stderr}");
    assert!(backend.commands.lock().unwrap().is_empty(), "没去连");
    assert!(server.take_vault_wanted());
    assert!(!server.take_vault_wanted(), "取一次就清掉");

    let (code, ..) = run(
        &endpoint,
        Request::Exec {
            host: db,
            command: "id".into(),
        },
        false,
    );
    assert_eq!(code, 3);
    assert!(!server.take_vault_wanted());
}

#[test]
fn transfers_go_to_the_backend_with_absolute_local_paths_and_print_a_summary() {
    let (_dir, endpoint, server, data) = started();
    let backend = Arc::new(FakeBackend::default());
    server.set_backend(backend.clone());
    server.set_enabled(true);
    let web = data.hosts()[0].public_id.to_string();
    let local = std::env::temp_dir().join("dist");

    let (code, stdout, _) = run(
        &endpoint,
        Request::Upload {
            host: web.clone(),
            source: local.clone(),
            destination: "/srv/app".into(),
        },
        true,
    );
    assert_eq!(code, 0);
    let summary: TransferSummary = serde_json::from_str(&stdout).unwrap();
    assert_eq!((summary.files, summary.bytes), (1, 3));

    // 有失败：退出码 1，摘要一行，失败的写在 stderr。
    let (code, stdout, stderr) = run(
        &endpoint,
        Request::Download {
            host: web.clone(),
            source: "/var/log".into(),
            destination: local.clone(),
        },
        false,
    );
    assert_eq!(code, super::PARTIAL_EXIT);
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert_eq!(stderr, "/var/log/secure: 权限不足\n");

    // 同步的跳过说成「未变」。
    let (code, stdout, _) = run(
        &endpoint,
        Request::Sync {
            host: web,
            source: local.clone(),
            destination: "~/app".into(),
            delete: true,
        },
        false,
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("未变 4 个"), "{stdout}");
    assert!(stdout.contains("删除 2 个"), "{stdout}");

    let local = local.display().to_string();
    assert_eq!(
        *backend.commands.lock().unwrap(),
        [
            ("web-01".to_string(), format!("upload {local} /srv/app")),
            ("web-01".to_string(), format!("download /var/log {local}")),
            ("web-01".to_string(), format!("sync {local} ~/app true")),
        ]
    );
}

#[test]
fn a_relative_local_path_is_refused_by_the_app() {
    let (_dir, endpoint, server, data) = started();
    let backend = Arc::new(FakeBackend::default());
    server.set_backend(backend.clone());
    server.set_enabled(true);
    let (code, _, stderr) = run(
        &endpoint,
        Request::Upload {
            host: data.hosts()[0].public_id.to_string(),
            source: "dist".into(),
            destination: "/srv".into(),
        },
        false,
    );
    assert_eq!(code, FAILURE_EXIT);
    assert!(stderr.contains("[bad_request]"), "{stderr}");
    assert!(backend.commands.lock().unwrap().is_empty());
}

/// 绕过命令那一端，直接写一个请求帧，读回一个回复。
fn raw(endpoint: &Path, payload: &[u8]) -> Reply {
    #[cfg(unix)]
    let stream = std::os::unix::net::UnixStream::connect(endpoint).unwrap();
    #[cfg(windows)]
    let stream = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(endpoint)
        .unwrap();
    let mut writer = &stream;
    write_frame(&mut writer, FrameKind::Json, payload).unwrap();
    writer.flush().unwrap();
    let mut reader = &stream;
    let (kind, payload) = read_frame(&mut reader).unwrap().unwrap();
    assert_eq!(kind, FrameKind::Json);
    parse_json(&payload).unwrap()
}

// ---- 交给 Agent 的卡片（工单 06） ----

/// 应用那一端：等一个 `exec --card` 进来。
fn take_run(server: &CliServer) -> (String, micaterm_terminal::RunHandle) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(taken) = server.take_run() {
            return taken;
        }
        assert!(std::time::Instant::now() < deadline, "没有 run 进来");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// 应用那一端：等一个改动进来。
fn take_change(server: &CliServer) -> (super::CliChange, super::ChangeReply) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(taken) = server.take_change() {
            return taken;
        }
        assert!(std::time::Instant::now() < deadline, "没有改动进来");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// 卡片的终端接下 `run`，shell 的程序是 `shell`：应答探测，再给命令行回 `output` 和 `code`。
/// 返回打进去的全部字节（不再有输入时）。
// 来自 since2006/shell-rs @ b2a307a: src/cli/tests.rs 的 `answer_run`
fn answer_run(
    run: micaterm_terminal::RunHandle,
    shell: &'static str,
    output: &'static str,
    code: i32,
) -> std::thread::JoinHandle<String> {
    use micaterm_terminal::TerminalTransportCommand;
    let (input, typed) = std::sync::mpsc::channel();
    assert!(run.accept(input));
    std::thread::spawn(move || {
        let mut all = String::new();
        while let Ok(TerminalTransportCommand::Write(bytes)) =
            typed.recv_timeout(std::time::Duration::from_millis(500))
        {
            let line = String::from_utf8(bytes.to_vec()).unwrap();
            all.push_str(&line);
            // ` sh -c '<script>' sh NONCE …`
            let Some(at) = line.find("' sh ") else {
                continue;
            };
            let nonce = &line[at + 5..at + 17];
            if line.contains("6973;P;") {
                run.feed(format!("\x1b]6973;P;{nonce};{shell}\x07").as_bytes());
            } else if line.contains("6973;B;") {
                let context: String = "deploy\nasset\n/home/deploy\nbash"
                    .bytes()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                run.feed(format!("\x1b]6973;B;{nonce};{context}\x07").as_bytes());
                std::thread::sleep(std::time::Duration::from_millis(100));
                run.feed(format!("{output}\x1b]6973;E;{nonce};{code}\x07").as_bytes());
            }
        }
        all
    })
}

fn card(id: &str, running: bool) -> super::CardInfo {
    super::CardInfo {
        id: id.into(),
        host: "WendaoSmoke00001".into(),
        host_name: "问道".into(),
        connected: true,
        running,
    }
}

#[test]
fn the_cards_handed_over_are_listed() {
    let (_dir, endpoint, server, _data) = started();
    server.set_enabled(true);
    let (code, _, stderr) = run(&endpoint, Request::ListCards, false);
    assert_eq!(code, 0);
    assert!(stderr.contains("交给 Agent"), "{stderr}");

    server.set_cards(vec![
        card("CardAAAAAAAAAAAA", false),
        card("CardBBBBBBBBBBBB", true),
    ]);
    let (code, stdout, _) = run(&endpoint, Request::ListCards, false);
    assert_eq!(code, 0);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "{stdout}");
    assert!(lines[1].starts_with("CardAAAAAAAAAAAA  问道"), "{stdout}");
    assert!(lines[1].ends_with("已连接"), "{stdout}");
    assert!(lines[2].ends_with("执行中"), "{stdout}");

    let (_, json, _) = run(&endpoint, Request::ListCards, true);
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(json[1]["id"], "CardBBBBBBBBBBBB");
    assert_eq!(json[1]["running"], true);
}

#[test]
fn a_command_typed_into_a_card_passes_on_its_output_and_where_it_ran() {
    let (_dir, endpoint, server, _data) = started();
    server.set_enabled(true);
    let exec = |exec_json| {
        let endpoint = endpoint.clone();
        std::thread::spawn(move || {
            let request = Request::ExecInCard {
                card: "CardAAAAAAAAAAAA".into(),
                command: "ls /srv".into(),
            };
            let (code, stdout, _) = run_as(&endpoint, request, false, exec_json);
            (code, stdout)
        })
    };

    let command = exec(false);
    let (card, run) = take_run(&server);
    assert_eq!(card, "CardAAAAAAAAAAAA");
    let terminal = answer_run(run, "/usr/bin/bash", "a\r\nb\r\n", 2);
    assert_eq!(command.join().unwrap(), (2, "a\nb\n".to_string()));
    assert!(terminal.join().unwrap().contains(" 'ls /srv'\r"));

    let command = exec(true);
    answer_run(take_run(&server).1, "/bin/busybox", "a\r\n", 0);
    let (code, result) = command.join().unwrap();
    assert_eq!(code, 0);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({
            "exit_code": 0,
            "stdout": "a\n",
            "stderr": "",
            "context": {
                "user": "deploy",
                "host": "asset",
                "cwd": "/home/deploy",
                "shell": "busybox",
                "interp": "bash",
            },
        })
    );
}

#[test]
fn a_command_that_cannot_be_typed_into_a_card_says_why() {
    let (_dir, endpoint, server, _data) = started();
    server.set_enabled(true);
    let exec = || {
        let endpoint = endpoint.clone();
        std::thread::spawn(move || {
            let request = Request::ExecInCard {
                card: "CardAAAAAAAAAAAA".into(),
                command: "true".into(),
            };
            let (code, _, stderr) = run(&endpoint, request, false);
            (code, stderr)
        })
    };
    let refused = |command: std::thread::JoinHandle<(i32, String)>, code: &str| {
        let (exit, stderr) = command.join().unwrap();
        assert_eq!(exit, FAILURE_EXIT);
        assert!(
            stderr.starts_with(&format!("micaterm: [{code}] ")),
            "{stderr}"
        );
        stderr
    };

    // 没交出去、收回了的卡片。
    let command = exec();
    take_run(&server)
        .1
        .fail(micaterm_terminal::RunFailure::NoTerminal);
    refused(command, "no_terminal");

    // 打不进去的 shell：除了探测什么也没打。
    let command = exec();
    let terminal = answer_run(take_run(&server).1, "/usr/bin/zsh", "", 0);
    assert!(refused(command, "unsupported_shell").contains("zsh"));
    assert!(!terminal.join().unwrap().contains("6973;B;"));

    // 没人应答探测。
    let command = exec();
    let (input, _typed) = std::sync::mpsc::channel();
    assert!(take_run(&server).1.accept(input));
    refused(command, "terminal_busy");

    // 应用没接：收回，从没打进去。
    let command = exec();
    refused(command, "connect_failed");
    assert!(server.take_run().is_none());

    // 关掉外部 CLI 时排着的一律回绝。
    let command = exec();
    std::thread::sleep(std::time::Duration::from_millis(100));
    server.set_enabled(false);
    let (exit, _) = command.join().unwrap();
    assert_eq!(exit, FAILURE_EXIT);
    assert!(server.take_run().is_none());
}

#[test]
fn reading_a_card_and_pressing_a_key_go_to_the_app() {
    let (_dir, endpoint, server, _data) = started();
    server.set_enabled(true);
    let ask = |request: Request, json: bool| {
        let endpoint = endpoint.clone();
        std::thread::spawn(move || run(&endpoint, request, json))
    };

    let read = ask(
        Request::ReadCard {
            card: "CardAAAAAAAAAAAA".into(),
            lines: Some(5),
        },
        false,
    );
    let (change, reply) = take_change(&server);
    assert!(
        matches!(
            &change,
            super::CliChange::ReadCard { card, lines: Some(5) } if card == "CardAAAAAAAAAAAA"
        ),
        "{change:?}"
    );
    reply.send(Ok(Reply::Screen(super::CardScreen {
        text: "$ less x".into(),
        returned_lines: 1,
        history_available: 0,
        truncated: false,
        alternate_screen: true,
    })));
    let (code, stdout, stderr) = read.join().unwrap();
    assert_eq!((code, stdout.as_str()), (0, "$ less x\n"));
    assert!(stderr.contains("全屏程序"), "{stderr}");

    let key = ask(
        Request::SendKey {
            card: "CardAAAAAAAAAAAA".into(),
            key: "q".into(),
            repeat: None,
        },
        true,
    );
    let (change, reply) = take_change(&server);
    assert!(
        matches!(change, super::CliChange::SendKey { .. }),
        "{change:?}"
    );
    reply.send(Ok(Reply::KeySent {
        key: "q".into(),
        repeat: 1,
    }));
    let (code, stdout, _) = key.join().unwrap();
    assert_eq!(code, 0);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stdout).unwrap(),
        serde_json::json!({"key": "q", "repeat": 1})
    );
}

// ---- `micaterm env`（工单 07） ----

/// 没开、开着没启用、启用了：`env` 都答得上，问到的卡片跟着变。
#[test]
fn env_says_whether_micaterm_is_there_and_the_card_still_handed() {
    use super::env::{AppState, report};
    let vars = |name: &str| match name {
        "TERM_PROGRAM" => Some("micaterm".to_string()),
        "MICATERM_SELF_CARD" => Some("SelfSelfSelfSelf".to_string()),
        "MICATERM_CARD" => Some("CardAAAAAAAAAAAA".to_string()),
        _ => None,
    };
    let (_dir, endpoint) = endpoint();
    let asked = |endpoint: &Path| client::query(endpoint, Request::ListCards);

    let nobody = report(vars, asked(&endpoint));
    assert_eq!(nobody.app, AppState::NotRunning);
    assert_eq!(nobody.self_card.as_deref(), Some("SelfSelfSelfSelf"));

    let server = CliServer::start(endpoint.clone()).unwrap();
    let off = report(vars, asked(&endpoint));
    assert_eq!(off.app, AppState::NotEnabled);
    assert!(off.reason.is_some());

    server.set_enabled(true);
    server.set_cards(vec![card("CardAAAAAAAAAAAA", false)]);
    let on = report(vars, asked(&endpoint));
    assert_eq!(on.app, AppState::Running);
    assert_eq!(on.card.as_ref().unwrap().handed, Some(true));
    assert_eq!(on.card.as_ref().unwrap().host_name.as_deref(), Some("问道"));

    server.set_cards(Vec::new());
    let taken_back = report(vars, asked(&endpoint));
    assert_eq!(taken_back.card.as_ref().unwrap().handed, Some(false));
}

// ---- 状态 hook（工单 09） ----

/// `micaterm hook` 报来的事排着等应用取；外部 Agent 没启用时不收。
#[test]
fn status_hooks_wait_for_the_app_and_are_dropped_while_disabled() {
    use crate::agent::HookEvent;
    let (_dir, endpoint, server, _data) = started();
    let hook = |event| Request::Hook {
        card: "SelfSelfSelfSelf".into(),
        event,
        message: Some("Claude needs your permission".into()),
    };

    let (code, _, stderr) = run(&endpoint, hook(HookEvent::NeedsAttention), false);
    assert_eq!(code, FAILURE_EXIT, "没启用：{stderr}");
    assert_eq!(server.take_hook(), None);

    server.set_enabled(true);
    let (code, stdout, _) = run(&endpoint, hook(HookEvent::NeedsAttention), false);
    assert_eq!((code, stdout.as_str()), (0, ""));
    assert_eq!(
        server.take_hook(),
        Some((
            "SelfSelfSelfSelf".to_string(),
            HookEvent::NeedsAttention,
            Some("Claude needs your permission".to_string())
        ))
    );
    assert_eq!(server.take_hook(), None);

    // 关掉时排着的丢掉。
    run(&endpoint, hook(HookEvent::TurnDone), false);
    server.set_enabled(false);
    assert_eq!(server.take_hook(), None);
}
