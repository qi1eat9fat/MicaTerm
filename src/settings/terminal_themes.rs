//! 外观 › 终端主题：只列当前界面明暗那一色的主题（浅色界面看浅色，深色界面看深色），
//! 分两列排开；另一色不出现——它跟当前界面不搭，看着突兀（2026-11-26 用户定）。
//! 每张卡片用主题自己的颜色画一小段 shell；选中的描正文色边（26 号 `.thc.on`）。
//! 点一张就写进设置：它属于哪一色就换哪一色。
// 参考 since2006/shell-rs @ b2a307a: src/settings/terminal_themes.rs（改成原型的两栏卡片）

use gpui_kit::component::ThemeMode;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use micaterm_terminal::{DEFAULT_FONT_FAMILY, TerminalTheme};

use super::{SettingsStore, TerminalThemeSettings};
use crate::i18n::t;
use crate::theme::Tokens;
use crate::theme::space;

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

/// `mode` 那色的主题卡片（26 号 `.stg-th`）：分两列、按顺序填，左列是前半、右列是后半；
/// 行距 10、栏距 12。另一色的主题不列。
pub fn terminal_theme_grid(
    store: &Entity<SettingsStore>,
    mode: ThemeMode,
    tokens: &Tokens,
    cx: &App,
) -> Div {
    let chosen = store.read(cx).settings().terminal_theme;
    let themes: Vec<&'static TerminalTheme> = TerminalTheme::for_mode(mode).collect();
    let split = themes.len().div_ceil(2);
    let column = |themes: &[&'static TerminalTheme]| {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(space::SPACE_10)
            .children(
                themes
                    .iter()
                    .map(|theme| card(theme, &chosen, store, tokens)),
            )
    };
    div()
        .flex()
        .gap(space::SPACE_12)
        .pt(space::SPACE_12)
        .pb(space::SPACE_4)
        .child(column(&themes[..split]))
        .child(column(&themes[split..]))
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
        .px(space::SPACE_10 - border)
        .rounded(crate::theme::size::RADIUS_MD)
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
        .child(div().mb(space::SPACE_2).opacity(0.75).child(theme.name()))
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
