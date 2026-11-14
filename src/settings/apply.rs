//! 应用设置：应用外观、界面字号（固定，不是设置）、窗口背景（Mica），共用组件的样子（`MenuStyle`、`FindStyle`），以及终端读的全局量（`TerminalColors`、`TerminalFont`、
//! `TerminalInteraction`）。终端 crate 不依赖设置，开着的终端靠观察这些全局量跟着变。
//!
//! 窗口壳在打开时、每次设置变化时和系统外观变化时调用 `apply`：每一步都先比较，没变的不动，
//! 所以随便调多少次都行。
// 参考 since2006/shell-rs @ b2a307a: src/settings/apply.rs

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{App, Window, px};
use micaterm_terminal::{
    BUNDLED_FONT_FAMILY, TerminalColors, TerminalFont, TerminalInteraction, is_font_installed,
};

use super::backdrop::apply_backdrop;
use super::{AppSettings, DEFAULT_TERMINAL_FONT_FAMILY};
use crate::sftp_tab::ShowHiddenFiles;
use crate::theme::code_colors::code_highlight;
use crate::theme::{
    INTERFACE_FONT_SIZE, MONO_FONT_FAMILY, Tokens, TypeScale, UI_FONT_FAMILY, component_theme,
    component_theme_name,
};
use micaterm_ui::{FindStyle, MenuStyle};

/// 让窗口和设置一致：应用外观（跟随系统时看窗口当前的外观）、界面字号、终端主题、
/// 终端字体和交互。
pub fn apply(settings: &AppSettings, window: &mut Window, cx: &mut App) {
    // 每次都读窗口的外观，不只在设置变化时：跟随系统时，系统一变主题就跟着变。
    let mode = settings.appearance.theme_mode(window.appearance());
    apply_app_theme(mode, window, cx);

    // 界面字号：固定的（`theme::INTERFACE_FONT_SIZE`），在换主题之后设，换主题不会动它。
    let font_size = px(INTERFACE_FONT_SIZE);
    if cx.theme().font_size != font_size {
        Theme::global_mut(cx).font_size = font_size;
        Theme::sync_base(cx);
        window.refresh();
    }

    // 右键菜单（`micaterm_ui::Menu`）的颜色和字：界面字号、深浅一变就跟着换。
    let style = menu_style(&Tokens::current(cx), cx);
    if cx.try_global::<MenuStyle>() != Some(&style) {
        cx.set_global(style);
        window.refresh();
    }

    // 查找浮卡（`micaterm_ui::FindCard`）和匹配高亮的颜色、字。
    let style = find_style(&Tokens::current(cx), cx);
    if cx.try_global::<FindStyle>() != Some(&style) {
        cx.set_global(style);
        window.refresh();
    }

    // 终端背景跟着界面的内容底走，主题只管前景和 ANSI 色（26 号，用户定）。
    let colors = TerminalColors::new(settings.terminal_theme.theme(mode))
        .on_background(Tokens::current(cx).content);
    if cx.try_global::<TerminalColors>() != Some(&colors) {
        cx.set_global(colors);
        window.refresh();
    }

    // 编辑器的语法颜色和终端同一套（26 号「编辑器」）：换主题、换深浅都跟着变。
    let terminal = settings.terminal_theme.theme(mode);
    let highlight = code_highlight(&cx.theme().highlight_theme, terminal);
    if *cx.theme().highlight_theme != highlight {
        Theme::global_mut(cx).highlight_theme = std::sync::Arc::new(highlight);
        window.refresh();
    }

    // Mica 背景（工单 26）：系统不支持时保持不透明。
    apply_backdrop(settings.interface.mica, mode, window, cx);

    apply_terminal(settings, cx);
    apply_sftp(settings, cx);
}

/// 右键菜单的样子（26 号 `.menu`）：浅色底同内容区、深色 `#202124`，外圈 `--line-strong`；
/// 悬停 `--sel`；快捷键 `--st-dim` 的 Maple Mono 11.5（随界面字号）；置灰 `--faint`；危险 `--err`。
fn menu_style(tokens: &Tokens, cx: &App) -> MenuStyle {
    let scale = TypeScale::current(cx);
    MenuStyle {
        background: tokens.popup,
        border: tokens.line_strong,
        shadow: if cx.theme().is_dark() {
            gpui_kit::hsla(0., 0., 0., 0.45)
        } else {
            gpui_kit::hsla(0., 0., 0., 0.10)
        },
        separator: tokens.line,
        text: tokens.text,
        hint: tokens.st_dim,
        disabled: tokens.faint,
        danger: tokens.err,
        selected: tokens.sel,
        font: UI_FONT_FAMILY.into(),
        text_size: scale.ui,
        hint_font: MONO_FONT_FAMILY.into(),
        hint_size: scale.mono_small,
    }
}

/// 查找浮卡的样子（26 号 `.find`、`.hl`）：浅色底同内容区、深色 `#202124`，外圈 `--line-strong`，
/// 阴影浅色 `0 4 14 / .08`、深色 `/ .4`；放大镜和计数 `--st-dim`，按钮图标 `--icon`、悬停 `--hover`；
/// 匹配暖黄（`find_*` token）。
fn find_style(tokens: &Tokens, cx: &App) -> FindStyle {
    let scale = TypeScale::current(cx);
    FindStyle {
        background: tokens.popup,
        border: tokens.line_strong,
        shadow: if cx.theme().is_dark() {
            gpui_kit::hsla(0., 0., 0., 0.4)
        } else {
            gpui_kit::hsla(0., 0., 0., 0.08)
        },
        text: tokens.text,
        dim: tokens.st_dim,
        icon: tokens.icon,
        hover: tokens.hover,
        disabled: tokens.faint,
        separator: tokens.line,
        tooltip: tokens.tooltip,
        tooltip_text: tokens.tooltip_text,
        font: UI_FONT_FAMILY.into(),
        text_size: scale.ui,
        mono_font: MONO_FONT_FAMILY.into(),
        mono_size: scale.mono_small,
        matched: tokens.find_match,
        matched_line: tokens.find_match_line,
        current: tokens.find_current,
        current_text: tokens.find_current_text,
    }
}

/// SFTP 读的全局量：隐藏文件即时生效，并发数下一批生效。
fn apply_sftp(settings: &AppSettings, cx: &mut App) {
    let hidden = settings.sftp.show_hidden;
    if cx.try_global::<ShowHiddenFiles>() != Some(&hidden) {
        cx.set_global(hidden);
    }
    micaterm_ssh::sftp::set_parallel_transfers(settings.sftp.parallel);
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

/// 设置里的终端字体。本机没装时退回默认的 Maple Mono，它也没装就退回随包的 JetBrains Mono，
/// 否则 GPUI 会悄悄换成另一种字体。
pub fn terminal_font(settings: &AppSettings, installed: impl Fn(&str) -> bool) -> TerminalFont {
    let family = [
        settings.terminal_font.family(),
        DEFAULT_TERMINAL_FONT_FAMILY,
    ]
    .into_iter()
    .find(|family| *family == BUNDLED_FONT_FAMILY || installed(family))
    .unwrap_or(BUNDLED_FONT_FAMILY);
    TerminalFont {
        family: Some(family.to_string().into()),
        size: px(settings.terminal_font.size),
        line_height: settings.terminal_font.line_height,
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, px};

    use super::{apply_sftp, terminal_font};
    use crate::settings::AppSettings;

    /// 并发数是进程全局量。库的单元测试里只有这里改它；UI 测试并行时互相影响，不在那边断言。
    #[gpui_kit::test]
    fn the_parallel_transfer_count_reaches_the_engine(cx: &mut TestAppContext) {
        let mut settings = AppSettings::default();
        settings.sftp.parallel = 7;
        cx.update(|cx| apply_sftp(&settings, cx));
        assert_eq!(micaterm_ssh::sftp::parallel_transfers(), 7);
    }

    const MAPLE: &str = "Maple Mono Normal NL NF CN";

    #[test]
    fn the_default_terminal_font_is_maple_mono_at_13() {
        let font = terminal_font(&AppSettings::default(), |family| family == MAPLE);
        assert_eq!(font.family.as_deref(), Some(MAPLE));
        assert_eq!(font.size, px(13.));
        assert_eq!(font.line_height, 1.6);
        // 和终端 crate 自己的默认值一样：设置没写过时开出来的终端也是这样。
        assert_eq!(font, micaterm_terminal::TerminalFont::default());
    }

    #[test]
    fn a_family_not_installed_falls_back_to_maple_mono_then_the_bundled_one() {
        let mut settings = AppSettings::default();
        settings.terminal_font.family = Some("Cascadia Mono".into());
        let installed = terminal_font(&settings, |family| family == "Cascadia Mono");
        assert_eq!(installed.family.as_deref(), Some("Cascadia Mono"));
        let maple = terminal_font(&settings, |family| family == MAPLE);
        assert_eq!(maple.family.as_deref(), Some(MAPLE));
        let missing = terminal_font(&settings, |_| false);
        assert_eq!(missing.family.as_deref(), Some("JetBrains Mono"));
    }
}
