// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-ssh/src/sftp/mod.rs
// 裁剪：去掉终端连接上的 SFTP 通道（`TerminalNotConnected`、`SftpChannelOpener`）；
// CLI 同步（`sync.rs`）在 Agent CLI 工单 03 加回。
//! The SFTP engine: one worker per session lists and changes remote files,
//! reads and saves them for an editor, and transfers them in batches,
//! several small files at once, resuming what a lost connection cut off.
//!
//! It sends only the requests WinSCP sends (no extensions, no `fsetstat`):
//! bastion hosts end the session on others. What happens comes out as
//! [`EngineEvent`]s through an [`EventSink`], with failures typed
//! ([`Reason`]); the app words them.
mod client;
mod control;
mod download;
mod edit;
mod engine;
mod error;
mod journal;
mod meter;
mod model;
mod operations;
mod parallel;
mod speed;
mod sync;
mod upload;
mod worker;
pub use client::is_network_error;
pub use control::Cancelled;
pub use edit::{EDIT_LIMIT, FileBytes, FileStamp, ReadFailure, SaveFailure, TextFile, TextFormat};
pub use engine::{Disconnect, EngineEvent, EventSink, EventsClosed, RemoteOperation, SftpCommand};
pub use error::*;
pub use model::*;
pub use parallel::{
    DEFAULT_PARALLEL_TRANSFERS, PARALLEL_TRANSFERS_RANGE, parallel_transfers,
    set_parallel_transfers,
};
pub use worker::SftpWorker;

#[cfg(test)]
mod tests;
