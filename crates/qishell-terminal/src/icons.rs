//! 终端右键菜单用到的、gpui-kit 默认图标包以外的 Lucide 图标。
//! 应用把 `TerminalIcons` 接进自己的 `AssetSource`，图标才画得出来。

use gpui_kit::assets::icon_assets;

icon_assets!(pub TerminalIcons, [ClipboardPaste, SquareDashedText, Eraser]);

/// 图标名（Lucide 目录）。
pub(crate) use gpui_kit::assets::IconName as TerminalIcon;
