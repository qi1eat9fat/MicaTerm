//! QiShell：以 SSH 主机管理为核心的桌面终端，Rex 风格的卡片分屏。
//!
//! 模块按能力划分，功能模块不依赖窗口壳（`workspace`），也不碰彼此的内部：
//! - `app`：全部 action（`app/actions/` 按领域分文件）、快捷键表、数据目录、图标与字体资源；
//! - `settings`：设置模型与 `settings.json` 的读写；
//! - `theme`：原型里 `.win.light` / `.win.dark` 两组颜色 token 和尺寸常量；
//! - `i18n`：文案翻译表（`locales/*.yml`，只有简体中文）；
//! - `card`：卡片（标签里的一格终端：头部、终端、状态栏），只认 `qishell_terminal` 的公开类型；
//! - `programs`：程序名表（命令名映射到程序图块，纯函数，本地和远程卡片共用）；
//! - `split`：终端标签里的卡片分屏树（纯逻辑）和它的渲染；
//! - `tabs`：标签列表的纯逻辑、终端标签、标签轨道里的标签；
//! - `host`：主机核心（`qishell-hosts`）包成 GPUI 实体 `HostStore`；
//! - `snippets`：命令片段面板和对话框；
//! - `secrets`：系统钥匙串（只存保险库的数据密钥）和打开数据目录里的保险库；
//! - `workspace`：窗口壳，持有全部 action 的处理器（`workspace/handlers/` 按领域分文件）。

pub mod app;
pub mod card;
pub mod host;
pub mod i18n;
pub mod programs;
pub mod secrets;
pub mod settings;
pub mod snippets;
pub mod split;
pub mod tabs;
pub mod theme;
pub mod workspace;

// 全部文案，来自 `locales/`：见 `i18n`。
rust_i18n::i18n!("locales", fallback = "zh-CN");

/// 初始化 gpui-kit、文案、字体、快捷键和全局 action。
/// 在 `Application::run` 里、建任何视图之前调用一次。
pub fn init(cx: &mut gpui_kit::App) {
    // 系统通知用的身份（Windows 上是 AppUserModelID），要在开窗口之前设。
    cx.set_app_identity(app::APP_IDENTITY, app::PRODUCT_NAME);
    app::init(cx);
}
