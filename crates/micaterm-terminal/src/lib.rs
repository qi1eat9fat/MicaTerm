//! MicaTerm 的终端：驱动 `alacritty_terminal` 的引擎（`TerminalEngine`）、视图（`TerminalView`）、
//! 查找、本地 PTY 和本地 shell 的检测。不依赖应用里的任何类型。
//!
//! 和应用之间的边界：
//! - 字节流从 `TerminalTransport` 来：本地终端由 `LocalTerminalTransportProvider` 按 `LocalLaunch`
//!   建工厂（生产用 `LocalPtyTransportProvider`，测试注入假的）；
//! - 字体、配色、交互走全局量 `TerminalFont`、`TerminalColors`、`TerminalInteraction`，由应用的设置写入；
//! - 动作（复制、粘贴、查找……）在 `actions` 里定义，命名空间 `micaterm_terminal`，由应用绑键、转发；
//!   `terminal_key_bindings` 是 Tab / Shift+Tab 在 `Terminal` 上下文里的绑定；
//! - 右键菜单是 `micaterm_ui::Menu`（和应用里的菜单是同一个组件），没有图标。
//!
//! 拷自 since2006/shell-rs @ b2a307a 的 `src/terminal/`（GPL-3.0），每个文件开头注明出处和裁剪。
//! 连接问答、主机系统、延迟这几样依赖 M2 的 micaterm-ssh / micaterm-hosts，先去掉，
//! 在原处留了 `M2 #12 接回` 的注释。

pub mod actions;
mod activity;
mod box_drawing;
mod directory;
mod engine;
mod environment;
mod font;
mod foreground;
mod i18n;
mod input;
mod links;
mod local_pty;
mod local_shell;
mod model;
mod mouse;
mod notices;
pub mod rendering;
mod scrollbar;
mod search;
mod shell_info;
mod shell_integration;
mod terminal_view;
#[cfg(test)]
mod testing;
mod theme;
mod transport;

pub use activity::{ShellActivity, ShellCommand, TitleSays, command_name, read_title, shell_words};
pub use directory::{HostMatch, ReportedDirectory};
pub use engine::{TerminalCell, TerminalEngine, TerminalEvent, TerminalSnapshot};
pub use font::{
    BUNDLED_FONT_FAMILY, DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT,
    FONT_SIZE_RANGE, LINE_HEIGHT_RANGE, TerminalFont, TerminalFontPreview, is_font_installed,
    monospace_font_families,
};
pub use foreground::program_name;
pub use input::{TerminalInput, TerminalInputEvent};
pub use local_pty::{LocalPtyTransportProvider, TERM_NAME, TERMINAL_ENCODING};
pub use local_shell::{LocalLaunch, LocalShell, default_shell_name, installed_shells, quote_word};
pub use model::{
    RightClick, TerminalCursorStyle, TerminalInteraction, TerminalLifecycle, TerminalSize,
    TerminalStatus,
};
pub use notices::TerminalNotice;
pub use search::{SearchDirection, SearchMark, SearchPosition};
pub use shell_info::LocalShellInfo;
pub use terminal_view::{
    CardItems, TERMINAL_FIND_KEY_CONTEXT, TERMINAL_KEY_CONTEXT, TerminalMenuItems, TerminalView,
    terminal_key_bindings,
};
pub use theme::{TerminalColors, TerminalTheme};
pub use transport::{
    FixedLocalTerminalTransportProvider, LocalTerminalTransportProvider,
    SharedLocalTerminalTransportProvider, SharedTerminalTransportFactory, TerminalTransport,
    TerminalTransportCommand, TerminalTransportEvent, TerminalTransportFactory,
};

// 终端自己的文案，来自 `locales/terminal.yml`：见 `i18n`。
rust_i18n::i18n!("locales", fallback = "zh-CN");
