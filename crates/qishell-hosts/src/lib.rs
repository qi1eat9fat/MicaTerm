// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-hosts/src/lib.rs
// 裁剪：去掉同步、Telnet、串口、项目终端、端口转发、主机分组、片段分类、SFTP 书签；
// 主机平铺；数据库从 QiShell 自己的 schema v1 起步；私钥进保险库，没有 `keys/` 目录。

//! QiShell 的主机核心：保存的主机（平铺，只有 SSH）、主机共用的凭据、所有主机共用的
//! 命令片段；它们写穿到的 SQLite 数据库；以及秘密存储的接口 [`secrets::SecretStore`]
//! （由保险库实现，测试用 [`secrets::InMemorySecretStore`]）。
//!
//! [`HostData`] holds it all in memory, which is what counts, and writes
//! each change through to the database. It says nothing in words and
//! notifies no one: failures and changes queue up as typed [`HostEvent`]s,
//! and vault writes as [`SecretJob`]s, for the app to take, word in its
//! own language and run off the UI thread. The app wraps it in a GPUI
//! entity.
//!
//! 测试夹具：[`HostData::empty`]、[`HostData::seed`]、[`HostData::with_secrets`] 注入内存版秘密存储，
//! `testing` feature 打开的 [`HostDatabase::in_memory`]。
//!
//! 把主机和凭据拼成 SSH 登录：[`HostLoginExt::of`]、[`HostData::login`]；秘密存储交给连接用的
//! 只读一面是 [`secrets::StoreSource`]。`SecretRef`、`ProxyKind`、`HostLogin` 都是 qishell-ssh 的。

mod credential;
mod data;
mod database;
mod login;
mod mark;
mod model;
mod private_key;
mod search;
pub mod secrets;
mod snippet;

pub use credential::*;
pub use data::{Change, Failure, HostData, HostEvent, SecretError, SecretJob, SecretOutcome};
pub use database::{HostDatabase, OpenError, SCHEMA_VERSION, StoredData};
pub use gpui_shared_string::SharedString;
pub use login::{
    DEFAULT_KEY_FILES, HostLogin, HostLoginExt, JumpLogin, LoginMethod, LoginRoute, ProxyLogin,
    SavedKey,
};
pub use mark::mark_color;
pub use model::*;
pub use private_key::{
    GeneratedKey, KeyAlgorithm, PastedKey, PastedKeyError, read_public_key, read_small,
};
pub use secrets::{SecretRef, SecretStore, SharedSecretStore, StoreSource};
pub use snippet::*;
// 主机栏和快速连接共用的主机搜索（工单 11）。
pub use search::host_matches;
