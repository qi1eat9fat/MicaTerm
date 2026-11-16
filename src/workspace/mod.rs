//! 窗口壳：无边框窗口里的标题栏、主机栏、标签区、右侧栏，以及**全部 action 的处理器**。
//!
//! - `frame`：把窗口壳的 action 处理器挂到 `Root` 外层的插件，对话框里派发的 action 也到得了；
//! - `view`：`Workspace` 实体、注入式构造（`new_with_services`）和 render；
//! - `handlers/`：处理器按领域分文件（`window`、`tabs`、`cards`、`terminal`、`settings`），
//!   每个文件提供 `bind_<领域>_actions`，render 里只挂这几个函数，各工单只改自己那个文件；
//! - `title_bar`、`window_buttons`、`host_sidebar`、`right_sidebar`、`new_tab_page`：各块界面，
//!   都是无状态的渲染函数，按钮一律经窗口壳的焦点派发 action；
//! - `notices`：终端的通知和响铃送到哪里（系统通知或窗口里的提示）；
//! - `remote/`：远程卡片（工单 12）：按主机建卡片、开标签、原地重连，连接表事件的分发和问答队列，
//!   连接前取秘密的钩子（13 接保险库解锁）。
//! - `window_state`：记住窗口大小和位置（`window.json`），和下次启动窗口开在哪（工单 26）。
//!
//! 功能模块不依赖窗口壳；窗口壳持有它们、转发 action。

mod agent_state;
mod cli;
mod cli_cards;
mod cli_changes;
mod frame;
mod handlers;
mod host_sidebar;
mod new_tab_page;
mod notices;
mod remote;
mod right_sidebar;
mod title_bar;
mod view;
mod window_buttons;
mod window_state;

pub use right_sidebar::{FilesButton, RightPanel};
pub use view::{Workspace, WorkspaceServices, window_options};
pub use window_state::{WindowState, default_window_size, window_placement};

/// 窗口壳要的全局初始化（注册 `Root` 插件）。在 `gpui_kit::init` 之后调用。
pub fn init(cx: &mut gpui_kit::App) {
    frame::init(cx);
}
