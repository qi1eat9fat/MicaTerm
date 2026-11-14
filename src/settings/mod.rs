//! 设置：设置模型（`model`）、`settings.json` 的读写（`store`）、「应用设置」（`apply`），
//! 窗口背景（`backdrop`：不透明或 Mica），
//! 和设置标签（`tab`：左边分类导航，页面在 `pages`、`sftp`、`security`、`app_page`，通用件在 `controls`）。
//!
//! 内存为准、写穿到磁盘：每次改动立刻写文件，写失败时内存照样改，
//! 并发出 `SettingsStoreEvent::PersistFailed`，由窗口壳弹通知，绝不静默吞掉。

mod app_page;
mod apply;
mod backdrop;
mod controls;
mod local_shell_editor;
mod model;
mod pages;
mod security;
mod sftp;
mod store;
mod tab;
mod terminal_themes;

pub use apply::{apply, apply_terminal, terminal_font};
pub use backdrop::{WindowBackdrop, mica_supported};
pub use model::{
    AppSettings, Appearance, DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, DEFAULT_TERMINAL_FONT_FAMILY,
    DEFAULT_TERMINAL_FONT_SIZE, DEFAULT_TERMINAL_LINE_HEIGHT, InterfaceSettings,
    LocalTerminalSettings, RightClick, SftpSettings, TERMINAL_FONT_SIZE_RANGE,
    TERMINAL_LINE_HEIGHT_RANGE, TerminalCursorStyle, TerminalFontSettings, TerminalInteraction,
    TerminalThemeSettings, WindowSettings,
};
pub use store::{SettingsProblem, SettingsStore, SettingsStoreEvent};
pub use tab::{SettingsPaths, SettingsTab};
pub use terminal_themes::theme_card_id;
// 分段按钮和标签轨道同一种样式，主机对话框也用它（工单 11）。
pub use controls::{OnChoose, segmented};
