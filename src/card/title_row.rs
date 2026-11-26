//! 卡片顶行：高 24、和终端同底色、没有分隔线。
//!
//! - 左边：图标（远程是服务器、本地是终端）+ 名称，图标和上面标签里的图标左对齐；放不下从后面截断。
//!   名称始终是正文色，不跟焦点变淡。
//! - 正中：前台在跑的程序（黄点 + 程序名，只写程序名不带参数），按整条居中；回到提示符时没有；
//!   离两边不到 12 就藏起来。
//! - 右边：放大时「还原」，然后「向右分屏」「向下分屏」「关闭」；「关闭」一直在，
//!   标签里只剩一张卡片时也放。不是焦点卡片时按钮平时不显示，鼠标移到这张卡片上才出来。
//! - 单击把焦点给这张卡片；双击（按钮以外的任何地方）放大或还原它，像双击窗口标题栏。
//! - 非焦点卡片的图标、程序名用淡色，和空心光标一起认出焦点卡片。

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::CardId;
use super::fit::{centered_fits, text_width};
use super::program_tile::render_tab_tile;
use crate::app::{
    CatalogIcon, CloseCard, CloseFocusedCard, HandCardToAgent, SplitCard, SplitDown, SplitRight,
    TakeCardBack, ToggleZoomCard, ZoomCard,
};
use crate::i18n::t;
use crate::programs::Program;
use crate::split::Axis;
use crate::theme::{Tokens, TypeScale, WEIGHT_NORMAL, size};
use crate::ui::icon_button::IconButton;

/// 顶行的高度。
pub(super) const TITLE_ROW: Pixels = size::CARD_TITLE;
/// 左边距，和上面标签里的图标对齐。
pub(super) const ROW_LEFT: f32 = 10.;
/// 顶行和状态栏里的圆点与字之间的距离。
pub(super) const ICON_GAP: f32 = 6.;
/// 顶行前面的程序图块。
const TILE: Pixels = size::TAB_BADGE;
const TILE_GAP: f32 = 6.;
const BUTTON_GAP: f32 = 2.;
/// Agent 按钮为对齐顶栏单独保留的右边距，不属于按钮之间的 gap。
const AGENT_MARGIN: f32 = 2.;
/// 右边只留 1：顶行按钮的中心正好落在顶栏 22 宽窗口按钮的圆点中心上
/// （「关闭」对关闭圆点，「向下分屏」对最大化，「向右分屏」对最小化）。
/// 新卡片选择器的顶行也用这个值。
pub(super) const ROW_RIGHT: f32 = 1.;

/// 顶行要显示的东西，render 前从卡片取好。
pub(super) struct TitleRow {
    /// 前面那块实心图块画的程序：和标签里这块卡片的图块是同一个。
    pub tile: Program,
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
    /// 远程卡片才有：`Some(已经交给 Agent)`。按钮在分屏按钮左边，和顶栏的 Agent 按钮同一列。
    pub agent: Option<bool>,
}

impl CardPlace {
    /// 顶行右边有几个按钮：分屏两个、关闭一个，放大时多一个「还原」，远程卡片多一个 Agent。
    fn buttons(self) -> usize {
        3 + usize::from(self.zoomed) + usize::from(self.agent.is_some())
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

/// 状态圆点。
pub(super) fn dot(color: Hsla) -> Div {
    crate::ui::dot(color, size::DOT)
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
            .sm()
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
        // 远程卡片的「交给 Agent / 收回 Agent」，排在分屏按钮左边。
        .when_some(place.agent, |buttons, handed| {
            let (label, action): (SharedString, Box<dyn Action>) = if handed {
                (t!("card.menu.take_back"), Box::new(TakeCardBack(id)))
            } else {
                (t!("card.menu.hand_to_agent"), Box::new(HandCardToAgent(id)))
            };
            // 和顶栏 Agent 图标同一列：那边和窗口按钮之间还隔着 2 的间距 + 1 的分隔线，
            // 这里补上同样的 2，中心就落在同一竖线上。
            buttons.child(
                div().flex_none().mr(px(AGENT_MARGIN)).child(
                    button("card-agent", CatalogIcon::Sparkles, label)
                        .pressed(handed)
                        .dispatch(action, target, tokens),
                ),
            )
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
        .child(
            button("card-close", CatalogIcon::Close, t!("card.close"))
                .tooltip_keys(Box::new(CloseFocusedCard))
                .dispatch(Box::new(CloseCard(id)), target, tokens),
        )
}

/// 右边按钮区实际占的宽度：按钮、按钮间距，以及 Agent 按钮单独的外边距。
pub(super) fn buttons_width(place: CardPlace) -> f32 {
    let buttons = place.buttons() as f32;
    buttons * f32::from(size::CONTROL_SM)
        + (buttons - 1.) * BUTTON_GAP
        + f32::from(place.agent.is_some()) * AGENT_MARGIN
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
    // 名称始终是正文色，不随焦点变淡（照原型窗格头）。
    let text = tokens.text;
    let right = ROW_RIGHT + buttons_width(place);
    let left = ROW_LEFT + f32::from(TILE) + TILE_GAP + text_width(window, &row.name, scale.sm);
    let program = row.program.clone().filter(|program| {
        let middle = f32::from(size::DOT) + ICON_GAP + text_width(window, program, scale.sm);
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
        .text_size(scale.sm)
        .font_weight(WEIGHT_NORMAL)
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
                .gap(px(TILE_GAP))
                .child(render_tab_tile(
                    id.element("card-title-tile"),
                    &row.tile,
                    tokens,
                ))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(row.name.clone()),
                ),
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

#[cfg(test)]
mod tests {
    use super::{AGENT_MARGIN, BUTTON_GAP, CardPlace, buttons_width};
    use crate::theme::size;

    fn place(zoomed: bool, agent: Option<bool>) -> CardPlace {
        CardPlace {
            focused: true,
            active: true,
            zoomed,
            agent,
        }
    }

    #[test]
    fn button_budget_includes_every_gap_and_the_agent_margin() {
        let button = f32::from(size::CONTROL_SM);
        assert_eq!(
            buttons_width(place(false, None)),
            3. * button + 2. * BUTTON_GAP
        );
        assert_eq!(
            buttons_width(place(true, Some(false))),
            5. * button + 4. * BUTTON_GAP + AGENT_MARGIN
        );
    }
}
