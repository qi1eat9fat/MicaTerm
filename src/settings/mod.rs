//! 设置：设置模型（`model`）与 `settings.json` 的读写（`store`）。
//!
//! 内存为准、写穿到磁盘：每次改动立刻写文件，写失败时内存照样改，
//! 并发出 `SettingsStoreEvent::PersistFailed`，由窗口壳弹通知，绝不静默吞掉。
//! 设置对话框（工单 07）和「应用设置」（把设置变成主题、终端字体等全局量）以后放在这个模块里。

mod model;
mod store;

pub use model::{
    AppSettings, Appearance, CursorStyle, DEFAULT_DARK_THEME, DEFAULT_INTERFACE_FONT_SIZE,
    DEFAULT_LIGHT_THEME, DEFAULT_TERMINAL_FONT_FAMILY, DEFAULT_TERMINAL_FONT_SIZE,
    DEFAULT_TERMINAL_LINE_HEIGHT, INTERFACE_FONT_SIZE_RANGE, InterfaceSettings,
    LocalTerminalSettings, RightClick, TERMINAL_FONT_SIZE_RANGE, TERMINAL_LINE_HEIGHT_RANGE,
    TerminalFontSettings, TerminalInteractionSettings, TerminalThemeSettings,
};
pub use store::{SettingsProblem, SettingsStore, SettingsStoreEvent};
