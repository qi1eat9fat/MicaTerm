//! 应用设置：把设置变成终端读的全局量（`TerminalFont`、`TerminalInteraction`）。
//! 终端 crate 不依赖设置，开着的终端靠观察这些全局量跟着变。
//!
//! 窗口壳在打开时和每次设置变化时调用 `apply_terminal`。工单 07 在这里加终端主题（`TerminalColors`）。

use gpui_kit::{App, px};
use qishell_terminal::{TerminalFont, TerminalInteraction, is_font_installed};

use super::{AppSettings, DEFAULT_TERMINAL_FONT_FAMILY};

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
