//! 卡片头部（高 33）：程序图块、名称、目录；右边的按钮区只在卡片聚焦或悬停时显示。
//! 02 的按钮区只有关闭；工单 05 在关闭前面加向右分屏、向下分屏、放大（`card_button`）。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::CardId;
use super::program_tile::render_program_tile;
use crate::app::{CatalogIcon, CloseCard};
use crate::i18n::t;
use crate::theme::{CARD_HEADER_HEIGHT, ICON_BUTTON, ICON_BUTTON_RADIUS, Tokens};

pub(super) fn render_header(
    id: CardId,
    name: SharedString,
    directory: String,
    focused: bool,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
    div()
        .id(id.element("card-header"))
        .test_support()
        .aria_label(format!("{name} {directory}"))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .h(CARD_HEADER_HEIGHT)
        .pl(px(12.))
        .pr(px(8.))
        .child(render_program_tile(id.element("card-tile"), tokens))
        .child(
            div()
                .flex_none()
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .whitespace_nowrap()
                .child(name),
        )
        .child(
            div()
                .min_w_0()
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(tokens.muted)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(directory),
        )
        .child(
            div()
                .ml_auto()
                .flex()
                .flex_none()
                .items_center()
                // 只在聚焦或悬停时显示。
                .when(!focused, |buttons| {
                    buttons
                        .invisible()
                        .group_hover("card", |style| style.visible())
                })
                .child(card_button(
                    id.element("card-close"),
                    CatalogIcon::Close,
                    t!("card.close"),
                    Box::new(CloseCard(id)),
                    target,
                    tokens,
                )),
        )
}

/// 头部按钮区的图标按钮：点了经窗口壳的焦点派发 `action`。
pub(super) fn card_button(
    id: ElementId,
    icon: CatalogIcon,
    label: SharedString,
    action: Box<dyn Action>,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement + use<> {
    let target = target.clone();
    let hover = tokens.hover;
    let text = tokens.text;
    div()
        .id(id)
        .test_support()
        .aria_label(label)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(ICON_BUTTON)
        .rounded(ICON_BUTTON_RADIUS)
        .text_color(tokens.icon)
        .cursor_pointer()
        .hover(move |style| style.bg(hover).text_color(text))
        .on_click(move |_, window, cx| target.dispatch_action(action.as_ref(), window, cx))
        .child(Icon::new(icon).size(ICON_BUTTON * 0.6))
}
