//! 设置标签各类的页面：外观、终端（26 号「设置」）。每个页面返回从上到下的分组标题和设置行。

use std::rc::Rc;

use gpui_kit::*;

use micaterm_terminal::TerminalFontPreview;

use super::controls::{
    CONTROL_HEIGHT, dropdown, field_box, note_with_keys, reset_if, row, row_with_note, section,
    segmented_with_height, setting_switch,
};
use super::tab::SettingsTab;
use super::terminal_themes::terminal_theme_grid;
use super::{
    AppSettings, Appearance, DEFAULT_TERMINAL_FONT_FAMILY, RightClick, SettingsStore,
    TERMINAL_FONT_SIZE_RANGE, TERMINAL_LINE_HEIGHT_RANGE,
};
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale};

/// 改一项设置，写进存储（存储再写文件、通知窗口壳去应用）。
pub(super) fn write(
    store: &Entity<SettingsStore>,
    cx: &mut App,
    change: impl FnOnce(&mut AppSettings),
) {
    store.update(cx, |store, cx| store.update(change, cx));
}

/// 应用外观的三个选项，按切换按钮从左到右。
const APPEARANCES: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

/// 数字下拉框的宽（26 号 `.stg-dd`）。
const NUMBER_WIDTH: Pixels = px(96.);
/// 长一些的下拉框、输入框的宽（字体、右键行为、Shell、本地目录）。
pub(super) const WIDE_WIDTH: Pixels = px(240.);

/// 外观：常规（应用外观、Mica 背景）、终端主题。
pub(super) fn appearance(
    tab: &SettingsTab,
    tokens: &Tokens,
    _: &mut Window,
    cx: &mut Context<SettingsTab>,
) -> Vec<AnyElement> {
    let store = tab.store.clone();
    let settings = store.read(cx).settings();
    let default = AppSettings::default();

    let mode = {
        let store = store.clone();
        segmented_with_height(
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
            CONTROL_HEIGHT,
            tokens,
        )
    };

    // Mica 背景（工单 26）：系统不支持时开关照样能开，只是不起作用。
    let mica = setting_switch(
        "mica",
        t!("settings.appearance.mica"),
        settings.interface.mica,
        &store,
        |settings, checked| settings.interface.mica = checked,
    );

    vec![
        section(t!("settings.appearance.general"), None, true, tokens),
        row(
            t!("settings.appearance.mode"),
            Some(t!("settings.appearance.mode_description")),
            reset_if(
                settings.appearance != default.appearance,
                "appearance",
                &store,
                move |settings| settings.appearance = AppSettings::default().appearance,
                tokens,
            ),
            mode,
            tokens,
        ),
        row(
            t!("settings.appearance.mica"),
            Some(t!("settings.appearance.mica_description")),
            reset_if(
                settings.interface.mica != default.interface.mica,
                "mica",
                &store,
                move |settings| settings.interface.mica = AppSettings::default().interface.mica,
                tokens,
            ),
            mica,
            tokens,
        ),
        section(
            t!("settings.appearance.terminal_theme"),
            Some(t!("settings.appearance.terminal_theme_description")),
            false,
            tokens,
        ),
        terminal_theme_grid(&store, tokens, cx).into_any_element(),
    ]
}

/// 终端字号能选的值：10–28 的每个整数像素。
fn terminal_font_sizes() -> impl Iterator<Item = f32> {
    let (start, end) = (
        *TERMINAL_FONT_SIZE_RANGE.start() as u32,
        *TERMINAL_FONT_SIZE_RANGE.end() as u32,
    );
    (start..=end).map(|size| size as f32)
}

/// 终端行高能选的值：1.0–2.0，每档 0.1。
fn terminal_line_heights() -> impl Iterator<Item = f32> {
    let (start, end) = (
        (*TERMINAL_LINE_HEIGHT_RANGE.start() * 10.).round() as u32,
        (*TERMINAL_LINE_HEIGHT_RANGE.end() * 10.).round() as u32,
    );
    (start..=end).map(|tenths| tenths as f32 / 10.)
}

/// 数字的显示：不带多余的 0（`14`、`11.6`、`1.45`）。
fn number_text(value: f32) -> String {
    let rounded = (value * 100.).round() / 100.;
    format!("{rounded}")
}

/// 行高的显示：至少一位小数（`1.0`、`1.6`、`1.65`）。
fn line_height_text(value: f32) -> String {
    let text = number_text(value);
    if text.contains('.') {
        text
    } else {
        format!("{text}.0")
    }
}

/// 数字下拉框：`values` 里的每个数一项；设置里的数不在里面（比如在终端里按 Ctrl+= 调出来的半档）时
/// 框上照样显示它，菜单里没有打勾的。
fn number_dropdown(
    id: &'static str,
    current: f32,
    values: Vec<f32>,
    store: &Entity<SettingsStore>,
    set: fn(&mut AppSettings, f32),
    tokens: &Tokens,
) -> impl IntoElement + use<> {
    number_dropdown_shown(id, current, values, number_text, store, set, tokens)
}

fn number_dropdown_shown(
    id: &'static str,
    current: f32,
    values: Vec<f32>,
    show: fn(f32) -> String,
    store: &Entity<SettingsStore>,
    set: fn(&mut AppSettings, f32),
    tokens: &Tokens,
) -> impl IntoElement + use<> {
    let store = store.clone();
    let chosen = values
        .iter()
        .position(|value| (value - current).abs() < 0.001);
    let labels = values.iter().map(|value| show(*value).into()).collect();
    dropdown(
        id,
        show(current).into(),
        labels,
        chosen,
        Rc::new(move |index, _, cx| {
            let value = values[index];
            write(&store, cx, |settings| set(settings, value))
        }),
        NUMBER_WIDTH,
        tokens,
    )
}

/// 字体预览那一行（26 号 `.stg-pv`）：命令、容易混的字符、中文（一个字占两格）、制表符。
const PREVIEW_LINES: &[&str] = &[
    "qi@web-01:~$ ls -la   0O 1lI {}[]()   中文对齐   ┌─┬─┐ ▁▂▃▅▇", // i18n: keep（看两格宽的字怎么对齐）
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

/// 终端：字体（字体、字号、行高、预览）、交互（选中即复制、右键行为）、本地终端（Shell）。
pub(super) fn terminal(
    tab: &SettingsTab,
    tokens: &Tokens,
    window: &mut Window,
    cx: &mut Context<SettingsTab>,
) -> Vec<AnyElement> {
    let store = tab.store.clone();
    let settings = store.read(cx).settings();
    let default = AppSettings::default();

    // 默认字体总在最前；其余是本机装了的等宽字体。
    let families: Vec<SharedString> =
        std::iter::once(SharedString::from(DEFAULT_TERMINAL_FONT_FAMILY))
            .chain(
                tab.font_families
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
            WIDE_WIDTH,
            tokens,
        )
    };

    let font_size = number_dropdown(
        "terminal-font-size",
        settings.terminal_font.size,
        terminal_font_sizes().collect(),
        &store,
        |settings, size| settings.terminal_font.size = size,
        tokens,
    );
    let line_height = number_dropdown_shown(
        "terminal-line-height",
        settings.terminal_font.line_height,
        terminal_line_heights().collect(),
        line_height_text,
        &store,
        |settings, line_height| settings.terminal_font.line_height = line_height,
        tokens,
    );

    let copy_on_select = setting_switch(
        "copy-on-select",
        t!("settings.terminal.copy_on_select"),
        settings.terminal_interaction.copy_on_select,
        &store,
        |settings, checked| settings.terminal_interaction.copy_on_select = checked,
    );

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
            WIDE_WIDTH,
            tokens,
        )
    };

    let preview = field_box(div(), tokens)
        .h_auto()
        .mt(px(12.))
        .mb(px(2.))
        .px(px(12.))
        .py(px(10.))
        .overflow_hidden()
        .child(TerminalFontPreview::new(
            "terminal-font-preview",
            PREVIEW_LINES,
        ))
        .into_any_element();

    let (size_min, size_max) = (
        number_text(*TERMINAL_FONT_SIZE_RANGE.start()),
        number_text(*TERMINAL_FONT_SIZE_RANGE.end()),
    );
    let (line_min, line_max) = (
        line_height_text(*TERMINAL_LINE_HEIGHT_RANGE.start()),
        line_height_text(*TERMINAL_LINE_HEIGHT_RANGE.end()),
    );
    let font = settings.terminal_font.clone();
    let interaction = settings.terminal_interaction;

    vec![
        section(t!("settings.terminal.font"), None, true, tokens),
        row(
            t!("settings.terminal.family"),
            Some(t!("settings.terminal.family_description")),
            reset_if(
                font.family != default.terminal_font.family,
                "terminal-font-family",
                &store,
                |settings| settings.terminal_font.family = None,
                tokens,
            ),
            family_field,
            tokens,
        ),
        row_with_note(
            t!("settings.terminal.size"),
            Some(note_with_keys(
                &t!(
                    "settings.terminal.size_description",
                    min = size_min,
                    max = size_max
                ),
                TypeScale::current(cx),
                tokens,
                window,
            )),
            reset_if(
                font.size != default.terminal_font.size,
                "terminal-font-size",
                &store,
                move |settings| {
                    settings.terminal_font.size = AppSettings::default().terminal_font.size
                },
                tokens,
            ),
            font_size,
            tokens,
        ),
        row(
            t!("settings.terminal.line_height"),
            Some(t!(
                "settings.terminal.line_height_description",
                min = line_min,
                max = line_max
            )),
            reset_if(
                font.line_height != default.terminal_font.line_height,
                "terminal-line-height",
                &store,
                move |settings| {
                    settings.terminal_font.line_height =
                        AppSettings::default().terminal_font.line_height
                },
                tokens,
            ),
            line_height,
            tokens,
        ),
        preview,
        section(t!("settings.terminal.interaction"), None, false, tokens),
        row(
            t!("settings.terminal.copy_on_select"),
            Some(t!("settings.terminal.copy_on_select_description")),
            reset_if(
                interaction.copy_on_select != default.terminal_interaction.copy_on_select,
                "copy-on-select",
                &store,
                move |settings| {
                    settings.terminal_interaction.copy_on_select =
                        AppSettings::default().terminal_interaction.copy_on_select
                },
                tokens,
            ),
            copy_on_select,
            tokens,
        ),
        row(
            t!("settings.terminal.right_click"),
            Some(t!("settings.terminal.right_click_description")),
            reset_if(
                interaction.right_click != default.terminal_interaction.right_click,
                "right-click",
                &store,
                move |settings| {
                    settings.terminal_interaction.right_click =
                        AppSettings::default().terminal_interaction.right_click
                },
                tokens,
            ),
            right_click,
            tokens,
        ),
        section(t!("settings.local_terminal.title"), None, false, tokens),
        row(
            t!("settings.local_terminal.shell"),
            Some(t!("settings.local_terminal.shell_description")),
            reset_if(
                settings.local_terminal.shell.is_some(),
                "local-shell",
                &store,
                |settings| settings.local_terminal.shell = None,
                tokens,
            ),
            tab.local_shell.clone(),
            tokens,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::{line_height_text, number_text, terminal_font_sizes, terminal_line_heights};

    #[test]
    fn numbers_show_without_trailing_zeros() {
        assert_eq!(number_text(14.), "14");
        assert_eq!(number_text(11.6), "11.6");
        assert_eq!(number_text(1.45), "1.45");
        assert_eq!(number_text(1.45 + 0.05), "1.5");
    }

    #[test]
    fn line_heights_keep_one_decimal() {
        assert_eq!(line_height_text(1.), "1.0");
        assert_eq!(line_height_text(1.6), "1.6");
        assert_eq!(line_height_text(1.65), "1.65");
    }

    #[test]
    fn the_terminal_font_offers_whole_sizes_and_tenths_of_line_height() {
        let sizes: Vec<f32> = terminal_font_sizes().collect();
        assert_eq!(
            (sizes[0], sizes[sizes.len() - 1], sizes.len()),
            (10., 28., 19)
        );
        let heights: Vec<f32> = terminal_line_heights().collect();
        assert_eq!(heights.len(), 11);
        assert_eq!((heights[0], heights[6], heights[10]), (1., 1.6, 2.));
    }
}
