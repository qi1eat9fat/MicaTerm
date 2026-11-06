//! backend：包装 SSH 核心（`qishell-ssh`）的类型，并把错误链翻译成中文文案。界面只认这里的类型
//! （spec 第 10 节）。
//!
//! - `remote`：远程终端的边界（`RemoteTerminals` trait、`RemoteTarget`、`RemoteEvent`），
//!   生产实现 `SshRemoteTerminals`；测试注入假的；
//! - `pool`：按主机持有连接的连接表：一条连接上可以开多个 shell 通道，最后一个通道关了才断开；
//!   同一台主机正在连时后来的终端等同一个结果，不再登录一次；
//! - `transport`：远程终端的传输：在连接表借来的连接上开一个 shell 通道；
//! - `tester`：主机对话框的「测试连接」（`ConnectionTester`），真的登录一次再挂断；
//! - `describe`：连接的错误和问题翻成文案；
//! - `runtime`：共享的 tokio 运行时。russh 把连接的会话任务放在发起连接的运行时上，
//!   所以连接一律在这个不属于任何卡片的运行时上建，关掉一张卡片不会断掉同一连接上的其他通道；
//! - `sftp`：SFTP 引擎界面这一侧（工单 20）：事件带文案、每个标签一条线程的传输、本机目录。

#[cfg(test)]
mod credential_tests;
mod describe;
mod pool;
mod remote;
#[cfg(test)]
mod route_tests;
mod runtime;
pub mod sftp;
#[cfg(test)]
mod sftp_tests;
mod tester;
#[cfg(test)]
mod tests;
mod transport;

pub use describe::{
    authentication_fields, authentication_instructions, authentication_title, describe_chain,
    describe_error, describe_login_error, host_key_changed_lead, latency_label, unknown_host_lead,
};
pub use remote::{
    ConnectionId, PromptId, RemoteEvent, RemotePrompt, RemoteTarget, RemoteTerminalId,
    RemoteTerminals, SharedRemoteTerminals, SshRemoteTerminals,
};
pub use tester::{
    ConnectionTester, LoginTest, SharedConnectionTester, SshConnectionTester, TrustAsker,
};

// 界面要用到的 SSH 核心类型：问题和回答、延迟、探测结果、登录。
pub use qishell_ssh::{
    AuthenticationPrompt, AuthenticationRequest, ConnectionPrompt, ConnectionPromptField,
    ConnectionPromptKind, ConnectionPromptReply, ConnectionSecret, HostKeyChangedPrompt, HostLogin,
    Latency, ProbedHost, UnknownHostPrompt,
};
// 跳板和代理的路线（工单 15）。
pub use qishell_ssh::{JumpLogin, LoginRoute};
