//! 卡片顶行（26 号「分屏」，2026-11-14 用户定）：高 28、和终端同底色、没有分隔线，
//! 看上去是终端上边距里的一行。
//!
//! - 左边：图标（远程是服务器、本地是终端）+ 名称，和下面的终端文字左对齐；放不下从后面截断。
//! - 正中：前台在跑的程序（黄点 + 程序名，只写程序名不带参数），按整条居中；回到提示符时没有；
//!   离两边不到 12 就藏起来。
//! - 右边：放大时「还原」，然后「向右分屏」「向下分屏」「关闭」；标签里只有这一张时不放「关闭」
//!   （和标签上的 × 是同一件事）。不是焦点卡片时按钮平时不显示，鼠标移到这张卡片上才出来。
//! - 单击把焦点给这张卡片；双击（按钮以外的任何地方）放大或还原它，像双击窗口标题栏。
//! - 非焦点卡片的名称、程序名、图标都用淡色，和空心光标一起认出焦点卡片。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::CardId;
use super::fit::{centered_fits, text_width};
use crate::app::{
    CatalogIcon, CloseCard, CloseFocusedCard, SplitCard, SplitDown, SplitRight, ToggleZoomCard,
    ZoomCard,
};
use crate::i18n::t;
use crate::split::Axis;
use crate::theme::{Tokens, TypeScale, size};
use crate::ui::icon_button::IconButton;

/// 顶行的高度（和状态栏一样）。
pub(super) const TITLE_ROW: Pixels = px(28.);
/// 左边距，和终端文字对齐（26 号 `.ctop` padding-left 20）。
pub(super) const ROW_LEFT: f32 = 20.;
/// 顶行和状态栏里的 13 图标、它和字之间的距离。
pub(super) const ROW_ICON: f32 = 13.;
pub(super) const ICON_GAP: f32 = 6.;
/// 按钮的边长、图标大小、按钮之间的距离、顶行右边距。
const BUTTON: f32 = 22.;
const BUTTON_ICON: Pixels = px(14.);
const BUTTON_GAP: f32 = 2.;
const ROW_RIGHT: f32 = 6.;

/// 顶行要显示的东西，render 前从卡片取好。
pub(super) struct TitleRow {
    pub remote: bool,
    pub name: SharedString,
    /// 前台在跑的程序（shell 以外的）。
    pub program: Option<SharedString>,
}

/// 卡片在标签里的样子：顶行、状态栏跟着变。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CardPlace {
    /// 焦点在这张卡片的终端里：不是的话字和图标用淡色。
    pub focused: bool,
    /// 是标签的焦点卡片：按钮常显；不是的话鼠标移上来才出来。
    pub active: bool,
    /// 放大着：分屏按钮前面多一个「还原」。
    pub zoomed: bool,
    /// 标签里只有这一张：不放「关闭」。
    pub alone: bool,
}

impl CardPlace {
    /// 顶行右边有几个按钮。
    fn buttons(self) -> usize {
        2 + usize::from(self.zoomed) + usize::from(!self.alone)
    }
}

/// 画顶行、状态栏都要的：按钮经它派发的焦点（卡片的终端）、窗口（量文字宽度）、颜色、字号。
#[derive(Clone, Copy)]
pub(super) struct Paint<'a> {
    pub target: &'a FocusHandle,
    pub window: &'a Window,
    pub tokens: &'a Tokens,
    pub scale: TypeScale,
}

/// 顶行和状态栏里的 13 图标。真程序（150%）实测几何居中的图标比中文字面中心高，往下挪半个像素。
pub(super) fn row_icon(icon: CatalogIcon, color: Hsla) -> Div {
    div()
        .flex_none()
        .relative()
        .top(px(0.5))
        .text_color(color)
        .child(Icon::new(icon).size(px(ROW_ICON)))
}

/// 6 的圆点。
pub(super) fn dot(color: Hsla) -> Div {
    div().flex_none().size(size::DOT).rounded_full().bg(color)
}

/// 顶行右边的按钮。经卡片终端的焦点 `target` 派发；点按钮不算点顶行（不触发双击放大）。
fn render_buttons(
    id: CardId,
    place: CardPlace,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
    let color = if place.focused {
        tokens.icon
    } else {
        tokens.st_dim
    };
    let button = |name: &'static str, icon: CatalogIcon, label: SharedString| {
        IconButton::new(id.element(name), icon, label.clone())
            .size(px(BUTTON), BUTTON_ICON)
            .radius(px(5.))
            .icon_color(color)
            .tooltip(label)
    };
    let split = |axis: Axis| Box::new(SplitCard { card: id, axis });
    div()
        .id(id.element("card-buttons"))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(BUTTON_GAP))
        .on_click(|_, _, cx| cx.stop_propagation())
        // 非焦点卡片：平时不显示，鼠标移到这张卡片上才出来（透明而不是不画，一移上来就点得到）。
        .when(!place.active, |buttons| {
            buttons
                .opacity(0.)
                .group_hover("card", |style| style.opacity(1.))
        })
        .when(place.zoomed, |buttons| {
            buttons.child(
                button("card-restore", CatalogIcon::Minimize2, t!("card.restore"))
                    .tooltip_keys(Box::new(ToggleZoomCard))
                    .dispatch(Box::new(ZoomCard(id)), target, tokens),
            )
        })
        .child(
            button(
                "card-split-right",
                CatalogIcon::Columns2,
                t!("card.split_right"),
            )
            .tooltip_keys(Box::new(SplitRight))
            .dispatch(split(Axis::Horizontal), target, tokens),
        )
        .child(
            button("card-split-down", CatalogIcon::Rows2, t!("card.split_down"))
                .tooltip_keys(Box::new(SplitDown))
                .dispatch(split(Axis::Vertical), target, tokens),
        )
        .when(!place.alone, |buttons| {
            buttons.child(
                button("card-close", CatalogIcon::Close, t!("card.close"))
                    .tooltip_keys(Box::new(CloseFocusedCard))
                    .dispatch(Box::new(CloseCard(id)), target, tokens),
            )
        })
}

/// 画顶行。`width` 是上一帧量到的卡片宽度（还没量过是 0）。
pub(super) fn render_title_row(
    id: CardId,
    row: &TitleRow,
    place: CardPlace,
    width: f32,
    paint: &Paint,
) -> impl IntoElement {
    let Paint {
        target,
        window,
        tokens,
        scale,
    } = *paint;
    let dim = !place.focused;
    let text = if dim { tokens.st_dim } else { tokens.text2 };
    let icon = if row.remote {
        CatalogIcon::Server
    } else {
        CatalogIcon::SquareTerminal
    };
    let buttons = place.buttons() as f32;
    let right = ROW_RIGHT + buttons * BUTTON + (buttons - 1.) * BUTTON_GAP;
    let left = ROW_LEFT + ROW_ICON + ICON_GAP + text_width(window, &row.name, scale.ui);
    let program = row.program.clone().filter(|program| {
        let middle = f32::from(size::DOT) + ICON_GAP + text_width(window, program, scale.ui);
        centered_fits(width, left, right, middle)
    });
    let label = match &row.program {
        Some(program) => format!("{} · {program}", row.name),
        None => row.name.to_string(),
    };
    div()
        .id(id.element("card-title"))
        .test_support()
        .aria_label(label)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .h(TITLE_ROW)
        .pl(px(ROW_LEFT))
        .pr(px(ROW_RIGHT))
        .text_size(scale.ui)
        .text_color(text)
        .whitespace_nowrap()
        .overflow_hidden()
        // 双击放大或还原这张卡片（标签里只有它时没反应）。
        .on_click({
            let target = target.clone();
            move |event, window, cx| {
                if event.click_count() == 2 {
                    target.dispatch_action(&ZoomCard(id), window, cx);
                }
            }
        })
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(ICON_GAP))
                .child(row_icon(
                    icon,
                    if dim { tokens.st_dim } else { tokens.icon },
                ))
                .child(div().min_w_0().truncate().child(row.name.clone())),
        )
        .when_some(program, |bar, program| {
            bar.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .justify_center()
                    .items_center()
                    .child(
                        div()
                            .id(id.element("card-title-program"))
                            .test_support()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(ICON_GAP))
                            .text_color(tokens.st_dim)
                            .child(dot(tokens.warn))
                            .child(program),
                    ),
            )
        })
        .child(render_buttons(id, place, target, tokens))
}
