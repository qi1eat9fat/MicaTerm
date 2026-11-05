//! 卡片头部（高 33）：程序图块、名称、目录、状态点；右边的按钮区只在卡片聚焦或悬停时显示。
//! 按钮依次是向右分屏、向下分屏、放大（放大着时是还原）、关闭；双击头部也是放大和还原（工单 05）。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::CardId;
use super::program_tile::render_program_tile;
use super::status_dot::{StatusDot, render_status_dot};
use crate::app::{CatalogIcon, CloseCard, SplitCard, ZoomCard};
use crate::i18n::t;
use crate::programs::Program;
use crate::split::Axis;
use crate::theme::{CARD_HEADER_HEIGHT, ICON_BUTTON, ICON_BUTTON_RADIUS, Tokens};

/// 头部跟着卡片的位置和里面的程序变的东西。
pub(super) struct HeaderState {
    /// 是所在标签的焦点卡片：按钮常显。
    pub focused: bool,
    /// 放大着：放大按钮换成还原。
    pub zoomed: bool,
    /// 图块上的程序（工单 06）。
    pub program: Program,
    /// 目录后面的状态点。
    pub dot: StatusDot,
    /// 远程卡片没连着（正在连接、已断开）：名称变灰（原型 `.card.off .chead .nm`，工单 12）。
    pub offline: bool,
    /// 悬停头部时的提示：远程卡片的完整窗口标题（工单 17；头部本身固定显示「主机名 + 目录」）。
    pub tooltip: Option<SharedString>,
}

/// 悬停卡片头部时的提示（原型 `.tip`）。
struct HeaderTooltip(SharedString);

impl Render for HeaderTooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        div()
            .id("card-header-tooltip")
            .test_support()
            .aria_label(self.0.clone())
            .max_w(px(480.))
            .px(px(9.))
            .py(px(5.))
            .rounded(px(7.))
            .bg(tokens.tooltip)
            .text_color(tokens.tooltip_text)
            .text_size(px(11.5))
            .line_height(relative(1.45))
            .child(self.0.clone())
    }
}

pub(super) fn render_header(
    id: CardId,
    name: SharedString,
    directory: String,
    state: HeaderState,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
    let zoom_target = target.clone();
    let (zoom_icon, zoom_label) = if state.zoomed {
        (CatalogIcon::Minimize2, t!("card.restore"))
    } else {
        (CatalogIcon::Maximize2, t!("card.zoom"))
    };
    let tooltip = state.tooltip.clone();
    div()
        .id(id.element("card-header"))
        .test_support()
        .aria_label(format!("{name} {directory}"))
        .when_some(tooltip, |header, title| {
            header.tooltip(move |_, cx| cx.new(|_| HeaderTooltip(title.clone())).into())
        })
        // 双击头部放大或还原。
        .on_click(move |event, window, cx| {
            if event.click_count() == 2 {
                zoom_target.dispatch_action(&ZoomCard(id), window, cx);
            }
        })
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .h(CARD_HEADER_HEIGHT)
        .pl(px(12.))
        .pr(px(8.))
        .child(render_program_tile(
            id.element("card-tile"),
            &state.program,
            tokens,
        ))
        .child(
            div()
                .flex_none()
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .whitespace_nowrap()
                .when(state.offline, |name| name.text_color(tokens.offline))
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
        .children(render_status_dot(id.element("card-dot"), state.dot, tokens))
        .child(
            div()
                .ml_auto()
                .flex()
                .flex_none()
                .items_center()
                // 只在聚焦或悬停时显示。
                .when(!state.focused, |buttons| {
                    buttons
                        .invisible()
                        .group_hover("card", |style| style.visible())
                })
                .child(card_button(
                    id.element("card-split-right"),
                    CatalogIcon::Columns2,
                    t!("card.split_right"),
                    Box::new(SplitCard(id, Axis::Horizontal)),
                    target,
                    tokens,
                ))
                .child(card_button(
                    id.element("card-split-down"),
                    CatalogIcon::Rows2,
                    t!("card.split_down"),
                    Box::new(SplitCard(id, Axis::Vertical)),
                    target,
                    tokens,
                ))
                .child(card_button(
                    id.element("card-zoom"),
                    zoom_icon,
                    zoom_label,
                    Box::new(ZoomCard(id)),
                    target,
                    tokens,
                ))
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
        .on_click(move |_, window, cx| {
            // 连点按钮不算双击头部。
            cx.stop_propagation();
            target.dispatch_action(action.as_ref(), window, cx)
        })
        .child(Icon::new(icon).size(ICON_BUTTON * 0.6))
}
