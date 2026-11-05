//! 应用级的接线：action、快捷键、数据目录、图标与字体资源。

mod actions;
mod assets;
mod fonts;
mod paths;
pub mod shortcuts;

pub use actions::*;
pub use assets::{AppAssets, CatalogIcon};
pub use paths::{DATA_DIR_ENV, DATA_DIR_NAME, SETTINGS_FILE, data_dir, data_dir_of, settings_path};
pub use paths::{DATABASE_FILE, database_path};

use gpui_kit::App;

/// 产品名：窗口标题、系统通知、可执行文件的版本信息都用它。
pub const PRODUCT_NAME: &str = "QiShell";

/// 系统显示 QiShell 的通知和任务栏按钮时用的身份（Windows 的 AppUserModelID）。
pub const APP_IDENTITY: &str = "com.qishell.QiShell";

/// 初始化 gpui-kit 和应用的全局量。
pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    crate::i18n::init();
    fonts::register(cx);
    // 在 gpui-kit 自己的绑定之后：同一个键后绑的胜出。
    shortcuts::bind(cx);
    cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
}
