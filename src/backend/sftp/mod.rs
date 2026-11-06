// 来自 since2006/shell-rs @ b2a307a: src/sftp/mod.rs
//! SSH 核心（`qishell-ssh`）的 SFTP 引擎，和应用自己的东西：引擎说的话翻成文案（`describe`）、
//! 这台电脑的文件（`local`）、每个 SFTP 标签一条线程的传输，事件带文案（`transport`）。
//! 界面只认这里的类型（spec 第 10 节）。

mod describe;
mod local;
mod transport;

pub use describe::*;
pub use local::*;
pub use qishell_ssh::sftp::*;
pub use transport::{
    SftpEvent, SftpTransport, SftpTransportProvider, SharedSftpTransportProvider,
    SshSftpTransportProvider,
};

/// 编辑器为什么打不开一个文件，应用的说法。
pub type ReadFailure = qishell_ssh::sftp::ReadFailure<String>;
/// 编辑器的保存为什么没做或者没做完，应用的说法。
pub type SaveFailure = qishell_ssh::sftp::SaveFailure<String>;
