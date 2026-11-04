//! 设置对话框里的通用件，照全部界面原型的样式：分组标题（`h4`）、分组（`.sgrp`）、
//! 一行设置（`.srow`：左边名称和说明，右边控件）、分段按钮（`.seg`）、下拉按钮。

use std::rc::Rc;

use gpui_kit::component::{
    Sizable as _,
    button::Button,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::Tokens;

/// 分组标题（原型的 `h4`）：12px 半粗、次要文字色。
pub fn section_title(title: SharedString, note: Option<SharedString>, tokens: &Tokens) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap(px(6.))
        .mb(px(8.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens.muted)
        .child(title)
        .when_some(note, |title, note| {
            title.child(
                div()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(tokens.faint)
                    .child(note),
            )
        })
}

/// 一组设置（原型的 `.sgrp`）：分组底、12 圆角、细描边，行与行之间有分隔线。
pub fn group(rows: impl IntoIterator<Item = AnyElement>, tokens: &Tokens) -> Div {
    let line = tokens.line;
    let mut rows = rows.into_iter().peekable();
    let mut children = Vec::new();
    while let Some(row) = rows.next() {
        let last = rows.peek().is_none();
        children.push(
            div()
                .when(!last, |row| row.border_b_1().border_color(line))
                .child(row)
                .into_any_element(),
        );
    }
    div()
        .flex()
        .flex_col()
        .mb(px(18.))
        .rounded(px(12.))
        .bg(tokens.group)
        .border_1()
        .border_color(tokens.line)
        .children(children)
}

/// 一行设置（原型的 `.srow`）：左边名称和说明，右边控件。
pub fn row(
    label: SharedString,
    description: Option<SharedString>,
    control: impl IntoElement,
    tokens: &Tokens,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(14.))
        .py(px(10.))
        .min_h(px(50.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(div().text_size(px(13.)).child(label))
                .when_some(description, |text, description| {
                    text.child(
                        div()
                            .mt(px(2.))
                            .text_size(px(11.5))
                            .line_height(relative(1.45))
                            .text_color(tokens.faint)
                            .child(description),
                    )
                }),
        )
        .child(div().flex_none().child(control))
        .into_any_element()
}

/// 选中某一项时做的事。
pub type OnChoose = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// 分段按钮（原型的 `.seg`）：每一项是 `(id, index)`，选中的那一项浮起来。
/// 选中状态写在无障碍属性里，测试靠它断言。
pub fn segmented(
    id: &'static str,
    options: Vec<SharedString>,
    selected: usize,
    on_choose: OnChoose,
    tokens: &Tokens,
) -> Div {
    let on = tokens.tab_active;
    let shadow = tokens.shadow;
    div()
        .flex()
        .p(px(2.))
        .rounded(px(15.))
        .bg(tokens.segment)
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
                .h(px(26.))
                .px(px(12.))
                .rounded(px(13.))
                .text_size(px(12.5))
                .cursor_pointer()
                .map(|button| {
                    if chosen {
                        button.bg(on).text_color(tokens.text).shadow(vec![BoxShadow {
                            color: shadow,
                            offset: point(px(0.), px(1.)),
                            blur_radius: px(4.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                    } else {
                        let text = tokens.text;
                        button
                            .text_color(tokens.muted)
                            .hover(move |style| style.text_color(text))
                    }
                })
                .on_click(move |_, window, cx| on_choose(index, window, cx))
                .child(label)
        }))
}

/// 下拉按钮（原型的 `.in.sel`）：按钮上是当前值，点开是选项菜单，当前值打勾。
pub fn dropdown(
    id: impl Into<ElementId>,
    current: SharedString,
    options: Vec<SharedString>,
    selected: Option<usize>,
    on_choose: OnChoose,
    width: Pixels,
) -> impl IntoElement {
    Button::new(id)
        .label(current)
        .dropdown_caret(true)
        .outline()
        .small()
        .w(width)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            let menu = options
                .iter()
                .enumerate()
                .fold(menu, |menu, (index, label)| {
                    let on_choose = on_choose.clone();
                    menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(selected == Some(index))
                            .on_click(move |_, window, cx| on_choose(index, window, cx)),
                    )
                });
            menu.scrollable(true).max_h(px(320.))
        })
}
