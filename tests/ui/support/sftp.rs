//! SFTP 标签的夹具：假的 SFTP 传输、假的本地目录，和打开在它们上面的窗口壳。
// 来自 since2006/shell-rs @ b2a307a: tests/ui/support/sftp.rs
// 裁剪：去掉 `SftpRoute`、`on_terminal`、`places`；改成 QiShell 的夹具（`..fake_services()`）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use qishell::backend::HostLogin;
use qishell::backend::sftp::{
    DirectoryEntry, DirectoryListing, DownloadRequest, EDIT_LIMIT, EntryKind, FileBytes,
    FileMetadata, FileStamp, LocalDirectoryProvider, PermissionEdit, ReadFailure, RemoteOperation,
    RemotePath, SaveFailure, SftpCommand, SftpEvent, SftpTransport, SftpTransportProvider,
    TextFile, TransferAnswer, TransferChoice, TransferDirection, TransferPhase, TransferProgress,
    UploadRequest,
};

/// 假服务器上给编辑器用的文件：内容和修改时间。
pub type FakeFiles = Arc<Mutex<std::collections::BTreeMap<String, (Vec<u8>, u32)>>>;
/// 编辑器的写入：路径、内容和文件当时该有的样子。
pub type FakeWrites = Arc<Mutex<Vec<(String, Vec<u8>, Option<FileStamp>)>>>;

#[derive(Default)]
pub struct FakeSftpProvider {
    /// 每次建传输用的登录：一个 SFTP 标签一条连接。
    pub logins: Arc<Mutex<Vec<HostLogin>>>,
    /// 工作线程收到的回答（`SftpCommand::PromptReply`）的请求号。
    pub prompt_replies: Arc<Mutex<Vec<u64>>>,
    /// 工作线程列过的目录，按顺序。
    pub listed: Arc<Mutex<Vec<String>>>,
    pub requests: Arc<Mutex<Vec<UploadRequest>>>,
    pub downloads: Arc<Mutex<Vec<DownloadRequest>>>,
    pub operations: Arc<Mutex<Vec<RemoteOperation>>>,
    /// 每个工作线程的事件通道：测试替工作线程说话。
    pub events: Arc<Mutex<Vec<async_channel::Sender<SftpEvent>>>>,
    /// 设了的话，下一条连接等收到消息才算连上。
    pub hold_connection: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    /// 「重新连接」又连了几次。
    pub reconnects: Arc<Mutex<usize>>,
    /// 远程主目录是 `/slow`，永远不回答：网络慢得第一个目录都到不了。
    pub slow_home: bool,
    /// 编辑器读写的远程文件。读 `/slow` 下面的路径永远不回答。
    pub files: FakeFiles,
    /// 编辑器的读取，按路径。
    pub reads: Arc<Mutex<Vec<String>>>,
    pub writes: FakeWrites,
    /// 传输问题的回答：请求号和回答。
    pub answers: Arc<Mutex<Vec<(u64, TransferAnswer)>>>,
}

// 工单 21–24 会用到其余的方法。
#[allow(dead_code)]
impl FakeSftpProvider {
    /// 服务器上有这些文件。
    pub fn with_files(files: &[(&str, &[u8])]) -> Self {
        let provider = Self::default();
        provider.files.lock().unwrap().extend(
            files
                .iter()
                .map(|(path, bytes)| (path.to_string(), (bytes.to_vec(), 100))),
        );
        provider
    }

    /// 别人改了服务器上的文件。
    pub fn change_file(&self, path: &str, bytes: &[u8]) {
        let mut files = self.files.lock().unwrap();
        let modified = files.get(path).map_or(100, |(_, modified)| modified + 7);
        files.insert(path.into(), (bytes.to_vec(), modified));
    }

    pub fn file(&self, path: &str) -> Vec<u8> {
        self.files.lock().unwrap()[path].0.clone()
    }

    /// 替最新的那条连接的工作线程发一个事件。
    pub fn emit(&self, event: SftpEvent) {
        let events = self.events.lock().unwrap();
        events
            .last()
            .expect("a connection")
            .try_send(event)
            .expect("the tab listens");
    }

    /// 建了几条连接。
    pub fn connections(&self) -> usize {
        self.logins.lock().unwrap().len()
    }
}

/// 假的远程文件的样子。
pub fn fake_stamp(bytes: &[u8], modified: u32) -> FileStamp {
    FileStamp::new(
        bytes.len() as u64,
        Some(std::time::UNIX_EPOCH + Duration::from_secs(u64::from(modified))),
    )
}

impl SftpTransportProvider for FakeSftpProvider {
    fn create(&self, login: &HostLogin) -> Box<dyn SftpTransport> {
        self.logins.lock().unwrap().push(login.clone());
        Box::new(FakeSftpTransport {
            prompt_replies: self.prompt_replies.clone(),
            listed: self.listed.clone(),
            requests: self.requests.clone(),
            downloads: self.downloads.clone(),
            operations: self.operations.clone(),
            events: self.events.clone(),
            hold: self.hold_connection.lock().unwrap().take(),
            reconnects: self.reconnects.clone(),
            files: self.files.clone(),
            reads: self.reads.clone(),
            writes: self.writes.clone(),
            answers: self.answers.clone(),
            home: if self.slow_home {
                "/slow"
            } else {
                "/home/tester"
            },
        })
    }
}

pub struct FakeSftpTransport {
    pub prompt_replies: Arc<Mutex<Vec<u64>>>,
    pub listed: Arc<Mutex<Vec<String>>>,
    pub requests: Arc<Mutex<Vec<UploadRequest>>>,
    pub downloads: Arc<Mutex<Vec<DownloadRequest>>>,
    pub operations: Arc<Mutex<Vec<RemoteOperation>>>,
    pub events: Arc<Mutex<Vec<async_channel::Sender<SftpEvent>>>>,
    pub hold: Option<mpsc::Receiver<()>>,
    pub reconnects: Arc<Mutex<usize>>,
    pub files: FakeFiles,
    pub reads: Arc<Mutex<Vec<String>>>,
    pub writes: FakeWrites,
    pub answers: Arc<Mutex<Vec<(u64, TransferAnswer)>>>,
    pub home: &'static str,
}

impl SftpTransport for FakeSftpTransport {
    fn run(
        self: Box<Self>,
        commands: async_channel::Receiver<SftpCommand>,
        events: async_channel::Sender<SftpEvent>,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push(events.clone());
        if let Some(hold) = &self.hold {
            let _ = hold.recv();
        }
        let connect = |home: &str| -> anyhow::Result<()> {
            events.send_blocking(SftpEvent::Connected {
                home: RemotePath::new(home)?,
            })?;
            Ok(())
        };
        connect(self.home)?;
        // 和真的引擎一样：「继续」接着跑停下的一批，没有的话只重连。
        let mut stopped = false;
        while let Ok(command) = commands.recv_blocking() {
            match command {
                SftpCommand::List { request_id, path } => {
                    self.listed.lock().unwrap().push(path.to_string());
                    if path.as_str() == "/slow" {
                        continue;
                    }
                    let result = match path.as_str() {
                        "/denied" => Err("无法读取目录：权限不足".into()),
                        "/missing" => Err("无法读取目录：没有这个目录".into()),
                        "/empty" => Ok(DirectoryListing::new("/empty", vec![])),
                        "/dotfiles" => Ok(dotfiles_listing()),
                        "/pictures" => Ok(files_listing(&self.files, "/pictures")),
                        _ => Ok(fake_listing(path.as_str())),
                    };
                    events.send_blocking(SftpEvent::Listed { request_id, result })?;
                }
                SftpCommand::Operate {
                    request_id,
                    operation,
                } => {
                    let refused = format!("{operation:?}").contains("denied");
                    // 名字里有这个的操作永远不回答：测试等答复的那段时间（工单 21）。
                    if format!("{operation:?}").contains("hold-answer") {
                        self.operations.lock().unwrap().push(operation);
                        continue;
                    }
                    if let RemoteOperation::CreateFile { path } = &operation {
                        self.files
                            .lock()
                            .unwrap()
                            .insert(path.to_string(), (Vec::new(), 100));
                    }
                    self.operations.lock().unwrap().push(operation);
                    events.send_blocking(SftpEvent::Operated {
                        request_id,
                        result: if refused {
                            Err("权限不足".into())
                        } else {
                            Ok(())
                        },
                    })?;
                }
                SftpCommand::ReadFile { request_id, path } => {
                    self.reads.lock().unwrap().push(path.to_string());
                    if path.as_str().starts_with("/slow") {
                        continue;
                    }
                    let result = match self.files.lock().unwrap().get(path.as_str()) {
                        Some((bytes, _)) if bytes.len() as u64 > EDIT_LIMIT => {
                            Err(ReadFailure::TooLarge(bytes.len() as u64))
                        }
                        Some((bytes, modified)) => {
                            TextFile::decode(bytes.clone(), fake_stamp(bytes, *modified))
                        }
                        None => Err(ReadFailure::Failed("文件不存在".into())),
                    };
                    events.send_blocking(SftpEvent::FileRead { request_id, result })?;
                }
                SftpCommand::ReadBytes {
                    request_id,
                    path,
                    limit,
                } => {
                    self.reads.lock().unwrap().push(path.to_string());
                    if path.as_str().starts_with("/slow") {
                        continue;
                    }
                    let result = match self.files.lock().unwrap().get(path.as_str()) {
                        Some((bytes, _)) if bytes.len() as u64 > limit => {
                            Err(ReadFailure::TooLarge(bytes.len() as u64))
                        }
                        Some((bytes, _)) => Ok(FileBytes::new(bytes.clone())),
                        None => Err(ReadFailure::Failed("文件不存在".into())),
                    };
                    events.send_blocking(SftpEvent::BytesRead { request_id, result })?;
                }
                SftpCommand::WriteFile {
                    request_id,
                    path,
                    bytes,
                    expected,
                } => {
                    self.writes
                        .lock()
                        .unwrap()
                        .push((path.to_string(), bytes.clone(), expected));
                    let mut files = self.files.lock().unwrap();
                    let now = files.get(path.as_str()).map(|(b, m)| fake_stamp(b, *m));
                    let result = if expected.is_some() && expected != now {
                        Err(SaveFailure::Changed)
                    } else {
                        // 每次保存晚一秒。
                        let modified = files.get(path.as_str()).map_or(100, |(_, m)| m + 1);
                        let stamp = fake_stamp(&bytes, modified);
                        files.insert(path.to_string(), (bytes, modified));
                        Ok(stamp)
                    };
                    drop(files);
                    events.send_blocking(SftpEvent::FileWritten { request_id, result })?;
                }
                SftpCommand::Download(request) => {
                    self.downloads.lock().unwrap().push(request);
                    events.send_blocking(SftpEvent::Progress(
                        TransferProgress::new(TransferPhase::Transferring)
                            .with_direction(TransferDirection::Download),
                    ))?;
                }
                SftpCommand::Upload(request) => {
                    self.requests.lock().unwrap().push(request);
                    events.send_blocking(SftpEvent::Progress(Default::default()))?;
                }
                // 真的引擎停下或结束一批后说 Idle，队列这时才能往下走。
                SftpCommand::Cancel => {
                    stopped = true;
                    events.send_blocking(SftpEvent::Progress(TransferProgress::new(
                        TransferPhase::Stopped,
                    )))?;
                    events.send_blocking(SftpEvent::Idle)?;
                }
                SftpCommand::Discard => {
                    stopped = false;
                    events.send_blocking(SftpEvent::Progress(TransferProgress::new(
                        TransferPhase::Completed,
                    )))?;
                    events.send_blocking(SftpEvent::Idle)?;
                }
                SftpCommand::Resume if stopped => {
                    stopped = false;
                    events.send_blocking(SftpEvent::Progress(TransferProgress::new(
                        TransferPhase::Transferring,
                    )))?;
                }
                SftpCommand::Resume => {
                    *self.reconnects.lock().unwrap() += 1;
                    connect("/home/tester")?;
                    events.send_blocking(SftpEvent::Idle)?;
                }
                SftpCommand::Forget => {
                    stopped = false;
                    events.send_blocking(SftpEvent::Idle)?;
                }
                SftpCommand::Answer { request_id, answer } => {
                    self.answers.lock().unwrap().push((request_id, answer));
                    stopped = answer.choice() == TransferChoice::Cancel;
                    events.send_blocking(SftpEvent::Progress(TransferProgress::new(
                        if answer.choice() == TransferChoice::Cancel {
                            TransferPhase::Stopped
                        } else {
                            TransferPhase::Completed
                        },
                    )))?;
                    events.send_blocking(SftpEvent::Idle)?;
                }
                SftpCommand::PromptReply { request_id, .. } => {
                    self.prompt_replies.lock().unwrap().push(request_id);
                }
                SftpCommand::Shutdown => break,
                _ => {}
            }
        }
        Ok(())
    }
}

/// 测试的本地文件系统：每个目录都列 `fake_listing`，改动只记下来，不真做。
#[derive(Clone, Default)]
pub struct FakeLocalDirectory {
    pub calls: Arc<Mutex<Vec<String>>>,
    /// 编辑器读写的文件，在内存里。
    pub files: Arc<Mutex<std::collections::BTreeMap<PathBuf, Vec<u8>>>>,
    /// 列过的目录，按顺序。
    pub listed: Arc<Mutex<Vec<PathBuf>>>,
    /// 列这些目录时失败，说的是这个原因。
    pub unreadable: Arc<Mutex<Vec<PathBuf>>>,
}

impl FakeLocalDirectory {
    pub fn record(&self, call: String) -> anyhow::Result<()> {
        self.calls.lock().unwrap().push(call);
        Ok(())
    }
}

/// 本地主目录和「下载」文件夹。
pub const LOCAL_HOME: &str = "/local/tester";
pub const LOCAL_DOWNLOADS: &str = "/local/tester/Downloads";

impl LocalDirectoryProvider for FakeLocalDirectory {
    fn home(&self) -> PathBuf {
        LOCAL_HOME.into()
    }

    fn downloads(&self) -> PathBuf {
        LOCAL_DOWNLOADS.into()
    }

    fn list(&self, path: &Path) -> anyhow::Result<DirectoryListing> {
        self.listed.lock().unwrap().push(path.to_path_buf());
        if self
            .unreadable
            .lock()
            .unwrap()
            .iter()
            .any(|bad| bad == path)
        {
            anyhow::bail!("找不到这个目录");
        }
        if path.ends_with("dotfiles") {
            return Ok(dotfiles_listing());
        }
        Ok(fake_listing(path.to_str().unwrap()))
    }

    fn trash(&self, paths: &[PathBuf]) -> anyhow::Result<()> {
        self.record(format!("trash {paths:?}"))
    }

    fn rename(&self, from: &Path, to: &Path) -> anyhow::Result<()> {
        self.record(format!("rename {} -> {}", from.display(), to.display()))
    }

    fn create_dir(&self, path: &Path) -> anyhow::Result<()> {
        self.record(format!("mkdir {}", path.display()))
    }

    fn create_file(&self, path: &Path) -> anyhow::Result<()> {
        self.files.lock().unwrap().insert(path.into(), Vec::new());
        self.record(format!("touch {}", path.display()))
    }

    fn read_file(&self, path: &Path, limit: u64) -> anyhow::Result<(Vec<u8>, FileStamp)> {
        let bytes = self
            .files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("「{}」不存在", path.display()))?;
        if bytes.len() as u64 > limit {
            return Err(ReadFailure::TooLarge(bytes.len() as u64).into());
        }
        let stamp = FileStamp::new(bytes.len() as u64, None);
        Ok((bytes, stamp))
    }

    fn write_file(
        &self,
        path: &Path,
        bytes: &[u8],
        expected: Option<FileStamp>,
    ) -> anyhow::Result<FileStamp> {
        let mut files = self.files.lock().unwrap();
        let now = files
            .get(path)
            .map(|bytes| FileStamp::new(bytes.len() as u64, None));
        if expected.is_some() && expected != now {
            return Err(SaveFailure::Changed.into());
        }
        files.insert(path.into(), bytes.to_vec());
        drop(files);
        self.record(format!("write {}", path.display()))?;
        Ok(FileStamp::new(bytes.len() as u64, None))
    }

    fn set_permissions(
        &self,
        paths: &[PathBuf],
        edit: PermissionEdit,
        recursive: bool,
        add_x_to_dirs: bool,
    ) -> anyhow::Result<()> {
        self.record(format!(
            "chmod {paths:?} +{:o} -{:o} recursive={recursive} x={add_x_to_dirs}",
            edit.set(),
            edit.clear()
        ))
    }
}

/// 假服务器上一个目录现在的文件。
pub fn files_listing(files: &FakeFiles, path: &str) -> DirectoryListing {
    let prefix = format!("{path}/");
    let entries = files
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(file, (bytes, modified))| {
            let name = file.strip_prefix(&prefix)?;
            Some(DirectoryEntry::new(
                name,
                FileMetadata::new(
                    EntryKind::File,
                    bytes.len() as u64,
                    Some(*modified),
                    Some(0o644),
                ),
            ))
        })
        .collect();
    DirectoryListing::new(path, entries)
}

/// 大多是隐藏文件的目录：`.bashrc`、`.ssh` 和一个 `notes.txt`。
fn dotfiles_listing() -> DirectoryListing {
    DirectoryListing::new(
        "/dotfiles",
        vec![
            DirectoryEntry::new(
                ".bashrc",
                FileMetadata::new(EntryKind::File, 10, Some(100), Some(0o644)),
            ),
            DirectoryEntry::new(
                ".ssh",
                FileMetadata::new(EntryKind::Directory, 0, None, Some(0o700)),
            ),
            DirectoryEntry::new(
                "notes.txt",
                FileMetadata::new(EntryKind::File, 20, Some(200), Some(0o644)),
            ),
        ],
    )
}

/// 每个目录都列这几项：两个文件、一个目录、一个指向目录的链接。
pub fn fake_listing(path: &str) -> DirectoryListing {
    DirectoryListing::new(
        path,
        vec![
            DirectoryEntry::new(
                "文件 甲.txt",
                FileMetadata::new(EntryKind::File, 12, Some(100), Some(0o644)),
            ),
            DirectoryEntry::new(
                "文件 乙.txt",
                FileMetadata::new(EntryKind::File, 34, Some(200), Some(0o644)),
            ),
            DirectoryEntry::new(
                "目录",
                FileMetadata::new(EntryKind::Directory, 0, None, Some(0o755)),
            ),
            DirectoryEntry::new(
                "链接目录",
                FileMetadata::new(EntryKind::Symlink, 7, Some(300), Some(0o120_777)),
            )
            .with_target_kind(Some(EntryKind::Directory)),
        ]
        .into_iter()
        .map(|entry| entry.with_owner(Some("root".into()), Some("wheel".into())))
        .collect(),
    )
}
