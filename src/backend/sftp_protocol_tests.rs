// 来自 since2006/shell-rs @ b2a307a: src/sftp/protocol_tests.rs
// 裁剪：去掉 `SftpRoute`（「文件」开在终端连接上）和 `sftp://` 外部连接的测试，QiShell 的 SFTP 标签总是自己一条连接；
// 服务器换成 `qishell_ssh::testing::TestServer`，故障注入也放在那里。
//! 工作线程走真协议的测试：只连本机，不碰用户的主机、凭据和 shell。
//! 这台机器上没有 `sftp-server` 时跳过（Windows 上用 Git 带的）。路径当不透明的前缀用。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use qishell_ssh::testing::{TEST_PASSWORD, TEST_USER, TestServer, has_sftp_server};
use qishell_ssh::{
    ConnectionPromptKind, ConnectionPromptReply, ConnectionSecret, HostLogin, Latency, LoginMethod,
    SshConnector,
};

use super::sftp::*;
use super::tests::Secrets;

struct Worker {
    commands: async_channel::Sender<SftpCommand>,
    events: async_channel::Receiver<SftpEvent>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.commands.try_send(SftpCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn worker(port: u16, data: &Path) -> Worker {
    let provider = SshSftpTransportProvider::new(
        SshConnector::new(data.join("known_hosts"), Arc::new(Secrets::default())),
        data.join("upload-resume"),
        data.join("download-resume"),
    );
    let login = HostLogin::new("127.0.0.1", port, TEST_USER, LoginMethod::Password);
    let transport = provider.create(&login);
    let (commands, receiver) = async_channel::unbounded();
    let (sender, events) = async_channel::unbounded();
    let thread = std::thread::spawn(move || transport.run(receiver, sender).unwrap());
    Worker {
        commands,
        events,
        thread: Some(thread),
    }
}

/// 下一个事件。连接的问题在这里回答：信任主机密钥，给密码。
async fn next(worker: &Worker) -> SftpEvent {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(25), worker.events.recv())
            .await
            .expect("worker timed out")
            .unwrap();
        let SftpEvent::Prompt(prompt) = event else {
            return event;
        };
        let reply = match prompt.kind() {
            ConnectionPromptKind::UnknownHost(_) => ConnectionPromptReply::TrustAndSave,
            ConnectionPromptKind::Authentication(_) => {
                ConnectionPromptReply::Answers(vec![ConnectionSecret::new(TEST_PASSWORD)])
            }
            _ => panic!("unexpected trust change"),
        };
        worker
            .commands
            .send(SftpCommand::PromptReply {
                request_id: prompt.request_id(),
                reply,
            })
            .await
            .unwrap();
    }
}

async fn connected(worker: &Worker) -> RemotePath {
    loop {
        match next(worker).await {
            SftpEvent::Connected { home } => return home,
            SftpEvent::Disconnected(error) => panic!("connect failed: {error}"),
            _ => {}
        }
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// 服务 `directory` 的服务器；这台机器没有 `sftp-server` 就是 `None`，测试跳过。
async fn server(directory: &Path) -> Option<TestServer> {
    if !has_sftp_server() {
        return None;
    }
    Some(TestServer::start(directory.into()).await.unwrap())
}

#[test]
fn worker_uses_sftp_without_shell_and_reconnects_after_interrupted_write() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        let Some(server) = server(&remote).await else {
            return;
        };
        server.interrupt_transfers(1);
        let worker = worker(server.port, temp.path());
        let home = connected(&worker).await;
        let source = temp.path().join("文件.bin");
        let data = vec![42; 300_007];
        std::fs::write(&source, &data).unwrap();
        worker
            .commands
            .send(SftpCommand::Upload(
                UploadRequest::new(vec![source], home).unwrap(),
            ))
            .await
            .unwrap();
        let mut reconnecting = false;
        loop {
            match next(&worker).await {
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Completed => {
                    assert_eq!(p.succeeded(), 1);
                    break;
                }
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Stopped => {
                    panic!("stopped unexpectedly: {p:?}")
                }
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Reconnecting => {
                    reconnecting = true
                }
                SftpEvent::Question(q) => panic!("unexpected question: {q:?}"),
                _ => {}
            }
        }
        assert!(reconnecting);
        assert_eq!(server.connections(), 2);
        assert_eq!(server.shells(), 0);
        assert_eq!(std::fs::read(remote.join("文件.bin")).unwrap(), data);
    });
}

/// 什么都没问时断开的连接，马上报告（和终端一样），不用等下一个请求；「继续」重新连。
#[test]
fn an_idle_connection_that_drops_is_reported_without_a_request() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let Some(server) = server(temp.path()).await else {
            return;
        };
        let worker = worker(server.port, temp.path());
        connected(&worker).await;
        server.hang_up();
        loop {
            match next(&worker).await {
                SftpEvent::Disconnected(reason) => {
                    assert_eq!(reason, "SFTP 连接中断，请重新连接");
                    break;
                }
                SftpEvent::Connected { .. } => panic!("still connected"),
                _ => {}
            }
        }
        worker.commands.send(SftpCommand::Resume).await.unwrap();
        connected(&worker).await;
        assert_eq!(server.connections(), 2);

        // 看着空闲连接不能让它在断开以后还开着。
        worker.commands.send(SftpCommand::Disconnect).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while server.ended() < 2 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the connection stayed open after 断开");
    });
}

/// 和终端一样，连上就报告往返时间，不用问它什么。
#[test]
fn a_connection_reports_its_round_trip_right_after_connecting() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let Some(server) = server(temp.path()).await else {
            return;
        };
        let worker = worker(server.port, temp.path());
        connected(&worker).await;
        let latency = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let SftpEvent::Latency(latency) = next(&worker).await {
                    break latency;
                }
            }
        })
        .await
        .expect("no round trip after connecting");
        assert!(matches!(latency, Latency::Measured(_)), "{latency:?}");
    });
}

#[test]
fn unsupported_sftp_does_not_launch_shell_or_loop_authentication() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let Some(server) = server(temp.path()).await else {
            return;
        };
        server.refuse_sftp();
        let worker = worker(server.port, temp.path());
        loop {
            match next(&worker).await {
                SftpEvent::Disconnected(_) => break,
                SftpEvent::Connected { .. } => panic!("unsupported subsystem connected"),
                _ => {}
            }
        }
        assert_eq!(server.connections(), 1);
        assert_eq!(server.shells(), 0);
    });
}

#[test]
fn reconnect_exhaustion_waits_for_manual_resume_and_resets_budget() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        let Some(server) = server(&remote).await else {
            return;
        };
        server.interrupt_transfers(4);
        let worker = worker(server.port, temp.path());
        let home = connected(&worker).await;
        let source = temp.path().join("file");
        std::fs::write(&source, vec![1; 200_000]).unwrap();
        worker
            .commands
            .send(SftpCommand::Upload(
                UploadRequest::new(vec![source], home).unwrap(),
            ))
            .await
            .unwrap();
        loop {
            match next(&worker).await {
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Stopped => break,
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Completed => {
                    panic!("exhaustion should stop")
                }
                SftpEvent::Question(q) => panic!("unexpected question {q:?}"),
                _ => {}
            }
        }
        assert_eq!(server.connections(), 4);
        // 手动再来一次有自己的三次重试，再中断一次。
        server.interrupt_transfers(1);
        worker.commands.send(SftpCommand::Resume).await.unwrap();
        loop {
            match next(&worker).await {
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Completed => break,
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Stopped => {
                    panic!("manual retry failed")
                }
                SftpEvent::Question(q) => panic!("unexpected question {q:?}"),
                _ => {}
            }
        }
        assert_eq!(server.connections(), 6);
        assert_eq!(
            std::fs::read(remote.join("file")).unwrap(),
            vec![1; 200_000]
        );
    });
}

#[test]
fn worker_downloads_and_resumes_after_an_interrupted_read() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        let data: Vec<u8> = (0..300_007).map(|i| (i % 241) as u8).collect();
        std::fs::write(remote.join("文件.bin"), &data).unwrap();
        let out = temp.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let Some(server) = server(&remote).await else {
            return;
        };
        server.interrupt_transfers(1);
        let worker = worker(server.port, temp.path());
        let home = connected(&worker).await;
        worker
            .commands
            .send(SftpCommand::Download(
                DownloadRequest::new(vec![home.join("文件.bin").unwrap()], out.clone()).unwrap(),
            ))
            .await
            .unwrap();
        let mut reconnecting = false;
        loop {
            match next(&worker).await {
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Completed => {
                    assert_eq!(p.succeeded(), 1);
                    assert_eq!(p.direction(), TransferDirection::Download);
                    break;
                }
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Stopped => {
                    panic!("stopped unexpectedly: {p:?}")
                }
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Reconnecting => {
                    reconnecting = true
                }
                SftpEvent::Question(q) => panic!("unexpected question: {q:?}"),
                _ => {}
            }
        }
        assert!(reconnecting);
        assert_eq!(server.connections(), 2);
        assert_eq!(std::fs::read(out.join("文件.bin")).unwrap(), data);
        assert!(!out.join("文件.bin.filepart").exists());
    });
}

/// 跑完一批，什么都不回答。
async fn transfer(worker: &Worker, command: SftpCommand) -> TransferProgress {
    worker.commands.send(command).await.unwrap();
    let mut last = None;
    loop {
        match next(worker).await {
            SftpEvent::Progress(progress) => last = Some(progress),
            SftpEvent::Question(question) => panic!("unexpected question: {question:?}"),
            SftpEvent::Idle => return last.expect("no progress before idle"),
            _ => {}
        }
    }
}

#[test]
fn scp_style_transfers_land_where_scp_would_put_them() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote");
        std::fs::create_dir_all(remote.join("sub")).unwrap();
        let Some(server) = server(&remote).await else {
            return;
        };
        let worker = worker(server.port, temp.path());
        let home = connected(&worker).await;
        let source = temp.path().join("local.txt");
        std::fs::write(&source, b"payload").unwrap();

        // 还没有：目标就是副本的名字。
        let request = UploadRequest::scp(source.clone(), home.join("renamed.txt").unwrap());
        let done = transfer(&worker, SftpCommand::Upload(request)).await;
        assert_eq!(done.phase(), TransferPhase::Completed);
        assert_eq!(
            std::fs::read(remote.join("renamed.txt")).unwrap(),
            b"payload"
        );

        // 已有的目录：副本放进去，用自己的名字。
        let request = UploadRequest::scp(source, home.join("sub").unwrap());
        let done = transfer(&worker, SftpCommand::Upload(request)).await;
        assert_eq!(done.phase(), TransferPhase::Completed);
        assert_eq!(
            std::fs::read(remote.join("sub").join("local.txt")).unwrap(),
            b"payload"
        );

        // 再下载回来，用新的本地名字。
        let back = temp.path().join("back.txt");
        let request =
            DownloadRequest::scp(home.join("renamed.txt").unwrap(), back.clone()).unwrap();
        let done = transfer(&worker, SftpCommand::Download(request)).await;
        assert_eq!(done.phase(), TransferPhase::Completed);
        assert_eq!(std::fs::read(&back).unwrap(), b"payload");

        // 上级目录不存在是错误，不猜。
        let request = UploadRequest::scp(
            temp.path().join("back.txt"),
            home.join("missing").unwrap().join("x.txt").unwrap(),
        );
        let done = transfer(&worker, SftpCommand::Upload(request)).await;
        assert_ne!(done.phase(), TransferPhase::Completed);
        assert!(!remote.join("missing").exists());
    });
}

/// 堡垒机拒绝 `fsetstat` 和扩展时：上传、覆盖上传、编辑器保存都在一条连接上做完，
/// 上传还保留本地文件的修改时间。
#[test]
fn uploads_and_saves_get_past_a_bastion_host_that_refuses_fsetstat_and_extensions() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        let Some(server) = server(&remote).await else {
            return;
        };
        server.act_as_bastion();
        let worker = worker(server.port, temp.path());
        let home = connected(&worker).await;
        let source = temp.path().join("README.md");
        let modified = std::time::UNIX_EPOCH + Duration::from_secs(1_500_000_000);
        let write_source = |bytes: &[u8]| {
            std::fs::write(&source, bytes).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&source)
                .unwrap()
                .set_modified(modified)
                .unwrap();
        };
        let upload =
            || SftpCommand::Upload(UploadRequest::new(vec![source.clone()], home.clone()).unwrap());
        let uploaded = remote.join("README.md");

        write_source(b"first");
        let done = transfer(&worker, upload()).await;
        assert_eq!(done.phase(), TransferPhase::Completed);
        assert_eq!(std::fs::read(&uploaded).unwrap(), b"first");
        assert_eq!(
            std::fs::metadata(&uploaded).unwrap().modified().unwrap(),
            modified
        );

        write_source(b"second");
        worker.commands.send(upload()).await.unwrap();
        loop {
            match next(&worker).await {
                SftpEvent::Question(question) => {
                    assert_eq!(question.kind(), TransferQuestionKind::Conflict);
                    worker
                        .commands
                        .send(SftpCommand::Answer {
                            request_id: question.id(),
                            answer: TransferAnswer::new(TransferChoice::Overwrite, false),
                        })
                        .await
                        .unwrap();
                }
                SftpEvent::Progress(p) if p.phase() == TransferPhase::Completed => {
                    assert_eq!(p.succeeded(), 1);
                    break;
                }
                SftpEvent::Progress(p)
                    if matches!(
                        p.phase(),
                        TransferPhase::Stopped | TransferPhase::Reconnecting
                    ) =>
                {
                    panic!("the upload over the file broke off: {p:?}")
                }
                _ => {}
            }
        }
        assert_eq!(std::fs::read(&uploaded).unwrap(), b"second");
        assert_eq!(
            std::fs::metadata(&uploaded).unwrap().modified().unwrap(),
            modified
        );

        worker
            .commands
            .send(SftpCommand::WriteFile {
                request_id: 7,
                path: home.join("README.md").unwrap(),
                bytes: b"edited".to_vec(),
                expected: None,
            })
            .await
            .unwrap();
        loop {
            match next(&worker).await {
                SftpEvent::FileWritten { request_id, result } => {
                    assert_eq!(request_id, 7);
                    result.unwrap();
                    break;
                }
                SftpEvent::Disconnected(e) => panic!("the save broke off: {e}"),
                _ => {}
            }
        }
        assert_eq!(std::fs::read(&uploaded).unwrap(), b"edited");
        assert_eq!(server.connections(), 1);
    });
}

/// 做一个文件操作，等它回答。
async fn operate(worker: &Worker, operation: RemoteOperation) -> Result<(), String> {
    worker
        .commands
        .send(SftpCommand::Operate {
            request_id: 11,
            operation,
        })
        .await
        .unwrap();
    loop {
        if let SftpEvent::Operated { request_id, result } = next(worker).await {
            assert_eq!(request_id, 11);
            return result;
        }
    }
}

/// 文件操作走真协议：建、改名（不覆盖）、改权限、连内容一起删。
#[test]
fn file_operations_reach_the_disk_of_the_server() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        let Some(server) = server(&remote).await else {
            return;
        };
        let worker = worker(server.port, temp.path());
        let home = connected(&worker).await;
        let at = |name: &str| home.join(name).unwrap();

        operate(
            &worker,
            RemoteOperation::CreateDirectory {
                path: at("新目录")
            },
        )
        .await
        .unwrap();
        assert!(remote.join("新目录").is_dir());

        let file = at("新目录").join("空文件.txt").unwrap();
        operate(&worker, RemoteOperation::CreateFile { path: file.clone() })
            .await
            .unwrap();
        assert!(remote.join("新目录").join("空文件.txt").is_file());
        // 已有同名的不能再建。
        operate(&worker, RemoteOperation::CreateFile { path: file.clone() })
            .await
            .unwrap_err();

        let other = at("新目录").join("别的.txt").unwrap();
        std::fs::write(remote.join("新目录").join("别的.txt"), b"keep").unwrap();
        // 改名不覆盖已有的。
        operate(
            &worker,
            RemoteOperation::Rename {
                from: file.clone(),
                to: other.clone(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            std::fs::read(remote.join("新目录").join("别的.txt")).unwrap(),
            b"keep"
        );
        let renamed = at("新目录").join("改过.txt").unwrap();
        operate(
            &worker,
            RemoteOperation::Rename {
                from: file,
                to: renamed,
            },
        )
        .await
        .unwrap();
        assert!(remote.join("新目录").join("改过.txt").is_file());

        operate(
            &worker,
            RemoteOperation::SetPermissions {
                paths: vec![at("新目录")],
                edit: PermissionEdit::exact(0o640),
                recursive: true,
                add_x_to_dirs: true,
            },
        )
        .await
        .unwrap();
        // Git 带的 `sftp-server` 没有真的 Unix 权限，只有 Unix 上能核对。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = |path: std::path::PathBuf| {
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777
            };
            assert_eq!(mode(remote.join("新目录")), 0o750);
            assert_eq!(mode(remote.join("新目录").join("改过.txt")), 0o640);
        }

        operate(
            &worker,
            RemoteOperation::Delete {
                paths: vec![at("新目录")],
            },
        )
        .await
        .unwrap();
        assert!(!remote.join("新目录").exists());
    });
}
