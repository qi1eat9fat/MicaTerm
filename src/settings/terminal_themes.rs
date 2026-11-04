//! 外观 › 终端主题：左栏浅色、右栏深色，同一行是同一主题的浅色版和深色版。
//! 每张卡片用主题自己的颜色画一小段 shell；选中的描强调色边、右上角打勾。
//! 点一张就写进设置：它属于哪一栏就换哪一栏。
// 参考 since2006/shell-rs @ b2a307a: src/settings/terminal_themes.rs（改成原型的两栏卡片）

use gpui_kit::component::{Icon, IconName, ThemeMode};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use qishell_terminal::{DEFAULT_FONT_FAMILY, TerminalTheme};

use super::{SettingsStore, TerminalThemeSettings};
use crate::i18n::t;
use crate::theme::Tokens;

/// 卡片里的一小段 shell：每一段写着用主题的哪个 ANSI 颜色画，`None` 是主题的文字色。
const SAMPLE: &[&[(&str, Option<usize>)]] = &[
    &[
        ("qi@web-01", Some(2)),
        (":", None),
        ("~", Some(4)),
        ("$ ls", None),
    ],
    &[
        ("config", Some(4)),
        ("  ", None),
        ("deploy.sh", Some(2)),
        ("  ", None),
        ("error.log", Some(1)),
    ],
];

/// 主题卡片的元素 id：`terminal-theme-<key>`。
pub fn theme_card_id(theme: &TerminalTheme) -> ElementId {
    ElementId::Name(format!("terminal-theme-{}", theme.key()).into())
}

/// 两栏主题卡片。
pub fn terminal_theme_grid(store: &Entity<SettingsStore>, tokens: &Tokens, cx: &App) -> Div {
    let chosen = store.read(cx).settings().terminal_theme;
    let column = |mode| {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(8.))
            .children(
                TerminalTheme::for_mode(mode).map(|theme| card(theme, &chosen, store, tokens)),
            )
    };
    div()
        .flex()
        .gap(px(8.))
        .px(px(14.))
        .py(px(12.))
        .child(column(ThemeMode::Light))
        .child(column(ThemeMode::Dark))
}

fn card(
    theme: &'static TerminalTheme,
    chosen: &TerminalThemeSettings,
    store: &Entity<SettingsStore>,
    tokens: &Tokens,
) -> AnyElement {
    let selected = chosen.theme(theme.mode()) == theme;
    let store = store.clone();
    let label = if selected {
        t!("settings.appearance.theme_chosen", name = theme.name())
    } else {
        theme.name().into()
    };
    div()
        .id(theme_card_id(theme))
        .test_support()
        .role(Role::RadioButton)
        .aria_label(label)
        .aria_selected(selected)
        .relative()
        .px(px(10.))
        .py(px(8.))
        .rounded(px(9.))
        .bg(theme.background())
        .text_color(theme.foreground())
        .cursor_pointer()
        .map(|card| {
            if selected {
                card.border_2().border_color(tokens.accent)
            } else {
                card.border_1().border_color(tokens.line)
            }
        })
        .on_click(move |_, _, cx| {
            store.update(cx, |store, cx| {
                store.update(|settings| settings.terminal_theme.choose(theme), cx)
            });
        })
        .child(
            div()
                .mb(px(3.))
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .child(theme.name()),
        )
        .child(
            div()
                .font_family(DEFAULT_FONT_FAMILY)
                .text_size(px(11.))
                .line_height(relative(1.5))
                .children(SAMPLE.iter().map(|line| {
                    div().flex().children(line.iter().map(|(text, color)| {
                        div()
                            .whitespace_nowrap()
                            .when_some(*color, |part, index| {
                                part.text_color(theme.indexed(index as u8))
                            })
                            .child(*text)
                    }))
                })),
        )
        .when(selected, |card| {
            card.child(
                div()
                    .absolute()
                    .top(px(6.))
                    .right(px(8.))
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(tokens.accent)
                    .text_color(tokens.on_accent)
                    .child(Icon::new(IconName::Check).size(px(10.))),
            )
        })
        .into_any_element()
}
