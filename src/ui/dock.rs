//! 右侧停靠面板（宽 380，左边线可拖）：内容底、左边一条细线，照官网原型的右侧源码管理/文件面板。
//! 凭据、片段两个面板共用这里的外框、标题行、折叠分组头和空状态提示。
//!
//! - 标题行：高 24（与顶栏同高），贯通全宽的下边线；左边 12 图标 + 11 SemiBold 标题（可跟 `→ 卡片名`），
//!   最右两个 20×20 图标按钮（「+」和「×」关闭）。
//! - 分组头（`dock_group`）：高 22、左右内缩 4px 微圆角悬停，左边是折叠箭头 + 11 SemiBold 次要色标题 + 胶囊计数徽章。

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::CatalogIcon;
use crate::theme::{Tokens, TypeScale, WEIGHT_NORMAL, WEIGHT_SEMIBOLD, size};
use crate::ui::count::count_badge;
use crate::ui::empty::{EmptyPlacement, EmptyState};
use crate::ui::icon_button::IconButton;

/// 分组行的高（与树节点行同高 22px）。
const GROUP: Pixels = size::DOCK_ROW;

/// 面板外框：不设左右内边距，保证标题行下边线和树列表悬停条贯通整个面板宽度。
pub fn dock_panel(id: &'static str, tokens: &Tokens) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .flex_col()
        // 宽度由外面的可拖分隔决定（`RightPanelWidths`），这里填满。
        .flex_1()
        .min_w_0()
        .min_h_0()
        .bg(tokens.content)
        .border_l(size::LINE)
        .border_color(tokens.line)
}

/// 标题行右边的「+」：元素 id、无障碍名称（也是悬停提示）、点了经 `target` 派发的 action。
pub struct DockNew {
    pub id: &'static str,
    pub label: SharedString,
    pub action: Box<dyn Action>,
}

/// 标题后面的「→ 名字」：元素 id、无障碍名称、显示的名字。
pub struct DockTarget {
    pub id: &'static str,
    pub label: SharedString,
    pub name: SharedString,
}

/// 标题行右边的「收起」：元素 id、无障碍名称（也是悬停提示）、点了经 `target` 派发的 action。
pub struct DockClose {
    pub id: &'static str,
    pub label: SharedString,
    pub action: Box<dyn Action>,
}

/// 标题行（照官网原型的 `Source Control` / `Files` 面板顶栏）。
pub fn dock_header(
    icon: CatalogIcon,
    title: SharedString,
    to: Option<DockTarget>,
    new: DockNew,
    close: DockClose,
    target: &FocusHandle,
    tokens: &Tokens,
    cx: &App,
) -> impl IntoElement + use<> {
    let scale = TypeScale::current(cx);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(size::DOCK_HEADER)
        .pl(px(10.))
        // 右边只留 1：20 宽按钮的中心正好落在顶栏 22 宽窗口按钮的圆点中心上
        // （「×」对关闭、「+」对最大化）。
        .pr(px(1.))
        .border_b(size::LINE)
        .border_color(tokens.line)
        .text_size(scale.sm)
        .line_height(size::DOCK_HEADER)
        .text_color(tokens.text)
        .font_weight(WEIGHT_SEMIBOLD)
        .whitespace_nowrap()
        .child(
            div()
                .flex_none()
                .text_color(tokens.icon)
                .child(Icon::new(icon).size(size::ICON_SM)),
        )
        .child(div().flex_none().child(title))
        .when_some(to, |row, to| {
            row.child(
                Icon::new(IconName::ArrowRight)
                    .size(px(11.))
                    .flex_none()
                    .text_color(tokens.st_dim),
            )
            .child(
                div()
                    .id(to.id)
                    .test_support()
                    .aria_label(to.label)
                    .min_w_0()
                    .truncate()
                    .text_size(scale.sm)
                    .font_weight(WEIGHT_NORMAL)
                    .text_color(tokens.st_dim)
                    .child(to.name),
            )
        })
        .child(
            div()
                .ml_auto()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(2.))
                .child(
                    IconButton::new(new.id, IconName::Plus, new.label.clone())
                        .sm()
                        .tooltip(new.label)
                        .dispatch(new.action, target, tokens),
                )
                .child(
                    IconButton::new(close.id, IconName::Close, close.label.clone())
                        .sm()
                        .tooltip(close.label)
                        .dispatch(close.action, target, tokens),
                ),
        )
}

/// 分组标题行（对标官网 `Source Control › Changes 80`）：折叠箭头 + 次要色 SemiBold 分组名 + 胶囊计数徽章，微圆角内缩悬停。
#[allow(clippy::too_many_arguments)]
pub fn dock_group(
    id: impl Into<ElementId>,
    label: SharedString,
    count: usize,
    collapsed: bool,
    first: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    tokens: &Tokens,
    cx: &App,
) -> AnyElement {
    let scale = TypeScale::current(cx);
    let hover = tokens.hover;
    let text = tokens.text;
    div()
        .id(id.into())
        .test_support()
        .aria_label(label.clone())
        .aria_expanded(!collapsed)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .h(GROUP)
        .mx(px(4.))
        .mt(if first { px(4.) } else { px(6.) })
        .pl(px(6.))
        .pr(px(8.))
        .rounded(size::RADIUS_SM)
        .text_size(scale.sm)
        .line_height(GROUP)
        .font_weight(WEIGHT_SEMIBOLD)
        .text_color(tokens.text2)
        .cursor_pointer()
        .hover(move |style| style.bg(hover).text_color(text))
        .on_click(on_toggle)
        .child(
            Icon::new(if collapsed {
                IconName::ChevronRight
            } else {
                IconName::ChevronDown
            })
            .size(px(11.))
            .flex_none()
            .text_color(tokens.st_dim),
        )
        .child(div().child(label))
        .child(count_badge(count, tokens, scale).ml(px(2.)))
        .into_any_element()
}

/// 列表里的一句话：还没有东西、现在用不了。`hint` 是下面小一号的第二行。
pub fn dock_empty(
    id: &'static str,
    text: SharedString,
    hint: Option<SharedString>,
    tokens: &Tokens,
    cx: &App,
) -> AnyElement {
    let mut empty = EmptyState::new(id, text, EmptyPlacement::Dock);
    if let Some(hint) = hint {
        empty = empty.hint(hint);
    }
    empty.render(tokens, TypeScale::current(cx))
}
