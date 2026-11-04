//! 设置对话框各类的页面：外观、终端。每个页面返回从上到下的分组标题和分组。

use std::ops::RangeInclusive;
use std::rc::Rc;

use gpui_kit::component::{
    Sizable as _,
    input::{InputEvent, InputState, NumberInput},
    switch::Switch,
};
use gpui_kit::*;

use qishell_terminal::TerminalFontPreview;

use super::controls::{dropdown, group, row, section_title, segmented};
use super::dialog::SettingsDialog;
use super::terminal_themes::terminal_theme_grid;
use super::{
    AppSettings, Appearance, DEFAULT_INTERFACE_FONT_SIZE, DEFAULT_TERMINAL_FONT_FAMILY,
    INTERFACE_FONT_SIZE_RANGE, RightClick, SettingsStore, TERMINAL_FONT_SIZE_RANGE,
    TERMINAL_LINE_HEIGHT_RANGE,
};
use crate::i18n::t;
use crate::theme::Tokens;

/// 改一项设置，写进存储（存储再写文件、通知窗口壳去应用）。
fn write(store: &Entity<SettingsStore>, cx: &mut App, change: impl FnOnce(&mut AppSettings)) {
    store.update(cx, |store, cx| store.update(change, cx));
}

/// 应用外观的三个选项，按分段按钮从左到右。
const APPEARANCES: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

/// 外观：常规（应用外观、界面字号）、终端主题。
pub(super) fn appearance(
    dialog: &SettingsDialog,
    tokens: &Tokens,
    _: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let store = dialog.store.clone();
    let settings = store.read(cx).settings();

    let mode = {
        let store = store.clone();
        segmented(
            "appearance",
            vec![
                t!("settings.appearance.system"),
                t!("settings.appearance.light"),
                t!("settings.appearance.dark"),
            ],
            APPEARANCES
                .iter()
                .position(|appearance| *appearance == settings.appearance)
                .unwrap_or_default(),
            Rc::new(move |index, _, cx| {
                write(&store, cx, |settings| {
                    settings.appearance = APPEARANCES[index]
                })
            }),
            tokens,
        )
    };

    let sizes: Vec<f32> = interface_font_sizes().collect();
    let font_size = {
        let store = store.clone();
        let chosen = sizes
            .iter()
            .position(|size| *size == settings.interface.font_size);
        let sizes_for_menu = sizes.clone();
        dropdown(
            "interface-font-size",
            number_text(settings.interface.font_size).into(),
            sizes.iter().map(|size| number_text(*size).into()).collect(),
            chosen,
            Rc::new(move |index, _, cx| {
                let size = sizes_for_menu[index];
                write(&store, cx, |settings| settings.interface.font_size = size)
            }),
            px(90.),
        )
    };

    vec![
        section_title(t!("settings.appearance.general"), None, tokens).into_any_element(),
        group(
            [
                row(
                    t!("settings.appearance.mode"),
                    Some(t!("settings.appearance.mode_description")),
                    mode,
                    tokens,
                ),
                row(
                    t!("settings.appearance.font_size"),
                    Some(t!(
                        "settings.appearance.font_size_description",
                        size = number_text(DEFAULT_INTERFACE_FONT_SIZE)
                    )),
                    font_size,
                    tokens,
                ),
            ],
            tokens,
        )
        .into_any_element(),
        section_title(
            t!("settings.appearance.terminal_theme"),
            Some(t!("settings.appearance.terminal_theme_description")),
            tokens,
        )
        .into_any_element(),
        group(
            [terminal_theme_grid(&store, tokens, cx).into_any_element()],
            tokens,
        )
        .into_any_element(),
    ]
}

/// 界面字号能选的值：范围内的每个整数像素。
fn interface_font_sizes() -> impl Iterator<Item = f32> {
    let (start, end) = (
        *INTERFACE_FONT_SIZE_RANGE.start() as u32,
        *INTERFACE_FONT_SIZE_RANGE.end() as u32,
    );
    (start..=end).map(|size| size as f32)
}

/// 数字的显示：不带多余的 0（`14`、`11.6`、`1.45`）。
fn number_text(value: f32) -> String {
    let rounded = (value * 100.).round() / 100.;
    format!("{rounded}")
}

/// 字体预览里的几行：字母、容易混的字符、中文（一个字占两格）、制表符和一行命令。
const PREVIEW_LINES: &[&str] = &[
    "qi@web-01:~$ ls -la",
    "0O 1lI {}[]() ,.;:'\"`",
    "中文对齐：你好，世界", // i18n: keep（看两格宽的字怎么对齐）
    "┌─┬─┐ ▁▂▃▅▇ ←↑→↓",
];

/// 右键行为的四个选项，按菜单从上到下。
const RIGHT_CLICKS: [(RightClick, &str); 4] = [
    (RightClick::Menu, "settings.right_click.menu"),
    (RightClick::Paste, "settings.right_click.paste"),
    (
        RightClick::CopyOrPaste,
        "settings.right_click.copy_or_paste",
    ),
    (RightClick::Nothing, "settings.right_click.nothing"),
];

/// 终端：字体（字体、字号、行高、预览）、交互（选中即复制、右键行为）、本地终端（shell）。
pub(super) fn terminal(
    dialog: &SettingsDialog,
    tokens: &Tokens,
    _: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let store = dialog.store.clone();
    let settings = store.read(cx).settings();

    // 默认字体随包附带，总在最前；其余是本机装了的等宽字体。
    let families: Vec<SharedString> =
        std::iter::once(SharedString::from(DEFAULT_TERMINAL_FONT_FAMILY))
            .chain(
                dialog
                    .font_families
                    .iter()
                    .filter(|family| family.as_ref() != DEFAULT_TERMINAL_FONT_FAMILY)
                    .cloned(),
            )
            .collect();
    let family = settings.terminal_font.family().to_string();
    let family_field = {
        let store = store.clone();
        let chosen = families
            .iter()
            .position(|candidate| candidate.as_ref() == family);
        let families_for_menu = families.clone();
        dropdown(
            "terminal-font-family",
            family.clone().into(),
            families,
            chosen,
            Rc::new(move |index, _, cx| {
                let family = families_for_menu[index].to_string();
                write(&store, cx, |settings| {
                    settings.terminal_font.family =
                        (family != DEFAULT_TERMINAL_FONT_FAMILY).then_some(family)
                })
            }),
            px(220.),
        )
    };

    let copy_on_select = {
        let store = store.clone();
        Switch::new("copy-on-select")
            .checked(settings.terminal_interaction.copy_on_select)
            .accessibility_label(t!("settings.terminal.copy_on_select"))
            .on_click(move |checked: &bool, _, cx| {
                let checked = *checked;
                write(&store, cx, |settings| {
                    settings.terminal_interaction.copy_on_select = checked
                })
            })
    };

    let right_click = {
        let store = store.clone();
        let chosen = RIGHT_CLICKS
            .iter()
            .position(|(action, _)| *action == settings.terminal_interaction.right_click);
        dropdown(
            "right-click",
            t!(RIGHT_CLICKS[chosen.unwrap_or_default()].1),
            RIGHT_CLICKS.iter().map(|(_, title)| t!(*title)).collect(),
            chosen,
            Rc::new(move |index, _, cx| {
                write(&store, cx, |settings| {
                    settings.terminal_interaction.right_click = RIGHT_CLICKS[index].0
                })
            }),
            px(260.),
        )
    };

    let range =
        |range: &RangeInclusive<f32>| (number_text(*range.start()), number_text(*range.end()));
    let (size_min, size_max) = range(&TERMINAL_FONT_SIZE_RANGE);
    let (line_min, line_max) = range(&TERMINAL_LINE_HEIGHT_RANGE);

    vec![
        section_title(t!("settings.terminal.font"), None, tokens).into_any_element(),
        group(
            [
                row(
                    t!("settings.terminal.family"),
                    Some(t!("settings.terminal.family_description")),
                    family_field,
                    tokens,
                ),
                row(
                    t!("settings.terminal.size"),
                    Some(t!(
                        "settings.terminal.size_description",
                        min = size_min,
                        max = size_max
                    )),
                    number_input("terminal-font-size", &dialog.terminal_font_size),
                    tokens,
                ),
                row(
                    t!("settings.terminal.line_height"),
                    Some(t!(
                        "settings.terminal.line_height_description",
                        min = line_min,
                        max = line_max
                    )),
                    number_input("terminal-line-height", &dialog.terminal_line_height),
                    tokens,
                ),
                div()
                    .px(px(14.))
                    .pb(px(12.))
                    .pt(px(2.))
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(9.))
                            .bg(qishell_terminal::TerminalColors::current(cx).background())
                            .overflow_hidden()
                            .child(TerminalFontPreview::new(
                                "terminal-font-preview",
                                PREVIEW_LINES,
                            )),
                    )
                    .into_any_element(),
            ],
            tokens,
        )
        .into_any_element(),
        section_title(t!("settings.terminal.interaction"), None, tokens).into_any_element(),
        group(
            [
                row(
                    t!("settings.terminal.copy_on_select"),
                    Some(t!("settings.terminal.copy_on_select_description")),
                    copy_on_select,
                    tokens,
                ),
                row(
                    t!("settings.terminal.right_click"),
                    None,
                    right_click,
                    tokens,
                ),
            ],
            tokens,
        )
        .into_any_element(),
        section_title(t!("settings.local_terminal.title"), None, tokens).into_any_element(),
        group(
            [row(
                t!("settings.local_terminal.shell"),
                Some(t!("settings.local_terminal.shell_description")),
                dialog.local_shell.clone(),
                tokens,
            )],
            tokens,
        )
        .into_any_element(),
    ]
}

/// 数字输入框，套在带 id 的容器里：测试点容器的中间就点到了输入框。
fn number_input(id: &'static str, state: &Entity<InputState>) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .w(px(120.))
        .child(NumberInput::new(state).small())
}

/// 一个数字设置的输入框，和它的订阅：输入的数在范围内就写进设置（打字打到一半的不写），
/// 点 ± 按 `step` 加减；失焦或回车时显示回设置里的值；设置在别处变了也跟着显示。
struct NumberField {
    read: fn(&AppSettings) -> f32,
    write: fn(&mut AppSettings, f32),
    range: RangeInclusive<f32>,
    step: f32,
}

impl NumberField {
    fn build(
        self,
        store: &Entity<SettingsStore>,
        window: &mut Window,
        cx: &mut Context<SettingsDialog>,
    ) -> (Entity<InputState>, Vec<Subscription>) {
        let NumberField {
            read,
            write: store_value,
            range,
            step,
        } = self;
        let value = read(&store.read(cx).settings());
        // ± 由输入框自己按 `step` 加减、收进范围，再发 `Change`，和打字走同一条路。
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(number_text(value))
                .step(f64::from(step))
                .min(f64::from(*range.start()))
                .max(f64::from(*range.end()))
        });
        let set = {
            let store = store.clone();
            move |value: f32, cx: &mut App| {
                write(&store, cx, |settings| store_value(settings, value));
            }
        };
        let show_stored = {
            let store = store.clone();
            move |input: &Entity<InputState>, window: &mut Window, cx: &mut App| {
                let stored = read(&store.read(cx).settings());
                let shown = input.read(cx).value().trim().parse::<f32>().ok();
                if shown != Some(stored) {
                    input.update(cx, |input, cx| {
                        input.set_value(number_text(stored), window, cx)
                    });
                }
            }
        };
        let subscriptions = vec![
            cx.subscribe_in(&input, window, {
                move |_, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        if let Ok(value) = input.read(cx).value().trim().parse::<f32>()
                            && range.contains(&value)
                        {
                            set(value, cx);
                        }
                    }
                    InputEvent::Blur | InputEvent::PressEnter { .. } => {
                        show_stored(input, window, cx)
                    }
                    InputEvent::Focus => {}
                }
            }),
            // 设置在别处变了（比如在终端里按 Ctrl+=）：没在输入时显示新值。
            cx.observe_in(store, window, {
                let input = input.clone();
                move |_, store, window, cx| {
                    if input.read(cx).focus_handle(cx).is_focused(window) {
                        return;
                    }
                    let stored = read(&store.read(cx).settings());
                    let shown = input.read(cx).value().trim().parse::<f32>().ok();
                    if shown != Some(stored) {
                        input.update(cx, |input, cx| {
                            input.set_value(number_text(stored), window, cx)
                        });
                    }
                }
            }),
        ];
        (input, subscriptions)
    }
}

/// 终端字号的输入框：每按一下 ± 变 0.5。
pub(super) fn terminal_font_size_field(
    store: &Entity<SettingsStore>,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> (Entity<InputState>, Vec<Subscription>) {
    NumberField {
        read: |settings| settings.terminal_font.size,
        write: |settings, size| settings.terminal_font.size = size,
        range: TERMINAL_FONT_SIZE_RANGE,
        step: 0.5,
    }
    .build(store, window, cx)
}

/// 终端行高的输入框：每按一下 ± 变 0.05。
pub(super) fn terminal_line_height_field(
    store: &Entity<SettingsStore>,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> (Entity<InputState>, Vec<Subscription>) {
    NumberField {
        read: |settings| settings.terminal_font.line_height,
        write: |settings, line_height| settings.terminal_font.line_height = line_height,
        range: TERMINAL_LINE_HEIGHT_RANGE,
        step: 0.05,
    }
    .build(store, window, cx)
}

#[cfg(test)]
mod tests {
    use super::{interface_font_sizes, number_text};

    #[test]
    fn numbers_show_without_trailing_zeros() {
        assert_eq!(number_text(14.), "14");
        assert_eq!(number_text(11.6), "11.6");
        assert_eq!(number_text(1.45), "1.45");
        assert_eq!(number_text(1.45 + 0.05), "1.5");
    }

    #[test]
    fn the_interface_font_size_offers_every_pixel_from_12_to_20() {
        assert_eq!(
            interface_font_sizes().collect::<Vec<_>>(),
            [12., 13., 14., 15., 16., 17., 18., 19., 20.]
        );
    }
}
