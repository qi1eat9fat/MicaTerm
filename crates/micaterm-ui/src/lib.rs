//! MicaTerm 共用的界面组件：应用（`micaterm`）和终端（`micaterm-terminal`）都要用的放在这里，
//! 这样两边画出来是同一个东西（ui-lessons 第四节第 6 条「组件统一」）。
//!
//! - `menu`：唯一的右键菜单实现（26 号「终端右键」「主机右键」）。
//! - `find`：查找浮卡（26 号「查找条」），终端和编辑器共用。
//!
//! 组件不认识应用的颜色 token：应用在设置变化时把 [`MenuStyle`]、[`FindStyle`] 设成全局量
//! （`settings::apply`）。

mod find;
mod menu;

pub use find::{FindButton, FindCard, FindStyle, FindTextButton, ReplaceRow};
pub use menu::{Menu, MenuStyle};
