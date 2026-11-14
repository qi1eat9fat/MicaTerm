//! 外观 › 终端主题：左栏浅色、右栏深色，同一行是同一主题的浅色版和深色版。
//! 每张卡片用主题自己的颜色画一小段 shell；选中的描正文色边（26 号 `.thc.on`）。
//! 点一张就写进设置：它属于哪一栏就换哪一栏。
// 参考 since2006/shell-rs @ b2a307a: src/settings/terminal_themes.rs（改成原型的两栏卡片）

use gpui_kit::component::ThemeMode;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use micaterm_terminal::{DEFAULT_FONT_FAMILY, TerminalTheme};

use super::{SettingsStore, TerminalThemeSettings};
use crate::i18n::t;
use crate::theme::Tokens;

/// 卡片里的字（26 号 `.thc`）：卡片是一小块终端的样子，三档界面字号都是 12，大了 64 高放不下。
const CARD_TEXT: Pixels = px(12.);

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
        (" ", None),
        ("deploy.sh", Some(2)),
        (" ", None),
        ("error.log", Some(1)),
    ],
];

/// 主题卡片的元素 id：`terminal-theme-<key>`。
pub fn theme_card_id(theme: &TerminalTheme) -> ElementId {
    ElementId::Name(format!("terminal-theme-{}", theme.key()).into())
}

/// 两栏主题卡片（26 号 `.stg-th`）：上面一行「浅色」「深色」，下面每行一对；行距 10、栏距 12。
pub fn terminal_theme_grid(store: &Entity<SettingsStore>, tokens: &Tokens, cx: &App) -> Div {
    let chosen = store.read(cx).settings().terminal_theme;
    let column = |mode, title: SharedString| {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(10.))
            .child(
                div()
                    .text_color(tokens.st_dim)
                    .line_height(px(18.))
                    .child(title),
            )
            .children(
                TerminalTheme::for_mode(mode).map(|theme| card(theme, &chosen, store, tokens)),
            )
    };
    div()
        .flex()
        .gap(px(12.))
        .pt(px(12.))
        .pb(px(4.))
        .child(column(ThemeMode::Light, t!("settings.appearance.light")))
        .child(column(ThemeMode::Dark, t!("settings.appearance.dark")))
}

/// 一张主题卡片（26 号 `.thc`）：就是一小块终端。高 64、圆角 6、细边；底色是那种深浅的界面内容底
/// （终端背景跟界面走），字是主题的前景和 ANSI 色。选中的是正文色的 2px 边。
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
    let background = match theme.mode() {
        ThemeMode::Light => Tokens::light().content,
        ThemeMode::Dark => Tokens::dark().content,
    };
    // 三行字在卡片里上下居中（用户定）。26 号的边是不占地方的阴影，字离左沿 10；这里是真边框，
    // 左右内边距扣掉边宽。选中时上下边一起变粗，居中的位置不变，字不跟着选中动。
    let border = if selected { px(2.) } else { px(1.) };
    div()
        .id(theme_card_id(theme))
        .test_support()
        .role(Role::RadioButton)
        .aria_label(label)
        .aria_selected(selected)
        .flex()
        .flex_col()
        .justify_center()
        .h(px(64.))
        .px(px(10.) - border)
        .rounded(crate::theme::size::RADIUS)
        .overflow_hidden()
        .bg(background)
        .text_color(theme.foreground())
        .text_size(CARD_TEXT)
        .line_height(relative(1.5))
        .cursor_pointer()
        .border(border)
        .border_color(if selected { tokens.text } else { tokens.line })
        .on_click(move |_, _, cx| {
            store.update(cx, |store, cx| {
                store.update(|settings| settings.terminal_theme.choose(theme), cx)
            });
        })
        .child(div().mb(px(2.)).opacity(0.75).child(theme.name()))
        .child(
            div()
                .font_family(DEFAULT_FONT_FAMILY)
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
        .into_any_element()
}
