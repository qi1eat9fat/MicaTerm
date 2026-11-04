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

use gpui_kit::WindowAppearance;
use gpui_kit::component::ThemeMode;
use serde::{Deserialize, Serialize};

pub use qishell_terminal::{RightClick, TerminalCursorStyle, TerminalInteraction};
use qishell_terminal::TerminalTheme;

/// 全部设置。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// 外观 → 应用外观。
    pub appearance: Appearance,
    /// 外观 → 界面字号。
    pub interface: InterfaceSettings,
    /// 外观 → 浅色、深色终端主题。
    pub terminal_theme: TerminalThemeSettings,
    /// 终端 → 字体、字号、行高。
    pub terminal_font: TerminalFontSettings,
    /// 终端 → 选中即复制、右键行为、光标。原样交给终端的全局量。
    pub terminal_interaction: TerminalInteraction,
    /// 终端 → 本地终端的 shell。
    pub local_terminal: LocalTerminalSettings,
}

impl AppSettings {
    /// 把每个数值收进应用接受的范围。读文件和每次改动都要过一遍，
    /// 这样手改的文件或者输入的值都不会让终端字号变成 0。
    pub fn normalized(mut self) -> Self {
        self.interface = self.interface.normalized();
        self.terminal_font = self.terminal_font.normalized();
        self.local_terminal.shell = self
            .local_terminal
            .shell
            .map(|shell| shell.trim().to_string())
            .filter(|shell| !shell.is_empty());
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

/// 界面字号的范围，单位像素。
pub const INTERFACE_FONT_SIZE_RANGE: RangeInclusive<f32> = 12.0..=20.0;
/// 默认界面字号：Windows 上 14，对齐系统 9pt 正文；其他平台 16。
pub const DEFAULT_INTERFACE_FONT_SIZE: f32 = if cfg!(windows) { 14.0 } else { 16.0 };

/// 外观 → 常规。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceSettings {
    /// 界面的基准字号（像素）。终端有自己的字号。
    pub font_size: f32,
}

impl Default for InterfaceSettings {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_INTERFACE_FONT_SIZE,
        }
    }
}

impl InterfaceSettings {
    /// 整像素，收进范围；不是数字时取默认值。
    fn normalized(mut self) -> Self {
        self.font_size = clamp_or(
            self.font_size,
            INTERFACE_FONT_SIZE_RANGE,
            DEFAULT_INTERFACE_FONT_SIZE,
        )
        .round();
        self
    }
}

/// 默认的浅色终端主题。主题 key 发布后不能改：它写进 `settings.json`。
pub const DEFAULT_LIGHT_THEME: &str = "qishell-light";
/// 默认的深色终端主题。
pub const DEFAULT_DARK_THEME: &str = "qishell-dark";

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

/// 终端默认字体：随包附带的 JetBrains Mono（`assets/fonts/`）。
pub const DEFAULT_TERMINAL_FONT_FAMILY: &str = qishell_terminal::DEFAULT_FONT_FAMILY;
/// 终端默认字号，单位像素（gorex 的取值）。
pub const DEFAULT_TERMINAL_FONT_SIZE: f32 = qishell_terminal::DEFAULT_FONT_SIZE;
/// 终端默认行高，字号的倍数。
pub const DEFAULT_TERMINAL_LINE_HEIGHT: f32 = qishell_terminal::DEFAULT_LINE_HEIGHT;
/// 终端接受的字号。
pub const TERMINAL_FONT_SIZE_RANGE: RangeInclusive<f32> = qishell_terminal::FONT_SIZE_RANGE;
/// 终端接受的行高。
pub const TERMINAL_LINE_HEIGHT_RANGE: RangeInclusive<f32> = qishell_terminal::LINE_HEIGHT_RANGE;

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

    /// 字号保留一位小数（默认 11.6 不能被取整掉），行高保留两位，都收进范围；
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
        assert_eq!(settings.terminal_font.family(), "JetBrains Mono");
        assert_eq!(settings.terminal_font.size, 11.6);
        assert_eq!(settings.terminal_theme.light, "qishell-light");
        assert_eq!(settings.terminal_theme.dark, "qishell-dark");
        assert_eq!(settings.terminal_interaction.right_click, RightClick::Menu);
        assert!(!settings.terminal_interaction.copy_on_select);
        assert!(settings.terminal_interaction.cursor_blinking);
        assert_eq!(settings.local_terminal.shell, None);
    }

    #[test]
    fn the_file_keeps_its_keys() {
        let json = r#"{
            "appearance": "dark",
            "interface": { "font_size": 15 },
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
        assert_eq!(settings.interface.font_size, 15.);
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
        assert_eq!(themes.theme(ThemeMode::Light).name(), "QiShell Light");
        assert_eq!(themes.theme(ThemeMode::Dark).name(), "QiShell Dark");
        themes.light = "solarized-light".into();
        themes.dark = "tokyo-day".into();
        assert_eq!(themes.theme(ThemeMode::Light).name(), "QiShell Light");
        assert_eq!(themes.theme(ThemeMode::Dark).name(), "QiShell Dark");

        let rose_pine = qishell_terminal::TerminalTheme::find("rose-pine").unwrap();
        themes.choose(rose_pine);
        assert_eq!(themes.dark, "rose-pine");
        assert_eq!(themes.theme(ThemeMode::Dark).name(), "Rosé Pine");
        assert_eq!(themes.light, "solarized-light");
    }

    #[test]
    fn numbers_out_of_range_are_brought_back() {
        let mut settings = AppSettings::default();
        settings.interface.font_size = 99.;
        settings.terminal_font.size = 11.63;
        settings.terminal_font.line_height = f32::NAN;
        settings.terminal_font.family = Some("   ".into());
        settings.local_terminal.shell = Some("  ".into());
        let settings = settings.normalized();
        assert_eq!(settings.interface.font_size, 20.);
        assert_eq!(settings.terminal_font.size, 11.6);
        assert_eq!(settings.terminal_font.line_height, 1.45);
        assert_eq!(settings.terminal_font.family, None);
        assert_eq!(settings.local_terminal.shell, None);
        // 默认值本身不被改动。
        assert_eq!(AppSettings::default().normalized(), AppSettings::default());
    }
}
