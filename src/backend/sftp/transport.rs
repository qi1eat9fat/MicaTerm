// 来自 since2006/shell-rs @ b2a307a: src/sftp/transport.rs
// 裁剪：去掉 `SftpRoute`（单栏「文件」借终端连接的路线）：SFTP 标签总是自己登录一条连接。
//! 应用这一侧的 SFTP 工作线程：事件带上文案，每个标签一条线程、一个 `current_thread` 运行时。
//! 连接只属于这个标签，标签关了连接就断，所以不用连接表，也不用共享运行时。

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_channel::{Receiver, Sender};
use micaterm_ssh::{ConnectionPrompt, HostLogin, Latency, SshConnector};

use super::{
    DirectoryListing, EngineEvent, EventSink, FileBytes, FileStamp, ReadFailure, RemotePath,
    SaveFailure, SftpCommand, SftpWorker, TextFile, TransferProgress, TransferQuestion,
    describe_disconnect, describe_reason, describe_reason_link,
};

/// 工作线程说的话，失败都已经是文案（出事时的界面语言）。进度和问题保持原样，显示时再翻。
#[derive(Clone, Debug)]
pub enum SftpEvent {
    Connecting,
    Connected {
        home: RemotePath,
    },
    Disconnected(String),
    Prompt(ConnectionPrompt),
    Listed {
        request_id: u64,
        result: Result<DirectoryListing, String>,
    },
    Operated {
        request_id: u64,
        result: Result<(), String>,
    },
    FileRead {
        request_id: u64,
        result: Result<TextFile, ReadFailure>,
    },
    FileWritten {
        request_id: u64,
        result: Result<FileStamp, SaveFailure>,
    },
    BytesRead {
        request_id: u64,
        result: Result<FileBytes, ReadFailure>,
    },
    Progress(TransferProgress),
    Question(TransferQuestion),
    Notice(String),
    Idle,
    /// 连接最近一次的往返：连着时每隔几秒一次，连上之后马上一次，有没有传输都一样。
    Latency(Latency),
}

/// 一个工作线程的一生，调用方放在自己的线程上跑。
pub trait SftpTransport: Send + 'static {
    fn run(
        self: Box<Self>,
        commands: Receiver<SftpCommand>,
        events: Sender<SftpEvent>,
    ) -> Result<()>;
}

/// 按登录建工作线程。生产是 `SshSftpTransportProvider`，测试注入假的。
pub trait SftpTransportProvider: Send + Sync + 'static {
    fn create(&self, login: &HostLogin) -> Box<dyn SftpTransport>;
}

pub type SharedSftpTransportProvider = Arc<dyn SftpTransportProvider>;

/// 真的登录一条连接、只开 SFTP 子系统。
pub struct SshSftpTransportProvider {
    connector: SshConnector,
    /// 上传、下载的续传记录放在哪；`None` 是没有地方放，不记。
    resume_dirs: Option<(PathBuf, PathBuf)>,
}

impl SshSftpTransportProvider {
    /// 上传和下载的续传记录分开放在两个目录。
    pub fn new(connector: SshConnector, resume_dir: PathBuf, download_resume_dir: PathBuf) -> Self {
        Self {
            connector,
            resume_dirs: Some((resume_dir, download_resume_dir)),
        }
    }

    /// 不记续传记录：没有地方放的时候（取不到数据目录）。
    pub fn without_resume_records(connector: SshConnector) -> Self {
        Self {
            connector,
            resume_dirs: None,
        }
    }
}

impl SftpTransportProvider for SshSftpTransportProvider {
    fn create(&self, login: &HostLogin) -> Box<dyn SftpTransport> {
        let connector = self.connector.clone();
        Box::new(SshSftpTransport(match &self.resume_dirs {
            Some((upload, download)) => {
                SftpWorker::new(connector, login.clone(), upload.clone(), download.clone())
            }
            None => SftpWorker::without_resume_records(connector, login.clone()),
        }))
    }
}

struct SshSftpTransport(SftpWorker);

impl SftpTransport for SshSftpTransport {
    fn run(
        self: Box<Self>,
        commands: Receiver<SftpCommand>,
        events: Sender<SftpEvent>,
    ) -> Result<()> {
        // 在工作线程上、每个事件发生时翻成文案。
        let events = EventSink::new(move |event| events.try_send(app_event(event)).is_ok());
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(self.0.run(commands, events))
    }
}

/// 应用收到的 `event`。
fn app_event(event: EngineEvent) -> SftpEvent {
    match event {
        EngineEvent::Connecting => SftpEvent::Connecting,
        EngineEvent::Connected { home } => SftpEvent::Connected { home },
        EngineEvent::Disconnected(why) => SftpEvent::Disconnected(describe_disconnect(&why)),
        EngineEvent::Prompt(prompt) => SftpEvent::Prompt(prompt),
        EngineEvent::Listed { request_id, result } => SftpEvent::Listed {
            request_id,
            result: result.map_err(|why| describe_reason(&why)),
        },
        EngineEvent::Operated { request_id, result } => SftpEvent::Operated {
            request_id,
            result: result.map_err(|why| describe_reason(&why)),
        },
        EngineEvent::FileRead { request_id, result } => SftpEvent::FileRead {
            request_id,
            result: result.map_err(|failure| failure.map_failed(|why| describe_reason(&why))),
        },
        EngineEvent::FileWritten { request_id, result } => SftpEvent::FileWritten {
            request_id,
            result: result.map_err(|failure| failure.map_failed(|why| describe_reason(&why))),
        },
        EngineEvent::BytesRead { request_id, result } => SftpEvent::BytesRead {
            request_id,
            result: result.map_err(|failure| failure.map_failed(|why| describe_reason(&why))),
        },
        EngineEvent::Progress(progress) => SftpEvent::Progress(progress),
        EngineEvent::Question(question) => SftpEvent::Question(question),
        EngineEvent::Notice(why) => SftpEvent::Notice(describe_reason_link(&why)),
        EngineEvent::Idle => SftpEvent::Idle,
        EngineEvent::Latency(latency) => SftpEvent::Latency(latency),
    }
}
