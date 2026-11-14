//! SFTP 真协议的冒烟测试：`SshSftpTransportProvider` 对着进程内的 SSH 测试服务器登录，
//! 经 OpenSSH 的 `sftp-server`（Windows 上是 Git for Windows 带的）列目录、上传、读文件、原地写回、
//! 下载、冲突、停下后续传、断线自动重连（工单 22）。
//! 这台机器上没有 `sftp-server` 时跳过。路径当不透明的前缀用：Git 的 `sftp-server` 报的是 MSYS 风格。

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use micaterm_ssh::testing::{TEST_PASSWORD, TEST_USER, TestServer, has_sftp_server};
use micaterm_ssh::{
    ConnectionPrompt, ConnectionPromptKind, ConnectionPromptReply, ConnectionSecret, HostLogin,
};
use micaterm_ssh::{LoginMethod, SshConnector};
use tempfile::TempDir;

use super::sftp::{
    DownloadRequest, RemotePath, SaveFailure, SftpCommand, SftpEvent, SftpTransportProvider as _,
    SshSftpTransportProvider, TransferAnswer, TransferChoice, TransferPhase, TransferQuestion,
    TransferQuestionKind, UploadRequest,
};
use super::tests::Secrets;

/// 只防挂死，不是引擎该多快。卡住的连接（没有 FIN 也没有 RST）要等一个 SFTP 请求超时（10 秒）
/// 引擎才说断了；满载时再慢一些也不算错。
const WAIT: Duration = Duration::from_secs(30);

/// 一个登录好的 SFTP 工作线程。
struct Session {
    commands: Sender<SftpCommand>,
    events: Receiver<SftpEvent>,
    home: RemotePath,
    /// 工作线程：`Shutdown` 之后它结束，运行时也就没了。
    worker: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
    /// 服务器和运行时都活到测试结束。
    _server: Arc<Served>,
}

/// 服务器和它的运行时。
struct Served {
    server: TestServer,
    _runtime: tokio::runtime::Runtime,
}

fn serve(directory: &Path) -> Arc<Served> {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let server = runtime
        .block_on(TestServer::start(directory.to_path_buf()))
        .expect("server");
    Arc::new(Served {
        server,
        _runtime: runtime,
    })
}

impl Session {
    /// 登录。第一次连的主机密钥信任；问密码就回答。
    fn open(directory: &Path, state: &Path) -> Self {
        let served = serve(directory);
        let port = served.server.port;
        Self::attach(served, port, state)
    }

    /// 经 `port` 登录 `server`。同一台服务器再连一次，续传记录才对得上。
    fn attach(server: Arc<Served>, port: u16, state: &Path) -> Self {
        let connector = SshConnector::new(
            state.join("known_hosts"),
            std::sync::Arc::new(Secrets::default()),
        );
        let provider = SshSftpTransportProvider::new(
            connector,
            state.join("upload-resume"),
            state.join("download-resume"),
        );
        let login = HostLogin::new("127.0.0.1", port, TEST_USER, LoginMethod::Password);
        let transport = provider.create(&login);
        let (commands, receiver) = async_channel::unbounded();
        let (sender, events) = async_channel::unbounded();
        let worker = std::thread::spawn(move || transport.run(receiver, sender));
        let mut session = Self {
            commands,
            events,
            home: RemotePath::new("/").expect("path"),
            worker: Some(worker),
            _server: server,
        };
        session.home = session.wait_connected();
        session
    }

    fn wait_connected(&self) -> RemotePath {
        loop {
            match self.next() {
                SftpEvent::Connected { home } => return home,
                SftpEvent::Prompt(prompt) => self.answer_prompt(&prompt),
                SftpEvent::Disconnected(why) => panic!("couldn't connect: {why}"),
                _ => {}
            }
        }
    }

    fn answer_prompt(&self, prompt: &ConnectionPrompt) {
        let reply = match prompt.kind() {
            ConnectionPromptKind::UnknownHost(_) => ConnectionPromptReply::TrustAndSave,
            ConnectionPromptKind::Authentication(_) => {
                ConnectionPromptReply::Answers(vec![ConnectionSecret::new(
                    TEST_PASSWORD.to_string(),
                )])
            }
            other => panic!("unexpected question: {other:?}"),
        };
        self.send(SftpCommand::PromptReply {
            request_id: prompt.request_id(),
            reply,
        });
    }

    fn upload(&self, source: &Path) {
        self.send(SftpCommand::Upload(
            UploadRequest::new(vec![source.to_path_buf()], self.home.clone()).unwrap(),
        ));
    }

    fn download(&self, name: &str, target: &Path) {
        self.send(SftpCommand::Download(
            DownloadRequest::new(vec![self.home.join(name).unwrap()], target.to_path_buf())
                .unwrap(),
        ));
    }

    /// 等这一批走到 `phase`；路上的问题交给 `answer`。
    fn wait_phase(
        &self,
        phase: TransferPhase,
        mut answer: impl FnMut(&TransferQuestion) -> TransferChoice,
    ) {
        loop {
            match self.next() {
                SftpEvent::Progress(progress) if progress.phase() == phase => return,
                SftpEvent::Progress(progress) if progress.phase() == TransferPhase::Stopped => {
                    panic!("the batch stopped: {:?}", progress.details())
                }
                SftpEvent::Question(question) => self.send(SftpCommand::Answer {
                    request_id: question.id(),
                    answer: TransferAnswer::new(answer(&question), false),
                }),
                SftpEvent::Prompt(prompt) => self.answer_prompt(&prompt),
                _ => {}
            }
        }
    }

    /// 等这一批传了一些字节。
    fn wait_some_bytes(&self) {
        loop {
            match self.next() {
                SftpEvent::Progress(progress) if progress.completed_bytes() > 0 => return,
                SftpEvent::Question(question) => panic!("unexpected question: {question:?}"),
                _ => {}
            }
        }
    }

    /// 停下这一批，等引擎说完。这一批不能在停下之前就传完。
    fn stop(&self) {
        self.send(SftpCommand::Cancel);
        self.wait_phase(TransferPhase::Stopped, |question| {
            panic!("unexpected question: {question:?}")
        });
        self.wait_idle();
    }

    fn wait_idle(&self) {
        while !matches!(self.next(), SftpEvent::Idle) {}
    }

    fn send(&self, command: SftpCommand) {
        self.commands.try_send(command).expect("the worker listens");
    }

    fn next(&self) -> SftpEvent {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Ok(event) = self.events.try_recv() {
                return event;
            }
            assert!(Instant::now() < deadline, "no event in time");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// 列目录，返回里面的名字。
    fn list(&self, path: &RemotePath) -> Result<Vec<String>, String> {
        self.send(SftpCommand::List {
            request_id: 7,
            path: path.clone(),
        });
        loop {
            if let SftpEvent::Listed { request_id, result } = self.next() {
                assert_eq!(request_id, 7);
                return result.map(|listing| {
                    listing
                        .entries()
                        .iter()
                        .map(|entry| entry.name().to_string())
                        .collect()
                });
            }
        }
    }
}

impl Session {
    /// 关掉：和关 SFTP 标签发的一样，再等工作线程（和它的运行时）结束。
    fn close_and_join(&mut self) {
        self.send(SftpCommand::Disconnect);
        self.send(SftpCommand::Shutdown);
        let worker = self.worker.take().expect("still running");
        let deadline = Instant::now() + WAIT * 4;
        while !worker.is_finished() {
            assert!(Instant::now() < deadline, "the worker did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = worker.join();
    }

    /// 这一批再也不动：`Resume` 只是连上去，答一个 `Idle`。
    fn resume_goes_nowhere(&self) {
        self.send(SftpCommand::Resume);
        loop {
            match self.next() {
                SftpEvent::Idle => return,
                SftpEvent::Progress(progress) => panic!("it went on: {:?}", progress.phase()),
                SftpEvent::Question(question) => panic!("it went on: {question:?}"),
                SftpEvent::Prompt(prompt) => self.answer_prompt(&prompt),
                _ => {}
            }
        }
    }

    /// 连接断了：引擎说 `Disconnected`。
    fn wait_disconnected(&self) {
        while !matches!(self.next(), SftpEvent::Disconnected(_)) {}
    }

    /// 传着的这一批断了线：引擎说 `Disconnected`（这时调用 `dropped`），自己重连，传完。
    fn reconnect_and_finish(&self, mut dropped: impl FnMut()) {
        let (mut was_dropped, mut reconnecting) = (false, false);
        loop {
            match self.next() {
                SftpEvent::Disconnected(_) => {
                    if !was_dropped {
                        dropped();
                    }
                    was_dropped = true;
                }
                SftpEvent::Prompt(prompt) => self.answer_prompt(&prompt),
                SftpEvent::Question(question) => panic!("unexpected question: {question:?}"),
                SftpEvent::Progress(progress) => match progress.phase() {
                    TransferPhase::Reconnecting => reconnecting = true,
                    TransferPhase::Stopped => panic!("gave up: {:?}", progress.details()),
                    TransferPhase::Completed => break,
                    _ => {}
                },
                _ => {}
            }
        }
        assert!(was_dropped && reconnecting, "the connection never dropped");
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.commands.try_send(SftpCommand::Shutdown);
    }
}

/// 够大：走 `.filepart`，也来得及在传完之前停下或者断开。
const LARGE: usize = 48 * 1024 * 1024;

/// 不是一个字节重复到底：续传接错位置时内容对不上。
fn large_file(path: &Path) -> Vec<u8> {
    let bytes: Vec<u8> = (0..LARGE).map(|i| (i % 251) as u8).collect();
    std::fs::write(path, &bytes).unwrap();
    bytes
}

/// 同样内容才算对，不打印 48 MB。
fn assert_same(path: &Path, expected: &[u8]) {
    let actual = std::fs::read(path).unwrap();
    assert!(actual == expected, "{} 的内容不对", path.display());
}

/// 在客户端和服务器之间转发 TCP：`kick` 掐断现在的连接，新连接照常转发；`refuse` 以后新连接一来就断，
/// 像断了网；`pause` 以后连着的连接什么都收不到，像网卡住了。
struct Relay {
    port: u16,
    connections: Arc<Mutex<Vec<TcpStream>>>,
    refusing: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
}

impl Relay {
    fn start(target: u16) -> Self {
        let connections = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let tracked = connections.clone();
        let refusing = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let (refused, held) = (refusing.clone(), paused.clone());
        std::thread::spawn(move || {
            for client in listener.incoming() {
                let Ok(client) = client else { return };
                if refused.load(Ordering::SeqCst) {
                    continue;
                }
                let Ok(server) = TcpStream::connect(("127.0.0.1", target)) else {
                    return;
                };
                let _ = (client.set_nodelay(true), server.set_nodelay(true));
                tracked.lock().unwrap().extend(client.try_clone());
                tracked.lock().unwrap().extend(server.try_clone());
                pump(&client, &server, held.clone());
                pump(&server, &client, held.clone());
            }
        });
        Self {
            port,
            connections,
            refusing,
            paused,
        }
    }

    fn refuse(&self, refusing: bool) {
        self.refusing.store(refusing, Ordering::SeqCst);
    }

    fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    fn kick(&self) {
        for connection in self.connections.lock().unwrap().drain(..) {
            let _ = connection.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// 一个方向。一边断了就断另一边。卡住时读到的先拿着，放开了再转。
fn pump(from: &TcpStream, to: &TcpStream, paused: Arc<AtomicBool>) {
    let (Ok(mut from), Ok(mut to)) = (from.try_clone(), to.try_clone()) else {
        return;
    };
    std::thread::spawn(move || {
        let mut buffer = vec![0; 64 * 1024];
        while let Ok(read) = from.read(&mut buffer) {
            while paused.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
            if read == 0 || to.write_all(&buffer[..read]).is_err() {
                break;
            }
        }
        let _ = to.shutdown(std::net::Shutdown::Both);
    });
}

fn directories() -> Option<(TempDir, TempDir)> {
    has_sftp_server().then(|| {
        (
            tempfile::tempdir().expect("served directory"),
            tempfile::tempdir().expect("state directory"),
        )
    })
}

#[test]
fn the_sftp_connection_lists_the_home_and_a_folder_that_is_not_there_is_a_reason() {
    let Some((served, state)) = directories() else {
        return;
    };
    std::fs::write(served.path().join("hello.txt"), "hi").unwrap();
    std::fs::create_dir(served.path().join("sub")).unwrap();
    let session = Session::open(served.path(), state.path());

    let names = session.list(&session.home).unwrap();
    assert!(names.contains(&"hello.txt".to_string()), "{names:?}");
    assert!(names.contains(&"sub".to_string()), "{names:?}");

    let missing = session.home.join("no-such-folder").unwrap();
    let reason = session.list(&missing).unwrap_err();
    assert!(reason.starts_with("无法读取目录"), "{reason}");
}

#[test]
fn a_file_is_uploaded_read_and_written_back_in_place() {
    let Some((served, state)) = directories() else {
        return;
    };
    let source = state.path().join("up.txt");
    std::fs::write(&source, "uploaded").unwrap();
    let session = Session::open(served.path(), state.path());

    session.send(SftpCommand::Upload(
        UploadRequest::new(vec![source], session.home.clone()).unwrap(),
    ));
    loop {
        match session.next() {
            SftpEvent::Progress(progress) if progress.phase() == TransferPhase::Completed => break,
            SftpEvent::Question(question) => panic!("unexpected question: {question:?}"),
            _ => {}
        }
    }
    assert_eq!(
        std::fs::read_to_string(served.path().join("up.txt")).unwrap(),
        "uploaded"
    );

    let path = session.home.join("up.txt").unwrap();
    session.send(SftpCommand::ReadFile {
        request_id: 8,
        path: path.clone(),
    });
    let file = loop {
        if let SftpEvent::FileRead { result, .. } = session.next() {
            break result.expect("the file reads");
        }
    };
    assert_eq!(file.text(), "uploaded");

    session.send(SftpCommand::WriteFile {
        request_id: 9,
        path,
        bytes: b"edited".to_vec(),
        expected: Some(file.stamp()),
    });
    loop {
        if let SftpEvent::FileWritten { result, .. } = session.next() {
            result.expect("the file is written back");
            break;
        }
    }
    assert_eq!(
        std::fs::read_to_string(served.path().join("up.txt")).unwrap(),
        "edited"
    );
}

/// 覆盖要先问，问题带着两边的大小；大文件经 `.filepart` 和备份换上去，不留下临时文件。
#[test]
fn overwriting_a_large_file_asks_and_replaces_it_safely() {
    let Some((served, state)) = directories() else {
        return;
    };
    let source = state.path().join("big.bin");
    let bytes = vec![7u8; 300 * 1024];
    std::fs::write(&source, &bytes).unwrap();
    std::fs::write(served.path().join("big.bin"), b"old").unwrap();
    let session = Session::open(served.path(), state.path());

    session.upload(&source);
    let mut asked = None;
    session.wait_phase(TransferPhase::Completed, |question| {
        asked = question.conflict().copied();
        TransferChoice::Overwrite
    });

    let facts = asked.expect("the conflict says what is on both sides");
    assert_eq!(facts.incoming().size(), bytes.len() as u64);
    assert_eq!(facts.existing().map(|existing| existing.size()), Some(3));
    assert_same(&served.path().join("big.bin"), &bytes);
    let left: Vec<_> = std::fs::read_dir(served.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["big.bin"], "no temporary file stays behind");
}

#[test]
fn a_file_is_downloaded_and_a_conflict_can_be_skipped() {
    let Some((served, state)) = directories() else {
        return;
    };
    std::fs::write(served.path().join("down.txt"), "from the server").unwrap();
    let target = state.path().join("in");
    std::fs::create_dir(&target).unwrap();
    let session = Session::open(served.path(), state.path());

    session.download("down.txt", &target);
    session.wait_phase(TransferPhase::Completed, |question| {
        panic!("unexpected question: {question:?}")
    });
    session.wait_idle();
    assert_eq!(
        std::fs::read_to_string(target.join("down.txt")).unwrap(),
        "from the server"
    );

    std::fs::write(served.path().join("down.txt"), "changed").unwrap();
    session.download("down.txt", &target);
    session.wait_phase(TransferPhase::Completed, |question| {
        assert_eq!(question.kind(), TransferQuestionKind::Conflict);
        TransferChoice::Skip
    });
    assert_eq!(
        std::fs::read_to_string(target.join("down.txt")).unwrap(),
        "from the server"
    );
}

/// 停下的上传留着 `.filepart` 和续传记录。「继续」接着传；标签关了再传同一个文件会问是否续传。
#[test]
fn a_stopped_upload_resumes_now_or_in_a_later_connection() {
    let Some((directory, state)) = directories() else {
        return;
    };
    let source = state.path().join("up.bin");
    let bytes = large_file(&source);
    let part = directory.path().join("up.bin.filepart");
    let served = serve(directory.path());
    let port = served.server.port;
    let session = Session::attach(served.clone(), port, state.path());

    session.upload(&source);
    session.wait_some_bytes();
    session.stop();
    assert!(part.exists(), "the part stays while stopped");

    // 继续：同一批。
    session.send(SftpCommand::Resume);
    session.wait_some_bytes();
    session.stop();
    drop(session);

    // 关了再传：问是否续传。
    let session = Session::attach(served.clone(), port, state.path());
    session.upload(&source);
    let mut asked = Vec::new();
    session.wait_phase(TransferPhase::Completed, |question| {
        asked.push(question.kind());
        TransferChoice::Resume
    });
    assert_eq!(asked, [TransferQuestionKind::Resume]);
    assert_same(&directory.path().join("up.bin"), &bytes);
    assert!(!part.exists(), "the part became the file");
}

#[test]
fn a_stopped_download_resumes_in_a_later_connection() {
    let Some((directory, state)) = directories() else {
        return;
    };
    let bytes = large_file(&directory.path().join("down.bin"));
    let target = state.path().join("in");
    std::fs::create_dir(&target).unwrap();
    let served = serve(directory.path());
    let port = served.server.port;
    let session = Session::attach(served.clone(), port, state.path());

    session.download("down.bin", &target);
    session.wait_some_bytes();
    session.stop();
    let part = target.join("down.bin.filepart");
    assert!(part.exists(), "the part stays while stopped");
    drop(session);

    let session = Session::attach(served.clone(), port, state.path());
    session.download("down.bin", &target);
    let mut asked = Vec::new();
    session.wait_phase(TransferPhase::Completed, |question| {
        asked.push(question.kind());
        TransferChoice::Resume
    });
    assert_eq!(asked, [TransferQuestionKind::Resume]);
    assert_same(&target.join("down.bin"), &bytes);
    assert!(!part.exists(), "the part became the file");
}

/// 连接断了：批次自己重连，接着传完，不用用户动手。
#[test]
fn a_dropped_connection_reconnects_on_its_own_and_the_upload_finishes() {
    let Some((directory, state)) = directories() else {
        return;
    };
    let source = state.path().join("up.bin");
    let bytes = large_file(&source);
    let served = serve(directory.path());
    let relay = Relay::start(served.server.port);
    let session = Session::attach(served.clone(), relay.port, state.path());

    session.upload(&source);
    session.wait_some_bytes();
    relay.kick();
    session.reconnect_and_finish(|| {});
    assert_same(&directory.path().join("up.bin"), &bytes);
}

/// 传的时候网卡住了（没有 FIN 也没有 RST）：一个 SFTP 请求超时后引擎说断了，自己重连，接着传完。
/// 有的机器上往卡住的连接写，非阻塞的 `send` 也会挡住线程；引擎不能跟着停在那里一声不吭。
#[test]
fn a_connection_stuck_during_an_upload_is_given_up_and_the_upload_finishes() {
    let Some((directory, state)) = directories() else {
        return;
    };
    let source = state.path().join("up.bin");
    let bytes = large_file(&source);
    let served = serve(directory.path());
    let relay = Relay::start(served.server.port);
    let session = Session::attach(served.clone(), relay.port, state.path());

    session.upload(&source);
    session.wait_some_bytes();
    relay.pause(true);
    // 引擎说断了以后网才好：卡住的那条掐掉，新连接照常转发。
    session.reconnect_and_finish(|| {
        relay.kick();
        relay.pause(false);
    });
    assert_same(&directory.path().join("up.bin"), &bytes);
}

/// 移出队列：引擎忘掉这一批，`.filepart` 留着；之后的 `Resume`（重新连接也发它）不会把它接着传。
#[test]
fn a_forgotten_batch_keeps_its_part_and_is_not_resumed() {
    let Some((directory, state)) = directories() else {
        return;
    };
    let source = state.path().join("up.bin");
    large_file(&source);
    let part = directory.path().join("up.bin.filepart");
    let session = Session::open(directory.path(), state.path());

    session.upload(&source);
    session.wait_some_bytes();
    session.stop();

    session.send(SftpCommand::Forget);
    session.wait_idle();
    session.send(SftpCommand::Resume);
    loop {
        match session.next() {
            SftpEvent::Idle => break,
            SftpEvent::Progress(progress) => panic!("it went on: {:?}", progress.phase()),
            _ => {}
        }
    }
    assert!(part.exists(), "the part stays for next time");
    assert!(!directory.path().join("up.bin").exists());
}

/// 编辑器的冲突框要的数据：别人改过文件时什么都不写，回答里带着文件现在的样子；
/// 覆盖就不核对。BOM 和 CRLF 在真协议上也原样来回。
#[test]
fn a_save_over_someone_elses_change_is_refused_with_what_the_file_is_now() {
    let Some((served, state)) = directories() else {
        return;
    };
    let on_disk = served.path().join("app.conf");
    std::fs::write(&on_disk, b"\xEF\xBB\xBFmine\r\n").unwrap();
    let session = Session::open(served.path(), state.path());
    let path = session.home.join("app.conf").unwrap();

    session.send(SftpCommand::ReadFile {
        request_id: 1,
        path: path.clone(),
    });
    let file = loop {
        if let SftpEvent::FileRead { result, .. } = session.next() {
            break result.expect("the file reads");
        }
    };
    assert_eq!(file.text(), "mine\n");

    std::fs::write(&on_disk, "someone else's, longer\n").unwrap();
    let bytes = file.format().encode("mine 2\n");
    session.send(SftpCommand::WriteFile {
        request_id: 2,
        path: path.clone(),
        bytes: bytes.clone(),
        expected: Some(file.stamp()),
    });
    loop {
        if let SftpEvent::FileWritten { result, .. } = session.next() {
            match result {
                Err(SaveFailure::Changed { now: Some(now) }) => assert_eq!(now.size(), 23),
                other => panic!("{other:?}"),
            }
            break;
        }
    }
    assert_eq!(
        std::fs::read_to_string(&on_disk).unwrap(),
        "someone else's, longer\n"
    );

    session.send(SftpCommand::WriteFile {
        request_id: 3,
        path,
        bytes,
        expected: None,
    });
    loop {
        if let SftpEvent::FileWritten { result, .. } = session.next() {
            result.expect("overwriting does not check");
            break;
        }
    }
    assert_eq!(std::fs::read(&on_disk).unwrap(), b"\xEF\xBB\xBFmine 2\r\n");
}

/// 写回是截断重写：写到一半关标签（断开加 `Shutdown`），工作线程也要等它写完才结束，运行时才不会把它中止，
/// 否则文件就只剩一半，还没人知道。
#[test]
fn closing_the_connection_during_a_save_lets_the_save_finish() {
    let Some((served, state)) = directories() else {
        return;
    };
    let on_disk = served.path().join("app.conf");
    std::fs::write(&on_disk, vec![0u8; LARGE]).unwrap();
    let mut session = Session::open(served.path(), state.path());
    let bytes: Vec<u8> = (0..LARGE).map(|i| (i % 251) as u8).collect();

    session.send(SftpCommand::WriteFile {
        request_id: 1,
        path: session.home.join("app.conf").unwrap(),
        bytes: bytes.clone(),
        expected: None,
    });
    // 文件已经截断了：从这里起中止写回就丢数据。
    let deadline = Instant::now() + WAIT;
    while std::fs::metadata(&on_disk).unwrap().len() >= LARGE as u64 {
        assert!(Instant::now() < deadline, "the save never started");
        std::thread::sleep(Duration::from_millis(1));
    }
    session.close_and_join();

    assert_same(&on_disk, &bytes);
}

/// 断网时丢弃：连不上也算丢弃了，之后的「重新连接」（`Resume`）不能把它接着传。
#[test]
fn a_batch_discarded_while_offline_does_not_go_on_after_reconnecting() {
    let Some((directory, state)) = directories() else {
        return;
    };
    let source = state.path().join("up.bin");
    large_file(&source);
    let served = serve(directory.path());
    let relay = Relay::start(served.server.port);
    let session = Session::attach(served.clone(), relay.port, state.path());

    session.upload(&source);
    session.wait_some_bytes();
    session.stop();
    relay.refuse(true);
    relay.kick();
    session.wait_disconnected();

    session.send(SftpCommand::Discard);
    session.wait_idle();

    relay.refuse(false);
    session.resume_goes_nowhere();
    assert!(!directory.path().join("up.bin").exists());
}

/// 下载的半截在本机：断网时丢弃也清得掉。
#[test]
fn a_download_discarded_while_offline_cleans_up_without_a_connection() {
    let Some((directory, state)) = directories() else {
        return;
    };
    large_file(&directory.path().join("down.bin"));
    let target = state.path().join("in");
    std::fs::create_dir(&target).unwrap();
    let served = serve(directory.path());
    let relay = Relay::start(served.server.port);
    let session = Session::attach(served.clone(), relay.port, state.path());

    session.download("down.bin", &target);
    session.wait_some_bytes();
    session.stop();
    let part = target.join("down.bin.filepart");
    assert!(part.exists(), "the part stays while stopped");
    relay.refuse(true);
    relay.kick();
    session.wait_disconnected();

    session.send(SftpCommand::Discard);
    session.wait_idle();
    assert!(!part.exists(), "the part is on this computer: it goes");

    relay.refuse(false);
    session.resume_goes_nowhere();
    assert!(!target.join("down.bin").exists());
}

/// 扫描时停下：引擎手里还没有批次。「继续」按原来的请求重新扫描，一直传完。
#[test]
fn a_batch_stopped_while_scanning_scans_again_on_resume() {
    let Some((directory, state)) = directories() else {
        return;
    };
    let source = state.path().join("up.bin");
    let bytes = large_file(&source);
    let served = serve(directory.path());
    let relay = Relay::start(served.server.port);
    let session = Session::attach(served.clone(), relay.port, state.path());

    // 网卡住了：扫描停在等服务器回答上。
    relay.pause(true);
    session.upload(&source);
    session.send(SftpCommand::Cancel);
    let stopped = loop {
        if let SftpEvent::Progress(progress) = session.next()
            && progress.phase() == TransferPhase::Stopped
        {
            break progress;
        }
    };
    assert!(stopped.details().is_empty(), "stopping is not a failure");
    session.wait_idle();
    relay.pause(false);

    session.send(SftpCommand::Resume);
    session.wait_phase(TransferPhase::Completed, |question| {
        panic!("unexpected question: {question:?}")
    });
    assert_same(&directory.path().join("up.bin"), &bytes);
}

/// 扫描时断线：和传的时候断线一样自己重连，接着扫描、传完。
#[test]
fn a_connection_dropped_while_scanning_reconnects_on_its_own() {
    let Some((directory, state)) = directories() else {
        return;
    };
    let source = state.path().join("up.bin");
    std::fs::write(&source, "uploaded").unwrap();
    let served = serve(directory.path());
    let relay = Relay::start(served.server.port);
    let session = Session::attach(served.clone(), relay.port, state.path());

    relay.pause(true);
    session.upload(&source);
    // 第一条进度是扫描开始了，后面就在等服务器回答。
    while !matches!(session.next(), SftpEvent::Progress(_)) {}
    relay.kick();
    relay.pause(false);

    let (mut dropped, mut reconnecting) = (false, false);
    loop {
        match session.next() {
            SftpEvent::Disconnected(_) => dropped = true,
            SftpEvent::Prompt(prompt) => session.answer_prompt(&prompt),
            SftpEvent::Question(question) => panic!("unexpected question: {question:?}"),
            SftpEvent::Progress(progress) => match progress.phase() {
                TransferPhase::Reconnecting => reconnecting = true,
                TransferPhase::Stopped => panic!("gave up: {:?}", progress.details()),
                TransferPhase::Completed => break,
                _ => {}
            },
            _ => {}
        }
    }
    assert!(dropped && reconnecting, "the connection never dropped");
    assert_eq!(
        std::fs::read_to_string(directory.path().join("up.bin")).unwrap(),
        "uploaded"
    );
}

/// 「上传文件夹…」（工单 35）：选中的文件夹整个传上去，子目录、深一层的文件和空文件夹都在。
#[test]
fn a_folder_with_subfolders_is_uploaded_whole() {
    let Some((served, state)) = directories() else {
        return;
    };
    let folder = state.path().join("工程");
    std::fs::create_dir_all(folder.join("子目录").join("深")).unwrap();
    std::fs::create_dir(folder.join("空")).unwrap();
    std::fs::write(folder.join("说明.txt"), "顶层").unwrap();
    std::fs::write(folder.join("子目录").join("a.txt"), "一层").unwrap();
    std::fs::write(folder.join("子目录").join("深").join("b.txt"), "两层").unwrap();
    let session = Session::open(served.path(), state.path());

    session.upload(&folder);
    session.wait_phase(TransferPhase::Completed, |question| {
        panic!("unexpected question: {question:?}")
    });
    let uploaded = served.path().join("工程");
    let read = |path: &Path| std::fs::read_to_string(uploaded.join(path)).unwrap();
    assert_eq!(read(Path::new("说明.txt")), "顶层");
    assert_eq!(read(&Path::new("子目录").join("a.txt")), "一层");
    assert_eq!(read(&Path::new("子目录").join("深").join("b.txt")), "两层");
    assert!(uploaded.join("空").is_dir());
    let names = session.list(&session.home.join("工程").unwrap()).unwrap();
    assert!(names.contains(&"子目录".to_string()), "{names:?}");
}
