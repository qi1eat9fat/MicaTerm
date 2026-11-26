//! 应用级的接线：action、快捷键、数据目录、图标与字体资源。

mod actions;
mod assets;
mod fonts;
mod paths;
mod quit;
pub mod shortcuts;
#[cfg(windows)]
pub mod win32;

pub use actions::*;
pub use assets::{AppAssets, CatalogIcon, LOGO_IMAGE, MARK_ICON, MARK_THIN_ICON};
pub use paths::{CLI_BIN_DIR, PROGRAM_FILE, cli_endpoint};
pub use paths::{DATA_DIR_ENV, DATA_DIR_NAME, SETTINGS_FILE, data_dir, data_dir_of, settings_path};
pub use paths::{DATABASE_FILE, database_path};
pub use paths::{KNOWN_HOSTS_FILE, known_hosts_path};
pub use paths::{VAULT_FILE, vault_path};
pub use paths::{WINDOW_STATE_FILE, window_state_path};
pub use paths::{download_resume_dir, upload_resume_dir};
pub use quit::{quit_held_back, set_quit_guard};
pub use shortcuts::{EDITOR_CONTEXT, EDITOR_FIND_CONTEXT};
pub use shortcuts::{LOCAL_FILE_LIST_CONTEXT, REMOTE_FILE_LIST_CONTEXT};

use gpui_kit::App;

/// 产品名：窗口标题、系统通知、可执行文件的版本信息都用它。
pub const PRODUCT_NAME: &str = "MicaTerm";

/// 系统显示 MicaTerm 的通知和任务栏按钮时用的身份（Windows 的 AppUserModelID）。
pub const APP_IDENTITY: &str = "com.micaterm.MicaTerm";

/// 初始化 gpui-kit 和应用的全局量。
pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    // 窗口壳的 action 处理器包在 `Root` 最外层，在 gpui-kit 的插件之后注册。
    crate::workspace::init(cx);
    crate::i18n::init();
    fonts::register(cx);
    // 在 gpui-kit 自己的绑定之后：同一个键后绑的胜出。
    shortcuts::bind(cx);
    cx.on_action(|_: &Quit, cx: &mut App| quit::quit(cx));
}
