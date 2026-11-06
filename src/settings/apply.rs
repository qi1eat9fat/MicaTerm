//! 应用设置：应用外观、界面字号，以及终端读的全局量（`TerminalColors`、`TerminalFont`、
//! `TerminalInteraction`）。终端 crate 不依赖设置，开着的终端靠观察这些全局量跟着变。
//!
//! 窗口壳在打开时、每次设置变化时和系统外观变化时调用 `apply`：每一步都先比较，没变的不动，
//! 所以随便调多少次都行。
// 参考 since2006/shell-rs @ b2a307a: src/settings/apply.rs

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{App, Window, px};
use qishell_terminal::{TerminalColors, TerminalFont, TerminalInteraction, is_font_installed};

use super::{AppSettings, DEFAULT_TERMINAL_FONT_FAMILY};
use crate::sftp_tab::{FileIconStyle, ShowHiddenFiles};
use crate::theme::{component_theme, component_theme_name};

/// 让窗口和设置一致：应用外观（跟随系统时看窗口当前的外观）、界面字号、终端主题、
/// 终端字体和交互。
pub fn apply(settings: &AppSettings, window: &mut Window, cx: &mut App) {
    // 每次都读窗口的外观，不只在设置变化时：跟随系统时，系统一变主题就跟着变。
    let mode = settings.appearance.theme_mode(window.appearance());
    apply_app_theme(mode, window, cx);

    // 界面字号：在换主题之后设，换主题不会动它。
    let font_size = px(settings.interface.font_size);
    if cx.theme().font_size != font_size {
        Theme::global_mut(cx).font_size = font_size;
        Theme::sync_base(cx);
        window.refresh();
    }

    let colors = TerminalColors::new(settings.terminal_theme.theme(mode));
    if cx.try_global::<TerminalColors>() != Some(&colors) {
        cx.set_global(colors);
        window.refresh();
    }

    apply_terminal(settings, cx);
    apply_sftp(settings, cx);
}

/// SFTP 读的全局量：文件图标风格和隐藏文件即时生效，并发数下一批生效。
fn apply_sftp(settings: &AppSettings, cx: &mut App) {
    let style = settings.sftp.file_icon_style;
    if cx.try_global::<FileIconStyle>() != Some(&style) {
        cx.set_global(style);
    }
    let hidden = settings.sftp.show_hidden;
    if cx.try_global::<ShowHiddenFiles>() != Some(&hidden) {
        cx.set_global(hidden);
    }
    qishell_ssh::sftp::set_parallel_transfers(settings.sftp.parallel);
}

/// 换成 `mode` 的界面主题（原型 token 映射出来的那套）；已经是它就不动。
fn apply_app_theme(mode: ThemeMode, window: &mut Window, cx: &mut App) {
    let name = component_theme_name(mode);
    if cx.theme().mode == mode && cx.theme().theme_name().as_ref() == name {
        return;
    }
    let config = component_theme(mode, cx);
    let theme = Theme::global_mut(cx);
    match mode {
        ThemeMode::Light => theme.light_theme = config,
        ThemeMode::Dark => theme.dark_theme = config,
    }
    Theme::change(mode, Some(window), cx);
}

/// 把终端相关的设置交给终端：字体、字号、行高，交互。没变的不重设，免得白白重画。
pub fn apply_terminal(settings: &AppSettings, cx: &mut App) {
    let font = terminal_font(settings, |family| is_font_installed(family, cx));
    if cx.try_global::<TerminalFont>() != Some(&font) {
        cx.set_global(font);
    }
    let interaction = settings.terminal_interaction;
    if cx.try_global::<TerminalInteraction>() != Some(&interaction) {
        cx.set_global(interaction);
    }
}

/// 设置里的终端字体。随包的 JetBrains Mono 总能用；别的字体本机没装时退回它，
/// 否则 GPUI 会悄悄换成另一种字体。
pub fn terminal_font(settings: &AppSettings, installed: impl Fn(&str) -> bool) -> TerminalFont {
    let family = settings.terminal_font.family();
    let family = if family == DEFAULT_TERMINAL_FONT_FAMILY || installed(family) {
        family
    } else {
        DEFAULT_TERMINAL_FONT_FAMILY
    };
    TerminalFont {
        family: Some(family.to_string().into()),
        size: px(settings.terminal_font.size),
        line_height: settings.terminal_font.line_height,
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::terminal_font;
    use crate::settings::AppSettings;

    #[test]
    fn the_default_terminal_font_is_jetbrains_mono_at_11_6() {
        let font = terminal_font(&AppSettings::default(), |_| false);
        assert_eq!(font.family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(font.size, px(11.6));
        assert_eq!(font.line_height, 1.45);
        // 和终端 crate 自己的默认值一样：设置没写过时开出来的终端也是这样。
        assert_eq!(font, qishell_terminal::TerminalFont::default());
    }

    #[test]
    fn a_family_not_installed_falls_back_to_the_bundled_one() {
        let mut settings = AppSettings::default();
        settings.terminal_font.family = Some("Cascadia Mono".into());
        let installed = terminal_font(&settings, |family| family == "Cascadia Mono");
        assert_eq!(installed.family.as_deref(), Some("Cascadia Mono"));
        let missing = terminal_font(&settings, |_| false);
        assert_eq!(missing.family.as_deref(), Some("JetBrains Mono"));
    }
}
