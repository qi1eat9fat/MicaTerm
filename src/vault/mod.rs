//! 保险库的界面（工单 13）：保险库（`micaterm_hosts::Vault`）的 GPUI 外壳 [`VaultStore`]、
//! 设置主密码框、解锁小卡片、重置确认，以及「用到秘密之前」的流程。
//!
//! - `store`：[`VaultStore`]，保险库的状态（未设 / 没设主密码 / 打不开 / 已锁定 / 已解锁），
//!   保险库操作一律放到后台执行器上跑；窗口壳建一个，装成全局量，对话框、设置页据此显示；
//! - `password`：设置主密码（第一次保存秘密时带「不设置」）、更换主密码；
//! - `unlock`：解锁小卡片（「在本机记住」默认勾选，主密码不对时描红）；
//! - `confirm`：重置保险库的确认、「在另一台电脑上创建」的确认；
//! - `field`：存在保险库里的一条秘密的输入框（[`SecretField`]），主机、代理、凭据的密码和口令共用；
//! - `flows`：写秘密之前（[`before_saving_secret`]）和连接用秘密之前（[`before_using_secrets`]）
//!   要弹哪个框；远程卡片等保险库（工单 33）用 [`asks_before_retrying`] 和 [`unlock_before_connecting`]，
//!   能知道用户是解开了还是取消了；
//! - `login`：一次登录要用到哪些秘密（无密码和 Agent 登录不用）。
//!
//! 功能模块不依赖窗口壳：按钮派发 `crate::app` 的保险库 action，由窗口壳交给这里。

mod confirm;
mod field;
mod flows;
mod login;
mod password;
mod reply;
mod store;
mod unlock;

pub use confirm::open_reset_dialog;
pub use field::{ReadSecret, SecretChange, SecretField, locked_away};
pub use flows::{asks_before_retrying, unlock_before_connecting};
pub use flows::{
    asks_before_saving, asks_before_using, before_saving_secret, before_using_secrets,
};
pub use login::login_secrets;
pub use password::open_master_password_dialog;
pub use store::{VaultStore, VaultStoreEvent};
pub use unlock::open_unlock_dialog;

// 保险库核心的类型，界面不必直接依赖 micaterm-hosts。
pub use micaterm_hosts::{Vault, VaultStatus};
