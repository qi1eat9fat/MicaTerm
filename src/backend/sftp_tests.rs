//! SFTP 真协议的冒烟测试：`SshSftpTransportProvider` 对着进程内的 SSH 测试服务器登录，
//! 经 OpenSSH 的 `sftp-server`（Windows 上是 Git for Windows 带的）列目录、上传、读文件、原地写回。
//! 这台机器上没有 `sftp-server` 时跳过。路径当不透明的前缀用：Git 的 `sftp-server` 报的是 MSYS 风格。

use std::path::Path;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use qishell_ssh::testing::{TEST_PASSWORD, TEST_USER, TestServer, has_sftp_server};
use qishell_ssh::{ConnectionPromptKind, ConnectionPromptReply, ConnectionSecret, HostLogin};
use qishell_ssh::{LoginMethod, SshConnector};
use tempfile::TempDir;

use super::sftp::{
    RemotePath, SftpCommand, SftpEvent, SftpTransportProvider as _, SshSftpTransportProvider,
    TransferPhase, UploadRequest,
};
use super::tests::Secrets;

const WAIT: Duration = Duration::from_secs(15);

/// 一个登录好的 SFTP 工作线程。
struct Session {
    commands: Sender<SftpCommand>,
    events: Receiver<SftpEvent>,
    home: RemotePath,
    /// 服务器和运行时都活到测试结束。
    _server: TestServer,
    _runtime: tokio::runtime::Runtime,
}

impl Session {
    /// 登录。第一次连的主机密钥信任；问密码就回答。
    fn open(directory: &Path, state: &Path) -> Self {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let server = runtime
            .block_on(TestServer::start(directory.to_path_buf()))
            .expect("server");
        let connector = SshConnector::new(
            state.join("known_hosts"),
            std::sync::Arc::new(Secrets::default()),
        );
        let provider = SshSftpTransportProvider::new(
            connector,
            state.join("upload-resume"),
            state.join("download-resume"),
        );
        let login = HostLogin::new("127.0.0.1", server.port, TEST_USER, LoginMethod::Password);
        let transport = provider.create(&login);
        let (commands, receiver) = async_channel::unbounded();
        let (sender, events) = async_channel::unbounded();
        std::thread::spawn(move || transport.run(receiver, sender));
        let mut session = Self {
            commands,
            events,
            home: RemotePath::new("/").expect("path"),
            _server: server,
            _runtime: runtime,
        };
        session.home = session.wait_connected();
        session
    }

    fn wait_connected(&self) -> RemotePath {
        loop {
            match self.next() {
                SftpEvent::Connected { home } => return home,
                SftpEvent::Prompt(prompt) => {
                    let reply = match prompt.kind() {
                        ConnectionPromptKind::UnknownHost(_) => ConnectionPromptReply::TrustAndSave,
                        ConnectionPromptKind::Authentication(_) => ConnectionPromptReply::Answers(
                            vec![ConnectionSecret::new(TEST_PASSWORD.to_string())],
                        ),
                        other => panic!("unexpected question: {other:?}"),
                    };
                    self.send(SftpCommand::PromptReply {
                        request_id: prompt.request_id(),
                        reply,
                    });
                }
                SftpEvent::Disconnected(why) => panic!("couldn't connect: {why}"),
                _ => {}
            }
        }
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

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.commands.try_send(SftpCommand::Shutdown);
    }
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
