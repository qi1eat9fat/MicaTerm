//! Rex 风格的视觉常量：颜色 token（`tokens`）和尺寸。
//!
//! 尺寸取 gorex 的数值（spec 第 3 节「视觉」、Rex 视觉拆解），颜色取全部界面原型。

mod tokens;

pub use tokens::Tokens;

use gpui_kit::{Pixels, px};

/// 卡片之间、卡片和窗口边之间的间隙。
pub const GAP: Pixels = px(8.);
/// 标题栏（主区域顶部的标签轨道那一行、主机栏顶部那一行）的高度。
pub const TITLE_BAR_HEIGHT: Pixels = px(44.);
/// 卡片圆角。
pub const CARD_RADIUS: Pixels = px(12.);
/// 卡片头部高度。
pub const CARD_HEADER_HEIGHT: Pixels = px(33.);
/// 卡片底部状态栏高度。
pub const STATUS_BAR_HEIGHT: Pixels = px(24.);
/// 标签高度；外面的轨道高 32。
pub const TAB_HEIGHT: Pixels = px(28.);
pub const TAB_TRACK_HEIGHT: Pixels = px(32.);
/// 标签宽度，固定。
pub const TAB_WIDTH: Pixels = px(180.);
/// 程序图块的宽和高。
pub const TILE_WIDTH: Pixels = px(21.);
pub const TILE_HEIGHT: Pixels = px(16.);
/// 主机栏宽度。
pub const HOST_SIDEBAR_WIDTH: Pixels = px(236.);
/// 右侧栏（图标列）宽度。
pub const RIGHT_SIDEBAR_WIDTH: Pixels = px(46.);
/// 右侧栏展开的面板宽度。
pub const RIGHT_PANEL_WIDTH: Pixels = px(290.);
/// 普通图标按钮的边长（`.ib`）和圆角。
pub const ICON_BUTTON: Pixels = px(26.);
pub const ICON_BUTTON_RADIUS: Pixels = px(10.);
/// 窗口按钮（`.ib.lg`）的边长和圆角。
pub const WINDOW_BUTTON: Pixels = px(30.);
pub const WINDOW_BUTTON_RADIUS: Pixels = px(11.);
/// 右侧栏图标按钮的边长。
pub const RAIL_BUTTON: Pixels = px(34.);
