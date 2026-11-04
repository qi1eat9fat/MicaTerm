//! QiShell 的 SSH 核心：登录（主机密钥、密码、私钥、Agent、跳板和代理）、
//! 一条连接上的通道（伪终端上的 shell、命令、SFTP），以及 SFTP 传输引擎。
//!
//! 它不显示也不说任何话：要问用户的事以带类型的提问发出，失败以 `anyhow`
//! 链里带类型的错误给出，由应用用自己的文案表述。秘密经 [`SecretSource`]
//! 读取，并且只在服务器要的时候才读，所以保险库锁着时用不到秘密的登录不受影响。
//!
//! 拷自 since2006/shell-rs @ b2a307a 的 `crates/shellrs-ssh`（MIT，原许可见 LICENSE）。
//! 裁剪：去掉 NAT64、`exec` 命令（外部 CLI 用）、远程端口转发、SFTP 的 CLI 同步、
//! 终端连接上另开的 SFTP 通道（单栏「文件」）和同步用的秘密。
//! 改造：私钥从内存里的私钥数据登录（经 `SecretSource` 读保险库），不再按文件路径读；
//! 系统探测多取 `uname -n`。

mod connection;
mod error;
mod latency;
mod login;
mod probe;
mod prompt;
mod proxy;
mod secret;
mod session;
pub mod sftp;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use connection::{
    AgentLocation, SshConnectionConfig, SshConnector, SshHandle, SshPrompts, Tunnel,
    is_network_error, timeout_excluding_prompts,
};
pub use error::{
    AgentProblem, AgentUnreachable, AuthError, ChannelError, ConnectError, ForwardProblem,
    KnownHostsError, MissingCredential, PasswordWanted, ProxyError, RouteFailure, WrapErr, Wrapped,
    find_cause, link_as, wrap, wrap_shared,
};
pub use latency::{LATENCY_INTERVAL, Latency, LatencyLevel, round_trip};
pub use login::{
    DEFAULT_KEY_FILES, HostLogin, JumpLogin, LoginMethod, LoginRoute, ProxyKind, ProxyLogin,
};
pub use probe::{HOST_OS_KEYS, HostOsProbe, PROBE_COMMAND, ProbeOutcome, WINDOWS_PROBE_COMMAND};
pub use prompt::{
    AuthenticationPrompt, AuthenticationRequest, ConnectionPrompt, ConnectionPromptField,
    ConnectionPromptKind, ConnectionPromptReply, ConnectionSecret, HostKeyChangedPrompt,
    UnknownHostPrompt,
};
pub use secret::{SecretRef, SecretSource, SharedSecretSource, read_secret};
pub use session::{
    ChannelStream, PtySize, open_probe, open_shell, resize_pty, run_exec, write_shell,
};
