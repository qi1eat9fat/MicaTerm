//! 窗口壳：无边框窗口里的标题栏、主机栏、标签区、右侧栏，以及**全部 action 的处理器**。
//!
//! - `view`：`Workspace` 实体、注入式构造（`new_with_services`）和 render；
//! - `handlers/`：处理器按领域分文件（`window`、`tabs`、`cards`、`terminal`、`settings`），
//!   每个文件提供 `bind_<领域>_actions`，render 里只挂这几个函数，各工单只改自己那个文件；
//! - `title_bar`、`window_buttons`、`host_sidebar`、`right_sidebar`、`start_card`：各块界面，
//!   都是无状态的渲染函数，按钮一律经窗口壳的焦点派发 action；
//! - `notices`：终端的通知和响铃送到哪里（系统通知或窗口里的提示）；
//! - `remote/`：远程卡片（工单 12）：按主机建卡片、开标签、原地重连，连接表事件的分发和问答队列，
//!   连接前取秘密的钩子（13 接保险库解锁）。
//! - `quick_connect`：快速连接弹层的开关、位置和焦点（工单 18）。
//!
//! 功能模块不依赖窗口壳；窗口壳持有它们、转发 action。

mod handlers;
mod host_sidebar;
mod icon_button;
mod notices;
mod quick_connect;
mod remote;
mod right_sidebar;
mod start_card;
mod title_bar;
mod view;
mod window_buttons;

pub use right_sidebar::RightPanel;
pub use view::{Workspace, WorkspaceServices, window_options};
