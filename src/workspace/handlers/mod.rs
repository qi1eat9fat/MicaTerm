//! 全部 action 的处理器，按领域分文件。
//!
//! 每个文件写 `impl Workspace { fn on_xxx(..) }`，再提供一个
//! `bind_<领域>_actions(root, cx) -> root`，把本领域的处理器挂到窗口壳根节点上。
//! `Workspace::render` 里只按顺序 `.map(bind_..)`，新工单只改自己领域的那个文件：
//! - `window`：主机栏、右侧栏、快速连接（01、14、18、19）；
//! - `tabs`：标签（02、04）；
//! - `cards`：卡片分屏（05）；
//! - `terminal`：转发给焦点卡片里的终端（03）；
//! - `settings`：设置对话框（07）；
//! - `hosts`：主机栏、主机对话框（11）；在新标签里连接主机（12），分屏打开由 16 加在这里；
//! - `remote`：远程卡片的重新连接、断开连接、复制 IP（12）；
//! - `snippets`：命令片段面板（19）；
//! - `quick_connect`：快速连接弹层的开关（18）；
//! - `vault`：保险库的锁定、解锁、主密码、重置（13）。
//!
//! 处理器里改了状态要 `cx.notify()`；在焦点监听器里改状态要 `defer_in`。

mod cards;
mod hosts;
mod quick_connect;
mod remote;
mod settings;
mod snippets;
mod tabs;
mod terminal;
mod vault;
mod window;

pub(super) use cards::bind_cards_actions;
pub(super) use hosts::bind_hosts_actions;
pub(super) use quick_connect::bind_quick_connect_actions;
pub(super) use remote::bind_remote_actions;
pub(super) use settings::bind_settings_actions;
pub(super) use snippets::bind_snippets_actions;
pub(super) use tabs::bind_tabs_actions;
pub(super) use terminal::bind_terminal_actions;
pub(super) use vault::bind_vault_actions;
pub(super) use window::bind_window_actions;
