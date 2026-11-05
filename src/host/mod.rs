//! 主机：主机核心（`qishell-hosts`）的 GPUI 外壳和主机的界面。
//!
//! - `store`：`HostStore`，主机数据的 GPUI 实体（写穿数据库，写不进去时报告）；
//! - `describe`：主机数据说的话（写失败、表单错误）翻成文案；
//! - `mark`：系统徽章（主机栏、快速连接、开始卡片共用）；
//! - `list`：主机栏里的搜索框和平铺列表（选中、搜索、拖动排序、右键菜单）；
//! - `quick_connect`：快速连接弹层（最近连过的在前，输入就筛选，回车连接）；
//! - `menu`：主机右键菜单（连接类的项由开关控制，后面的工单接上处理器后打开）；
//! - `dialog/`：主机对话框（按分组拆文件）和删除确认。
//!
//! 功能模块不依赖窗口壳：这里只派发 `crate::app` 的 action，由窗口壳的处理器处理。

mod describe;
pub mod dialog;
mod list;
mod mark;
pub mod menu;
mod quick_connect;
mod store;

// 主机核心的类型都从这里取（命令片段、凭据……），应用的界面不必直接依赖 qishell-hosts。
pub use qishell_hosts::*;

pub use describe::ErrorMessage;
pub use dialog::{HostDialog, open_delete_host_dialog, open_host_dialog};
pub use list::{HostList, list_address};
pub use mark::HostMark;
pub use quick_connect::{QuickChoice, QuickConnectEvent, QuickConnectList, QuickHost, quick_hosts};
pub use store::{HostStore, HostStoreEvent};

use crate::i18n::t;

/// 生产环境的主机数据：数据目录里的 `qishell.db`。打不开时返回原因，由窗口壳报告，
/// 这次运行改用只在内存里的空数据。
pub fn load_hosts() -> Result<HostData, SharedString> {
    let path = crate::app::database_path()
        .map_err(|error| t!("host.store.open_failed", error = error.to_string()))?;
    let database = HostDatabase::open(&path)
        .map_err(|error| t!("host.store.open_failed", error = error.message()))?;
    HostData::load(database)
        .map_err(|error| t!("host.store.open_failed", error = error.to_string()))
}
