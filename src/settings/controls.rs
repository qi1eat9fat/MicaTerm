//! 设置标签的通用件（照空壳的设置页）：页标题（18/600）、分组标题（等宽小字 + 下面一条细线）、
//! 一行设置（左边标题 + 一行说明，右边控件，下面一条细线）、改过时标题后面的「恢复默认」、
//! 下拉框、输入框的样子、开关、切换按钮组。

use std::rc::Rc;

use gpui_kit::component::{
    Icon, Sizable as _,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::pages::write;
use super::{AppSettings, SettingsStore};
use crate::app::CatalogIcon;
use crate::i18n::t;
use crate::theme::{
    Tokens, TypeScale, WEIGHT_NORMAL, WEIGHT_SEMIBOLD, size, space,
};
use crate::ui::icon_button::TextTooltip;
use crate::ui::toggle::Toggle;

/// 说明、分组标题这些一行字的行高。
const LINE_HEIGHT: Pixels = px(18.);
/// 设置行里标题和说明那一栏至少多宽：再窄控件就换到下一行。
const TEXT_MIN: Pixels = px(200.);
/// 下拉框、输入框、切换按钮组的高（统一 `CONTROL_LG = 28px`）。
pub const CONTROL_HEIGHT: Pixels = size::CONTROL_LG;

/// 页标题：18 号 Semibold、下面留 6。
pub fn page_title(title: SharedString, scale: TypeScale, tokens: &Tokens) -> AnyElement {
    div()
        .id("settings-title")
        .test_support()
        .aria_label(title.clone())
        .flex()
        .items_center()
        .h(px(24.))
        .mb(space::SPACE_6)
        .text_size(scale.page_title)
        .font_weight(WEIGHT_SEMIBOLD)
        .text_color(tokens.text)
        .child(title)
        .into_any_element()
}

/// 分组标题：11px Regular 界面字体（不再用等宽），下面一条细线。
pub fn section(
    title: SharedString,
    note: Option<SharedString>,
    first: bool,
    tokens: &Tokens,
    scale: TypeScale,
) -> AnyElement {
    let faint = tokens.faint;
    div()
        .flex()
        .gap(space::SPACE_8)
        .when(first, |section| section.pt(space::SPACE_8))
        .when(!first, |section| section.mt(space::SPACE_16))
        .pb(space::SPACE_8)
        .border_b(size::LINE)
        .border_color(tokens.line)
        .text_size(scale.sm)
        .font_weight(WEIGHT_NORMAL)
        .text_color(tokens.text2)
        .line_height(px(16.))
        .child(div().flex_none().child(title))
        .children(note.map(|note| div().min_w_0().truncate().text_color(faint).child(note)))
        .into_any_element()
}

/// 一行设置：左边标题（正文色）和一句说明（淡色），右边控件；上下留 12、至少 56 高，下面一条细线。
/// `reset` 是标题后面的「恢复默认」，只在和默认不同时给。
pub fn row(
    title: impl IntoElement,
    description: Option<SharedString>,
    reset: Option<AnyElement>,
    control: impl IntoElement,
    tokens: &Tokens,
) -> AnyElement {
    let description = description.map(|text| note(tokens).child(text).into_any_element());
    row_with_note(title, description, reset, control, tokens)
}

/// 一行设置，说明那一行放任意内容（数据目录的路径要带 id）。
pub fn row_with_note(
    title: impl IntoElement,
    note: Option<AnyElement>,
    reset: Option<AnyElement>,
    control: impl IntoElement,
    tokens: &Tokens,
) -> AnyElement {
    // 窗口窄时控件换到标题和说明下面（用户 2026-11-16）：说明那一栏至少 TEXT_MIN 宽，
    // 放不下控件就整行折开，不再把说明挤成一个字一行。
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(20.))
        .gap_y(px(8.))
        .min_h(px(56.))
        .py(space::SPACE_12)
        .border_b(size::LINE)
        .border_color(tokens.line)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow(1.)
                .flex_shrink(1.)
                .flex_basis(TEXT_MIN)
                .min_w(TEXT_MIN)
                .gap(space::SPACE_4)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::SPACE_4)
                        .h(LINE_HEIGHT)
                        .text_size(TypeScale::from_base(crate::theme::INTERFACE_FONT_SIZE).emphasis)
                        .font_weight(WEIGHT_NORMAL)
                        .text_color(tokens.text)
                        .child(div().line_height(LINE_HEIGHT).child(title))
                        .children(reset),
                )
                .children(note),
        )
        .child(div().flex_none().child(control))
        .into_any_element()
}

/// 设置标题下面那句说明的样子：次要色（前景 65%），行高 18。
pub fn note(tokens: &Tokens) -> Div {
    div().text_color(tokens.text2).line_height(LINE_HEIGHT)
}

/// 一句说明，里面 `Ctrl+` 开头的词画成快捷键（26 号 `.kbd`）：界面字体、和说明同号，
/// 和文字按基线对齐再上移 0.5（用户定的全局规则）。说明只有一行，按词拼成一排。
pub fn note_with_keys(text: &str, scale: TypeScale, tokens: &Tokens) -> AnyElement {
    let mut parts: Vec<(String, bool)> = Vec::new();
    for (index, word) in text.split(' ').enumerate() {
        let key = word.starts_with("Ctrl+");
        let space = if index > 0 { " " } else { "" };
        match parts.last_mut() {
            // 空格跟着前面的文字；快捷键前后的空格各归旁边的文字。
            Some((last, false)) if !key => {
                last.push_str(space);
                last.push_str(word);
            }
            Some((last, false)) => {
                last.push_str(space);
                parts.push((word.to_string(), true));
            }
            Some((_, true)) => parts.push((format!("{space}{word}"), key)),
            None => parts.push((word.to_string(), key)),
        }
    }
    note(tokens)
        .flex()
        .items_center()
        .whitespace_nowrap()
        .children(parts.into_iter().map(|(part, key)| {
            div()
                .line_height(LINE_HEIGHT)
                .when(key, |part| part.text_size(scale.body))
                .child(part)
        }))
        .into_any_element()
}

/// 页底的一句注释（26 号 `.stg-note`：设置 › 安全写保险库文件）。
pub fn page_note(text: SharedString, tokens: &Tokens) -> AnyElement {
    note(tokens).mt(space::SPACE_12).child(text).into_any_element()
}

/// 标题后面的「恢复默认」（26 号 `.stg-row .rs`，照 Zed 的 Undo 小图标按钮）：18 见方、圆角 4，
/// 12 的撤销箭头；点了用 `reset` 把这一项改回默认。元素 id 是 `<id>-reset`。
pub fn reset_button(
    id: &'static str,
    store: &Entity<SettingsStore>,
    reset: impl Fn(&mut AppSettings) + 'static,
    tokens: &Tokens,
) -> AnyElement {
    let store = store.clone();
    let hover = tokens.hover;
    let text = tokens.text;
    let label = t!("settings.reset");
    div()
        .id(ElementId::Name(format!("{id}-reset").into()))
        .test_support()
        .aria_label(label.clone())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::CONTROL_SM)
        .rounded(size::RADIUS_SM)
        .text_color(tokens.st_dim)
        .cursor_pointer()
        .hover(move |style| style.bg(hover).text_color(text))
        .tooltip(move |_, cx| TextTooltip::view(label.clone(), cx))
        .on_click(move |_, _, cx| write(&store, cx, &reset))
        .child(Icon::new(CatalogIcon::Undo2).size(size::ICON_SM))
        .into_any_element()
}

/// 和默认不同时的「恢复默认」：`changed` 为假时没有。
pub fn reset_if(
    changed: bool,
    id: &'static str,
    store: &Entity<SettingsStore>,
    reset: impl Fn(&mut AppSettings) + 'static,
    tokens: &Tokens,
) -> Option<AnyElement> {
    changed.then(|| reset_button(id, store, reset, tokens))
}

/// 下拉框、输入框共用的框：高 28、左右 10、圆角 6、细边、输入框底。
pub fn field_box<E: Styled>(element: E, tokens: &Tokens) -> E {
    crate::ui::input::frame(element, tokens, false, false)
        .px(crate::ui::input::INSET)
        .text_color(tokens.text)
}

/// 单行输入框：高 28、圆角 6、细边、输入框底，聚焦时品牌色边。
pub fn field_input(input: Input, focused: bool, tokens: &Tokens, scale: TypeScale) -> Input {
    let input = input
        .small()
        .focus_bordered(false)
        .text_size(scale.sm)
        .line_height(px(16.))
        .rounded(size::RADIUS_MD)
        .bg(tokens.input)
        .border_color(crate::ui::input::border_color(tokens, false, focused));
    Styled::h(input, size::INPUT)
}

/// 设置里下拉菜单每一项的元素 id 是 `(MENU_OPTION, 第几项)`，无障碍名是它的文字（测试靠它）。
pub const MENU_OPTION: &str = "menu-option";

/// 设置里下拉菜单一项的高。
const MENU_OPTION_HEIGHT: Pixels = size::MENU_ROW;

/// 设置里下拉菜单的一项：字是界面字号。gpui-component 的菜单项写死了 `text_sm`（0.875rem，
/// Windows 上约 10.5），比设置里的字小一圈，所以自己画文字；`checked` 的勾照旧由菜单画。
/// `index` 是它在选项里排第几（不算分隔线），给元素 id 用。
pub fn menu_option(
    index: usize,
    label: SharedString,
    checked: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        div()
            .id((MENU_OPTION, index))
            .test_support()
            .aria_label(label.clone())
            .flex()
            .items_center()
            .h(MENU_OPTION_HEIGHT)
            .text_size(TypeScale::current(cx).sm)
            .child(label.clone())
    })
    .checked(checked)
    .on_click(move |_, window, cx| on_click(window, cx))
}

/// 一项开关设置（空壳的开关 36×20，同尺寸下约 28×16；开着是强调色底）：拨动时用 `set` 把新值写进设置。
/// `label` 是无障碍名，测试也靠它。点过之后开关拿着焦点，gpui-component 会在胶囊外再套一圈焦点框；
/// 设置里的控件都不要这圈框（和输入框一样），所以关掉。
pub fn setting_switch(
    id: impl Into<ElementId>,
    label: SharedString,
    checked: bool,
    store: &Entity<SettingsStore>,
    set: impl Fn(&mut AppSettings, bool) + 'static,
) -> Toggle {
    let store = store.clone();
    Toggle::new(id, label, checked).on_click(move |checked, _, cx| {
        write(&store, cx, |settings| set(settings, checked))
    })
}

/// 选中某一项时做的事。
pub type OnChoose = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// 切换按钮组（主机表单），高 28。
pub fn segmented(
    id: &'static str,
    options: Vec<SharedString>,
    selected: usize,
    on_choose: OnChoose,
    tokens: &Tokens,
    scale: TypeScale,
) -> Div {
    segmented_with_height(id, options, selected, on_choose, size::INPUT, tokens, scale)
}

/// 切换按钮组（空壳的分段控件）：外圈前景 4% 的底、细边、圆角 8，里外各留 2、项之间空 2；
/// 每一项圆角 6，选中的是强调色底、强调色上的字，其余次要色。每一项是 `(id, index)`，
/// 选中状态写在无障碍属性里，测试靠它断言。`height` 连外圈的线算在内。
pub fn segmented_with_height(
    id: &'static str,
    options: Vec<SharedString>,
    selected: usize,
    on_choose: OnChoose,
    height: Pixels,
    tokens: &Tokens,
    scale: TypeScale,
) -> Div {
    let text = tokens.text;
    let hover = tokens.sel;
    let item_height = if height == size::INPUT {
        size::SEGMENT_ITEM
    } else {
        height - px(6.)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .self_start()
        .gap(space::SPACE_2)
        .p(space::SPACE_2)
        .rounded(size::RADIUS_LG)
        .bg(tokens.tab_active)
        .border_1()
        .border_color(tokens.line)
        .children(options.into_iter().enumerate().map(|(index, label)| {
            let on_choose = on_choose.clone();
            let chosen = index == selected;
            div()
                .id((id, index))
                .test_support()
                .role(Role::RadioButton)
                .aria_label(label.clone())
                .aria_selected(chosen)
                .flex()
                .items_center()
                .h(item_height)
                .px(space::SPACE_10)
                .rounded(size::RADIUS_MD)
                .text_size(scale.sm)
                .line_height(px(16.))
                .cursor_pointer()
                .map(|button| {
                    if chosen {
                        button
                            .bg(tokens.brand)
                            .text_color(tokens.on_brand)
                            .font_weight(WEIGHT_NORMAL)
                    } else {
                        button
                            .text_color(tokens.text2)
                            .hover(move |style| style.bg(hover).text_color(text))
                    }
                })
                .on_click(move |_, window, cx| on_choose(index, window, cx))
                .child(label)
        }))
}

/// 下拉框的按钮：一个框，左边当前值，右边下箭头；点开是 gpui-component 的弹出菜单。
#[derive(IntoElement)]
pub struct SelectBox {
    base: Stateful<Div>,
    selected: bool,
}

impl Styled for SelectBox {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for SelectBox {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl gpui_kit::component::Selectable for SelectBox {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl DropdownMenu for SelectBox {}

impl RenderOnce for SelectBox {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base.test_support()
    }
}

/// 下拉框（26 号 `.stg-dd`）：框上是当前值，点开是选项菜单，当前值打勾。
pub fn dropdown(
    id: &'static str,
    current: SharedString,
    options: Vec<SharedString>,
    selected: Option<usize>,
    on_choose: OnChoose,
    width: Pixels,
    tokens: &Tokens,
    scale: TypeScale,
) -> impl IntoElement + use<> {
    let base = field_box(div().id(id), tokens)
        .text_size(scale.sm)
        .line_height(px(16.))
        .aria_label(current.clone())
        .flex()
        .items_center()
        .justify_between()
        .gap(space::SPACE_6)
        .w(width)
        .cursor_pointer()
        .child(div().min_w_0().truncate().child(current))
        .child(
            Icon::new(CatalogIcon::ChevronDown)
                .size(px(14.))
                .flex_none()
                .text_color(tokens.st_dim),
        );
    SelectBox {
        base,
        selected: false,
    }
    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
        let menu = options
            .iter()
            .enumerate()
            .fold(menu, |menu, (index, label)| {
                let on_choose = on_choose.clone();
                menu.item(menu_option(
                    index,
                    label.clone(),
                    selected == Some(index),
                    move |window, cx| on_choose(index, window, cx),
                ))
            });
        menu.scrollable(true).max_h(px(320.))
    })
}
