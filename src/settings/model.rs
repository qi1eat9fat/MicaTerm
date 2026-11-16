//! 设置模型：`settings.json` 里存的全部内容。
//!
//! M1 的字段在这里一次写全（外观、界面字号、终端主题、终端字体、终端交互、本地终端），
//! 设置对话框（工单 07）只做界面，不改模型的结构。每个字段都 `#[serde(default)]`：
//! 文件里没写的项取默认值，旧版本写的文件照样能读。
//!
//! 写进文件的键（字段名、枚举的 `snake_case` 值、主题 key）发布后不能改。
// 来自 since2006/shell-rs @ b2a307a: src/settings/model.rs（裁剪：只留 M1 的字段；
// 「终端交互」直接用终端 crate 的 `TerminalInteraction`，终端字体的默认值和范围也取自终端 crate）

use std::ops::RangeInclusive;
use std::path::PathBuf;

use gpui_kit::WindowAppearance;
use gpui_kit::component::ThemeMode;
use serde::{Deserialize, Serialize};

use micaterm_ssh::sftp::{DEFAULT_PARALLEL_TRANSFERS, PARALLEL_TRANSFERS_RANGE};
use micaterm_terminal::TerminalTheme;
pub use micaterm_terminal::{RightClick, TerminalCursorStyle, TerminalInteraction};

use crate::sftp_tab::ShowHiddenFiles;

/// 全部设置。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// 外观 → 应用外观。
    pub appearance: Appearance,
    /// 外观 → Mica 背景。
    pub interface: InterfaceSettings,
    /// 外观 → 浅色、深色终端主题。
    pub terminal_theme: TerminalThemeSettings,
    /// 终端 → 字体、字号、行高。
    pub terminal_font: TerminalFontSettings,
    /// 终端 → 选中即复制、右键行为、光标。原样交给终端的全局量。
    pub terminal_interaction: TerminalInteraction,
    /// 终端 → 本地终端的 shell。
    pub local_terminal: LocalTerminalSettings,
    /// SFTP（工单 20 定好模型，25 只加页面）。
    pub sftp: SftpSettings,
    /// 应用 → 窗口（工单 26）。
    pub window: WindowSettings,
    /// 外部 CLI（Agent CLI 工单 01）。
    pub cli: CliSettings,
}

/// 外部 CLI：`micaterm` 命令给 AI Agent 用。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CliSettings {
    /// 启用外部 Agent（默认关）。关着时照样在听，请求一律回「没启用」，Agent 才知道让用户来开它。
    pub enabled: bool,
    /// 应用内启动 Agent 时运行的命令（Agent CLI 工单 07）；`None` 是自动检测（先找 claude，再找 codex）。
    pub agent_command: Option<String>,
    /// 应用内的 Agent 在哪个文件夹里启动：Agent 面板标题行的「换文件夹」选的，下次启动还用它。
    /// `None` 是主目录；文件夹不在了（数据目录拷到了别的电脑）也退回主目录。
    pub agent_directory: Option<PathBuf>,
}

/// 应用 → 窗口。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    /// 记住窗口大小和位置：下次启动放回上次的大小、位置和最大化状态（默认开）。
    /// 窗口的样子总是记在数据目录的 `window.json` 里，这个开关只决定下次启动用不用它。
    pub remember: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self { remember: true }
    }
}

impl AppSettings {
    /// 把每个数值收进应用接受的范围。读文件和每次改动都要过一遍，
    /// 这样手改的文件或者输入的值都不会让终端字号变成 0。
    pub fn normalized(mut self) -> Self {
        self.terminal_font = self.terminal_font.normalized();
        self.local_terminal.shell = self
            .local_terminal
            .shell
            .map(|shell| shell.trim().to_string())
            .filter(|shell| !shell.is_empty());
        self.sftp = self.sftp.normalized();
        self.cli.agent_directory = self
            .cli
            .agent_directory
            .filter(|directory| !directory.as_os_str().is_empty());
        self
    }
}

/// SFTP：同时传输数量、本地默认打开目录、显示隐藏文件。
/// 旧文件里的 `file_icon_style`（图标风格，工单 12 去掉了）读的时候忽略。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SftpSettings {
    /// 每个 SFTP 标签同时传几个文件（1–10，默认 3）。
    pub parallel: usize,
    /// 本地一侧从哪里开始；`None` 是「下载」文件夹，不把本机的路径写进文件（数据目录能拷到别的电脑）。
    pub local_directory: Option<PathBuf>,
    /// 显示隐藏文件，本地和远程各一个开关。
    pub show_hidden: ShowHiddenFiles,
}

impl Default for SftpSettings {
    fn default() -> Self {
        Self {
            parallel: DEFAULT_PARALLEL_TRANSFERS,
            local_directory: None,
            show_hidden: ShowHiddenFiles::default(),
        }
    }
}

impl SftpSettings {
    /// 数量收进范围；空的目录等于没设。
    fn normalized(mut self) -> Self {
        self.parallel = self.parallel.clamp(
            *PARALLEL_TRANSFERS_RANGE.start(),
            *PARALLEL_TRANSFERS_RANGE.end(),
        );
        self.local_directory = self
            .local_directory
            .filter(|directory| !directory.as_os_str().is_empty());
        self
    }
}

/// 应用外观，默认跟随系统。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    Dark,
    Light,
    #[default]
    System,
}

impl Appearance {
    /// 窗口当前是 `window` 外观时该用的主题模式。
    pub fn theme_mode(self, window: WindowAppearance) -> ThemeMode {
        match self {
            Self::Dark => ThemeMode::Dark,
            Self::Light => ThemeMode::Light,
            Self::System => window.into(),
        }
    }
}

/// 外观 → 常规。界面字号不是设置（用户定，见 `theme::INTERFACE_FONT_SIZE`）；
/// 旧文件里的 `font_size` 读的时候不认，下次写文件就没了。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceSettings {
    /// Mica 背景（默认关，工单 26）。只在 Windows 11 22H2 及以上生效，别处开着也不起作用。
    pub mica: bool,
}

/// 默认的浅色终端主题。主题 key 发布后不能改：它写进 `settings.json`。
pub const DEFAULT_LIGHT_THEME: &str = "micaterm-light";
/// 默认的深色终端主题。
pub const DEFAULT_DARK_THEME: &str = "micaterm-dark";

/// 外观 → 终端主题：浅色和深色各选一套（从内置的 4 对里选），按 key 存。
/// 这个版本不认识的 key 由使用方退回默认主题，不让整个文件读失败。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalThemeSettings {
    pub light: String,
    pub dark: String,
}

impl Default for TerminalThemeSettings {
    fn default() -> Self {
        Self {
            light: DEFAULT_LIGHT_THEME.into(),
            dark: DEFAULT_DARK_THEME.into(),
        }
    }
}

impl TerminalThemeSettings {
    /// `mode` 那一栏选的主题。key 不认识（别的版本写的、手改的）或者是另一栏的主题时，
    /// 退回这一栏的默认主题。
    pub fn theme(&self, mode: ThemeMode) -> &'static TerminalTheme {
        let key = match mode {
            ThemeMode::Light => &self.light,
            ThemeMode::Dark => &self.dark,
        };
        TerminalTheme::find(key)
            .filter(|theme| theme.mode() == mode)
            .unwrap_or_else(|| TerminalTheme::default_for(mode))
    }

    /// 选 `theme`：它属于哪一栏就换哪一栏。
    pub fn choose(&mut self, theme: &'static TerminalTheme) {
        let key = theme.key().to_string();
        match theme.mode() {
            ThemeMode::Light => self.light = key,
            ThemeMode::Dark => self.dark = key,
        }
    }
}

/// 终端默认字体：Maple Mono Normal NL NF CN（本机没装时退回随包的 JetBrains Mono，见 `settings::apply`）。
pub const DEFAULT_TERMINAL_FONT_FAMILY: &str = micaterm_terminal::DEFAULT_FONT_FAMILY;
/// 终端默认字号，单位像素（26 号原型）。
pub const DEFAULT_TERMINAL_FONT_SIZE: f32 = micaterm_terminal::DEFAULT_FONT_SIZE;
/// 终端默认行高，字号的倍数。
pub const DEFAULT_TERMINAL_LINE_HEIGHT: f32 = micaterm_terminal::DEFAULT_LINE_HEIGHT;
/// 终端接受的字号。
pub const TERMINAL_FONT_SIZE_RANGE: RangeInclusive<f32> = micaterm_terminal::FONT_SIZE_RANGE;
/// 终端接受的行高。
pub const TERMINAL_LINE_HEIGHT_RANGE: RangeInclusive<f32> = micaterm_terminal::LINE_HEIGHT_RANGE;

/// 终端 → 字体。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalFontSettings {
    /// `None` 是默认字体 `DEFAULT_TERMINAL_FONT_FAMILY`。
    pub family: Option<String>,
    /// 像素。
    pub size: f32,
    /// `size` 的倍数。
    pub line_height: f32,
}

impl Default for TerminalFontSettings {
    fn default() -> Self {
        Self {
            family: None,
            size: DEFAULT_TERMINAL_FONT_SIZE,
            line_height: DEFAULT_TERMINAL_LINE_HEIGHT,
        }
    }
}

impl TerminalFontSettings {
    /// 实际使用的字体名。
    pub fn family(&self) -> &str {
        self.family
            .as_deref()
            .unwrap_or(DEFAULT_TERMINAL_FONT_FAMILY)
    }

    /// 字号保留一位小数（11.6 这类不能被取整掉），行高保留两位，都收进范围；
    /// 不是数字的取默认值；空白的字体名等于没设。
    fn normalized(mut self) -> Self {
        self.size = (clamp_or(
            self.size,
            TERMINAL_FONT_SIZE_RANGE,
            DEFAULT_TERMINAL_FONT_SIZE,
        ) * 10.)
            .round()
            / 10.;
        self.line_height = (clamp_or(
            self.line_height,
            TERMINAL_LINE_HEIGHT_RANGE,
            DEFAULT_TERMINAL_LINE_HEIGHT,
        ) * 100.)
            .round()
            / 100.;
        self.family = self
            .family
            .map(|family| family.trim().to_string())
            .filter(|family| !family.is_empty());
        self
    }
}

/// 终端 → 本地终端。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalTerminalSettings {
    /// 新开的本地终端运行的程序（路径或命令，可带参数）。`None` 是自动检测
    /// （Windows 优先 PowerShell 7），每次重新找。
    pub shell: Option<String>,
}

/// `value` 收进 `range`；不是有限数时取 `default`。
fn clamp_or(value: f32, range: RangeInclusive<f32>, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(*range.start(), *range.end())
    } else {
        default
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, Appearance, RightClick, TerminalCursorStyle};

    #[test]
    fn an_empty_file_gives_the_defaults() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, AppSettings::default());
        assert_eq!(settings.appearance, Appearance::System);
        assert_eq!(
            settings.terminal_font.family(),
            "Maple Mono Normal NL NF CN"
        );
        assert_eq!(settings.terminal_font.size, 13.);
        assert_eq!(settings.terminal_theme.light, "micaterm-light");
        assert_eq!(settings.terminal_theme.dark, "micaterm-dark");
        assert_eq!(settings.terminal_interaction.right_click, RightClick::Menu);
        assert!(!settings.terminal_interaction.copy_on_select);
        assert!(settings.terminal_interaction.cursor_blinking);
        assert_eq!(settings.local_terminal.shell, None);
        assert_eq!(settings.sftp.parallel, 3);
        assert_eq!(settings.sftp.local_directory, None);
        assert!(!settings.sftp.show_hidden.local && !settings.sftp.show_hidden.remote);
        assert!(!settings.interface.mica);
        assert!(settings.window.remember);
        assert!(!settings.cli.enabled);
    }

    #[test]
    fn mica_and_remembering_the_window_keep_their_keys() {
        let json = r#"{ "interface": { "mica": true }, "window": { "remember": false } }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.interface.mica);
        assert!(!settings.window.remember);
        let again: AppSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(again, settings);
    }

    #[test]
    fn the_sftp_settings_keep_their_keys_and_range() {
        let json = r#"{ "sftp": {
            "parallel": 99,
            "local_directory": "D:/Work",
            "file_icon_style": "monochrome",
            "show_hidden": { "local": true }
        } }"#;
        let settings = serde_json::from_str::<AppSettings>(json)
            .unwrap()
            .normalized();
        assert_eq!(settings.sftp.parallel, 10);
        assert_eq!(
            settings.sftp.local_directory.as_deref(),
            Some(std::path::Path::new("D:/Work"))
        );
        assert!(settings.sftp.show_hidden.local && !settings.sftp.show_hidden.remote);
    }

    #[test]
    fn the_file_keeps_its_keys() {
        let json = r#"{
            "appearance": "dark",
            "interface": { "font_size": 14, "mica": true },
            "terminal_theme": { "light": "tokyo-day", "dark": "rose-pine" },
            "terminal_font": { "family": "Cascadia Mono", "size": 13.5, "line_height": 1.2 },
            "terminal_interaction": {
                "copy_on_select": true,
                "right_click": "copy_or_paste",
                "cursor_style": "beam",
                "cursor_blinking": false
            },
            "local_terminal": { "shell": "pwsh -NoLogo" }
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.appearance, Appearance::Dark);
        // 以前的界面字号不再是设置：读的时候不认，写回去也没有它。
        assert!(settings.interface.mica);
        assert!(
            !serde_json::to_string(&settings)
                .unwrap()
                .contains("\"font_size\":14")
        );
        assert_eq!(settings.terminal_theme.dark, "rose-pine");
        assert_eq!(settings.terminal_font.family(), "Cascadia Mono");
        assert_eq!(settings.terminal_font.line_height, 1.2);
        assert_eq!(
            settings.terminal_interaction.right_click,
            RightClick::CopyOrPaste
        );
        assert_eq!(
            settings.terminal_interaction.cursor_style,
            TerminalCursorStyle::Beam
        );
        assert_eq!(
            settings.local_terminal.shell.as_deref(),
            Some("pwsh -NoLogo")
        );
        // 写回去再读，一样。
        let again: AppSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(again, settings);
    }

    #[test]
    fn each_column_falls_back_to_its_default_theme() {
        use gpui_kit::component::ThemeMode;
        let mut themes = super::TerminalThemeSettings::default();
        assert_eq!(themes.theme(ThemeMode::Light).name(), "MicaTerm Light");
        assert_eq!(themes.theme(ThemeMode::Dark).name(), "MicaTerm Dark");
        themes.light = "solarized-light".into();
        themes.dark = "tokyo-day".into();
        assert_eq!(themes.theme(ThemeMode::Light).name(), "MicaTerm Light");
        assert_eq!(themes.theme(ThemeMode::Dark).name(), "MicaTerm Dark");

        let rose_pine = micaterm_terminal::TerminalTheme::find("rose-pine").unwrap();
        themes.choose(rose_pine);
        assert_eq!(themes.dark, "rose-pine");
        assert_eq!(themes.theme(ThemeMode::Dark).name(), "Rosé Pine Moon");
        assert_eq!(themes.light, "solarized-light");
    }

    #[test]
    fn numbers_out_of_range_are_brought_back() {
        let mut settings = AppSettings::default();
        settings.terminal_font.size = 11.63;
        settings.terminal_font.line_height = f32::NAN;
        settings.terminal_font.family = Some("   ".into());
        settings.local_terminal.shell = Some("  ".into());
        let settings = settings.normalized();
        assert_eq!(settings.terminal_font.size, 11.6);
        assert_eq!(settings.terminal_font.line_height, 1.6);
        assert_eq!(settings.terminal_font.family, None);
        assert_eq!(settings.local_terminal.shell, None);
        // 默认值本身不被改动。
        assert_eq!(AppSettings::default().normalized(), AppSettings::default());
    }
}
