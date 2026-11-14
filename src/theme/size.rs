//! 尺寸：照 Zed 的比例（`zed/crates/ui`、`workspace`、`settings_ui`），数值取 26 号原型。
//! 和字号有关的行高（侧栏行）不在这里，见 `TypeScale`。

use gpui_kit::{Pixels, px};

/// 标签栏高度，侧栏顶部那一行同高、和它对齐。
pub const TAB_BAR: Pixels = px(32.);
/// 标签宽度，固定；多了横向滚动。
pub const TAB_WIDTH: Pixels = px(176.);
/// 每个窗格底部的状态栏高度。
pub const STATUS_BAR: Pixels = px(28.);
/// 普通按钮、图标按钮的高度。
pub const BUTTON: Pixels = px(28.);
/// 右键菜单的行高、最小宽度。
pub const MENU_ROW: Pixels = px(28.);
pub const MENU_MIN_WIDTH: Pixels = px(200.);
/// 输入框高度。
pub const INPUT: Pixels = px(30.);
/// 圆角：按钮、输入框、行用小的；菜单、弹窗、浮卡用大的。
pub const RADIUS: Pixels = px(6.);
pub const RADIUS_LG: Pixels = px(8.);
/// 区块之间的分隔线粗细。
pub const LINE: Pixels = px(1.);
/// 侧栏宽度。
pub const SIDEBAR_WIDTH: Pixels = px(232.);
/// 标题栏一行的图标按钮（26 号 `.tabbar .ib`）：边长 24、图标 15。侧栏顶部的搜索、新建主机
/// 和标签栏右边的分屏、深浅切换、设置用同一套（用户定）。
pub const BAR_BUTTON: Pixels = px(24.);
pub const BAR_ICON: Pixels = px(15.);
/// 右侧图标栏的宽度（含左边线 1，内宽 32 = 窗口按钮宽，图标和关闭按钮的 × 对齐）、图标边长。
pub const RAIL_WIDTH: Pixels = px(33.);
pub const RAIL_ICON: Pixels = px(17.);
/// 普通图标边长。
pub const ICON: Pixels = px(16.);
/// 停靠面板（凭据、片段）宽度。
pub const DOCK_PANEL_WIDTH: Pixels = px(280.);
/// 弹窗（连接提示、主机密钥、解锁保险库）宽度、离窗口顶的距离。
pub const MODAL_WIDTH: Pixels = px(448.);
pub const MODAL_TOP: Pixels = px(72.);
/// 设置标签页左导航宽度。
pub const SETTINGS_NAV_WIDTH: Pixels = px(200.);
/// 新标签页的窄列宽度、列表行高。
pub const NEW_TAB_COLUMN: Pixels = px(320.);
pub const NEW_TAB_ROW: Pixels = px(32.);
/// 勾选框这类小方块：12 在 100/125/150/200% 下都是整像素。
pub const CHECKBOX: Pixels = px(12.);
/// 已连接、标签状态点的直径。
pub const DOT: Pixels = px(6.);
/// 标签前那一块（14 见方）里的系统首字：图标的一部分，不跟界面字号走（26 号 `.bdg`）。
pub const BADGE_TEXT: Pixels = px(9.);
