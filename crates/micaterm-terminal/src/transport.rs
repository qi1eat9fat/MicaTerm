// 来自 since2006/shell-rs @ b2a307a: src/terminal/transport.rs
// 裁剪：去掉串口、Telnet、远程主机的 provider，侧边执行（`Exec`）、SFTP 通道、Break、
// 写文件数据（ZMODEM）和保留内容（`Replay`）；连接问答、主机系统、延迟依赖 M2 的
// micaterm-ssh / micaterm-hosts，先去掉（M2 #12 接回）。

use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

use anyhow::Result;
use async_channel::Sender;

use crate::{LocalLaunch, LocalShell, LocalShellInfo, TerminalSize};

/// Commands accepted by every terminal byte-stream transport.
pub enum TerminalTransportCommand {
    Write(Vec<u8>),
    Resize(TerminalSize),
    // M2 #12 接回：`PromptReply`（连接问答的回答）。
    Shutdown,
}

impl std::fmt::Debug for TerminalTransportCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Write(bytes) => formatter.debug_tuple("Write").field(&bytes.len()).finish(),
            Self::Resize(size) => formatter.debug_tuple("Resize").field(size).finish(),
            Self::Shutdown => formatter.write_str("Shutdown"),
        }
    }
}

/// Events produced by a terminal byte-stream transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalTransportEvent {
    Started,
    Output(Vec<u8>),
    // M2 #12 接回：`Prompt`（连接问答）、`HostOsDetected`（主机系统）、`Latency`（延迟）。
    /// The folder a local terminal's shell is in now, sent when it changes;
    /// only a local transport of a system that tells sends it.
    WorkingDirectory(PathBuf),
    /// 本地 shell 的前台程序换了：程序名（`program_name` 起的），`None` 是 shell 自己在提示符下。
    /// 只有能看到进程的本地传输发它（工单 06）。
    ForegroundProgram(Option<String>),
    Exited {
        code: u32,
        signal: Option<String>,
    },
    Failed(String),
}

/// One running transport. Implementations may block because MicaTerm always
/// invokes this method on a dedicated worker thread.
pub trait TerminalTransport: Send + 'static {
    fn run(
        self: Box<Self>,
        initial_size: TerminalSize,
        commands: mpsc::Receiver<TerminalTransportCommand>,
        events: Sender<TerminalTransportEvent>,
    ) -> Result<()>;
}

/// Creates a fresh one-shot transport for initial launch and restart.
pub trait TerminalTransportFactory: Send + Sync + 'static {
    fn create(&self) -> Box<dyn TerminalTransport>;
}

pub type SharedTerminalTransportFactory = Arc<dyn TerminalTransportFactory>;

/// Creates the factory of a local terminal for the shell the settings name
/// and the folder it starts in; asked when it opens and when it restarts, so
/// a changed setting reaches the next one. The workspace owns this boundary
/// and UI tests inject a fake.
pub trait LocalTerminalTransportProvider: Send + Sync + 'static {
    fn factory_for(&self, launch: &LocalLaunch) -> SharedTerminalTransportFactory;

    /// 卡片头部和状态栏显示的 shell 名称和版本。阻塞：在后台线程调用。
    /// 默认去本机查（`LocalShellInfo::detect`），测试的假实现给固定的。
    fn shell_info(&self, shell: &LocalShell) -> LocalShellInfo {
        LocalShellInfo::detect(shell)
    }
}

pub type SharedLocalTerminalTransportProvider = Arc<dyn LocalTerminalTransportProvider>;

/// For tests: every local terminal gets the same factory, and the launches
/// asked for are kept. Every shell is described as the same `info`.
pub struct FixedLocalTerminalTransportProvider {
    factory: SharedTerminalTransportFactory,
    launches: Mutex<Vec<LocalLaunch>>,
    info: LocalShellInfo,
}

impl FixedLocalTerminalTransportProvider {
    pub fn new(factory: SharedTerminalTransportFactory) -> Self {
        Self {
            factory,
            launches: Default::default(),
            info: LocalShellInfo::new("pwsh", Some("PowerShell 7.5.2")),
        }
    }

    /// 每个 shell 都说成 `info`。
    pub fn with_shell_info(mut self, info: LocalShellInfo) -> Self {
        self.info = info;
        self
    }

    /// How each local terminal started, in order.
    pub fn launches(&self) -> Vec<LocalLaunch> {
        self.launches
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// The shell of each local terminal started, in order.
    pub fn shells(&self) -> Vec<LocalShell> {
        self.launches()
            .into_iter()
            .map(|launch| launch.shell)
            .collect()
    }
}

impl LocalTerminalTransportProvider for FixedLocalTerminalTransportProvider {
    fn factory_for(&self, launch: &LocalLaunch) -> SharedTerminalTransportFactory {
        self.launches
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(launch.clone());
        self.factory.clone()
    }

    fn shell_info(&self, _: &LocalShell) -> LocalShellInfo {
        self.info.clone()
    }
}

pub(crate) fn send_event(
    events: &Sender<TerminalTransportEvent>,
    event: TerminalTransportEvent,
) -> bool {
    events.send_blocking(event).is_ok()
}
