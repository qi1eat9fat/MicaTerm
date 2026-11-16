//! 右侧停靠面板（26 号 `.dk`，照 Zed 停靠面板）：默认宽 280（左边线可拖）、左右留 8、内容底、左边一条细线，
//! 从右侧图标栏展开；凭据、片段两个面板共用这里的外框、标题行、分组标题和搜索框。
//!
//! - 标题行（`.dk-h`）：高 32、左留 8；标题是正文色界面字，后面可跟「→ 焦点卡片名」（13 的线条箭头 +
//!   淡色名字，三段同一个间距 4）；最右一个 24 的「+」（图标 14）。
//! - 分组标题（`.gh`）：高 28、左右留 8，次要色小一号的字，后面一个实心小三角（展开朝下、收起朝右），
//!   点它收起 / 展开这一组；紧跟标题行的第一组上面留 4，其余留 14。
//! - 搜索框（`.dks`）：高 28、比面板深一档的底、细边，聚焦时边深一档；淡色放大镜。

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::{TRIANGLE_DOWN_ICON, TRIANGLE_RIGHT_ICON};
use crate::theme::{Tokens, TypeScale, size};
use crate::ui::icon_button::IconButton;

/// 标题行的高。
const HEADER: Pixels = px(32.);
/// 标题行「+」的边长和图标边长。
const HEADER_BUTTON: Pixels = px(24.);
const HEADER_ICON: Pixels = px(14.);
/// 分组标题的高。
const GROUP: Pixels = px(28.);
/// 搜索框的高。
const SEARCH: Pixels = px(28.);

/// 面板外框：子元素从上往下排。调用方加完自己的东西后再调 `.test_support()`（测试构建里它换类型）。
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
        .px(px(8.))
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

/// 标题行（`.dk-h`）。
pub fn dock_header(
    title: SharedString,
    to: Option<DockTarget>,
    new: DockNew,
    target: &FocusHandle,
    tokens: &Tokens,
    cx: &App,
) -> impl IntoElement + use<> {
    let scale = TypeScale::current(cx);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .h(HEADER)
        .pl(px(8.))
        .text_size(scale.ui)
        .line_height(HEADER)
        .text_color(tokens.text)
        .whitespace_nowrap()
        .child(div().flex_none().child(title))
        .when_some(to, |row, to| {
            row.child(
                Icon::new(IconName::ArrowRight)
                    .size(px(13.))
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
                    .text_color(tokens.st_dim)
                    .child(to.name),
            )
        })
        .child(
            div().ml_auto().flex_none().child(
                IconButton::new(new.id, IconName::Plus, new.label.clone())
                    .size(HEADER_BUTTON, HEADER_ICON)
                    .tooltip(new.label)
                    .dispatch(new.action, target, tokens),
            ),
        )
}

/// 分组标题（`.gh`）：`first` 是紧跟标题行的那组。点了调 `on_toggle`（收起 / 展开）。
pub fn dock_group(
    id: impl Into<ElementId>,
    label: SharedString,
    collapsed: bool,
    first: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    tokens: &Tokens,
    cx: &App,
) -> AnyElement {
    let scale = TypeScale::current(cx);
    // 26 号 `--fs-gh`：界面字号 14 时 13，13、12 时都是 12。
    let text_size = scale.small;
    let color = if cx.theme().is_dark() {
        tokens.st_dim
    } else {
        tokens.text2
    };
    div()
        .id(id.into())
        .test_support()
        .aria_label(label.clone())
        .aria_expanded(!collapsed)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(GROUP)
        .mt(if first { px(4.) } else { px(14.) })
        .px(px(8.))
        .text_size(text_size)
        .line_height(GROUP)
        .text_color(color)
        .cursor_pointer()
        .on_click(on_toggle)
        .child(label)
        .child(
            Icon::default()
                .path(if collapsed {
                    TRIANGLE_RIGHT_ICON
                } else {
                    TRIANGLE_DOWN_ICON
                })
                .size(px(7.))
                .opacity(0.8),
        )
        .into_any_element()
}

/// 搜索框（`.dks`）：`id` 给输入框，`label` 是它的无障碍名称。
pub fn dock_search(
    id: &'static str,
    label: SharedString,
    input: &Entity<InputState>,
    tokens: &Tokens,
    window: &Window,
    cx: &App,
) -> Div {
    let scale = TypeScale::current(cx);
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(SEARCH)
        .mt(px(2.))
        .mb(px(8.))
        .px(px(8.))
        .rounded(size::RADIUS)
        .bg(tokens.input)
        .border_1()
        .border_color(if focused {
            tokens.line_strong
        } else {
            tokens.line
        })
        .child(
            Icon::new(IconName::Search)
                .size(px(14.))
                .flex_none()
                .text_color(tokens.st_dim),
        )
        .child(
            div().flex_1().min_w_0().child(
                Input::new(input)
                    .id(id)
                    .aria_label(label)
                    .appearance(false)
                    .small()
                    .px_0()
                    .text_size(scale.ui),
            ),
        )
}

/// 列表里的一句话：还没有东西、搜不到、现在用不了。`hint` 是下面小一号的第二行。
pub fn dock_empty(
    id: &'static str,
    text: SharedString,
    hint: Option<SharedString>,
    tokens: &Tokens,
    cx: &App,
) -> AnyElement {
    let scale = TypeScale::current(cx);
    div()
        .id(id)
        .test_support()
        .aria_label(text.clone())
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
        .px(px(16.))
        .py(px(30.))
        .text_size(scale.ui)
        .text_color(tokens.st_dim)
        .child(text)
        .children(hint.map(|hint| {
            div()
                .text_size(scale.mono_small)
                .text_color(tokens.faint)
                .text_center()
                .child(hint)
        }))
        .into_any_element()
}
