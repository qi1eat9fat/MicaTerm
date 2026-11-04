//! 标签轨道（原型 `.track`）和里面的标签（`.tab`）：宽固定 180、高 28，叠放的程序图块、名称、目录和状态点。
//! 点标签切过去；悬停时状态点让位给 ×，点了关闭整个标签；右键菜单三项；放不下时轨道横向滚动。
//! 渲染不读实体，只拿窗口壳取好的快照；点击、菜单都经窗口壳的焦点派发 action。
// 参考 gorex 的模型重写：tabs.go 的标签外观、悬停时出现 ×。

use std::cell::Cell;

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::TabId;
use crate::app::{
    ActivateTab, CatalogIcon, CloseOtherTabs, CloseTab, CloseTabsToRight, OpenTabMenu,
};
use crate::card::{StatusDot, render_status_dot, render_tile_stack};
use crate::programs::Program;
use crate::i18n::t;
use crate::theme::{TAB_HEIGHT, TAB_TRACK_HEIGHT, TAB_WIDTH, Tokens};

/// 画一个标签要的东西。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabSnapshot {
    pub id: TabId,
    /// 焦点卡片的名称（shell 名、主机名）。
    pub name: SharedString,
    /// 焦点卡片的目录。
    pub directory: SharedString,
    pub active: bool,
    /// 叠放的程序图块：焦点卡片的在最前面，其余按分屏树的顺序，最多 3 块。
    pub tiles: Vec<Program>,
    /// 标签上的状态点：有卡片要注意是橙点，否则有卡片在输出是绿点。
    pub dot: StatusDot,
}

/// 标签轨道跨帧要记住的东西：横向滚动的位置、上一次露出来的在前标签、开着的右键菜单。
#[derive(Default)]
pub struct TabRail {
    scroll: ScrollHandle,
    /// 在前的标签一换，就把它滚进视野；没换时不动，用户自己滚开的位置保留。
    revealed: Cell<Option<TabId>>,
    /// 下一帧不管换没换，都把在前的标签滚进视野（又切到它一次时）。
    reveal_requested: Cell<bool>,
    menu: Option<OpenMenu>,
}

/// 开着的标签右键菜单。菜单实体由轨道持有，关菜单时一起放掉。
///
/// 不用 gpui-component 的 `context_menu`：它的状态和关菜单的订阅互相持有，
/// 菜单实体要等同一个元素下一次右键才放掉；标签关掉了就再也放不掉。
struct OpenMenu {
    tab: TabId,
    position: Point<Pixels>,
    view: Entity<PopupMenu>,
    _dismiss: Subscription,
}

/// 标签之间的竖线（`.tsep`）。
const SEPARATOR_WIDTH: Pixels = px(1.);
/// × 按钮的边长（`.tab .x`）。
const CLOSE_BUTTON: Pixels = px(18.);

impl TabRail {
    /// 轨道（底色圆角条）和里面的标签；没有标签时什么都不画。
    /// `target` 是窗口壳的焦点：点击和菜单都经它派发，焦点在哪里都送得到窗口壳。
    pub fn render(
        &self,
        tabs: Vec<TabSnapshot>,
        target: &FocusHandle,
        tokens: &Tokens,
    ) -> Option<impl IntoElement + use<>> {
        if tabs.is_empty() {
            self.revealed.set(None);
            return None;
        }
        self.reveal_active(&tabs);
        let active = tabs.iter().position(|tab| tab.active);
        let menu_tab = self.menu_tab();
        let count = tabs.len();
        let mut children = Vec::with_capacity(count * 2);
        for (index, tab) in tabs.into_iter().enumerate() {
            let pressed = menu_tab == Some(tab.id);
            children.push(render_tab(tab, pressed, target, tokens).into_any_element());
            if index + 1 < count {
                // 在前的标签两边不画竖线（它自己有底色）。
                let hidden = active == Some(index) || active == Some(index + 1);
                children.push(render_separator(hidden, tokens).into_any_element());
            }
        }
        Some(
            div()
                .id("tab-track")
                .test_support()
                .flex()
                .items_center()
                .h(TAB_TRACK_HEIGHT)
                .p(px(2.))
                .rounded(px(16.))
                .bg(tokens.track)
                .min_w_0()
                // 只横着滚：普通滚轮的竖直增量也换成横向（GPUI 对单轴滚动容器的处理）。
                .overflow_x_scroll()
                .track_scroll(&self.scroll)
                .children(children)
                // 放在最后，不打乱标签和竖线的序号；deferred 画在最上层，不受轨道裁剪。
                .children(self.menu.as_ref().map(|menu| {
                    deferred(
                        anchored()
                            .position(menu.position)
                            .snap_to_window_with_margin(px(8.))
                            .child(menu.view.clone()),
                    )
                    .with_priority(POPUP_PRIORITY)
                })),
        )
    }

    /// 下一帧把在前的标签滚进视野，即使它本来就在前（用户把它滚开了又切到它）。
    pub fn request_reveal(&self) {
        self.reveal_requested.set(true);
    }

    /// 在 `position`（窗口坐标）为 `tab` 开右键菜单；之前开着的那个放掉。
    /// `dismiss` 是窗口壳对菜单关闭事件的订阅，跟菜单一起放掉。
    pub fn open_menu(
        &mut self,
        tab: TabId,
        position: Point<Pixels>,
        view: Entity<PopupMenu>,
        dismiss: Subscription,
    ) {
        self.menu = Some(OpenMenu {
            tab,
            position,
            view,
            _dismiss: dismiss,
        });
    }

    /// 关掉右键菜单，只在开着的正是 `view` 时（右键另一个标签时，旧菜单的关闭事件晚到）。
    pub fn close_menu(&mut self, view: &Entity<PopupMenu>) {
        if self.menu.as_ref().is_some_and(|menu| &menu.view == view) {
            self.menu = None;
        }
    }

    /// 开着右键菜单的标签。
    pub fn menu_tab(&self) -> Option<TabId> {
        self.menu.as_ref().map(|menu| menu.tab)
    }

    /// 在前的标签换了：下一帧把它滚进视野。子元素里标签和竖线交替，第 i 个标签是第 2i 个子元素。
    fn reveal_active(&self, tabs: &[TabSnapshot]) {
        let Some((index, tab)) = tabs.iter().enumerate().find(|(_, tab)| tab.active) else {
            return;
        };
        if self.revealed.get() != Some(tab.id) || self.reveal_requested.get() {
            self.revealed.set(Some(tab.id));
            self.reveal_requested.set(false);
            self.scroll.scroll_to_item(index * 2);
        }
    }
}

/// 标签的右键菜单，只有三项。菜单派发的 action 送到 `target`（窗口壳的焦点）。
pub fn build_tab_menu(
    tab: TabId,
    target: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let target = target.clone();
    PopupMenu::build(window, cx, move |menu, _, _| {
        menu.action_context(target)
            .menu(t!("tabs.menu.close"), Box::new(CloseTab(tab)))
            .menu(t!("tabs.menu.close_others"), Box::new(CloseOtherTabs(tab)))
            .menu(t!("tabs.menu.close_right"), Box::new(CloseTabsToRight(tab)))
    })
}

/// `pressed`：这个标签的右键菜单开着（原型 `.tab.rc`）。
fn render_tab(
    tab: TabSnapshot,
    pressed: bool,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
    let hover = tokens.hover;
    let shadow = tokens.shadow;
    let id = tab.id;
    let activate = target.clone();
    let open_menu = target.clone();
    div()
        .id(("tab", id.0 as usize))
        .test_support()
        .group("tab")
        .aria_label(format!("{} {}", tab.name, tab.directory))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(9.))
        .w(TAB_WIDTH)
        .h(TAB_HEIGHT)
        .pl(px(6.))
        .pr(px(4.))
        .rounded(px(14.))
        .cursor_pointer()
        .when(tab.active, |this| {
            this.bg(tokens.tab_active).shadow(vec![
                BoxShadow {
                    color: shadow,
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: shadow,
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(8.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
        })
        .when(!tab.active, |this| this.hover(move |style| style.bg(hover)))
        .when(pressed, |this| this.bg(tokens.pressed))
        .on_click(move |_, window, cx| {
            activate.dispatch_action(&ActivateTab(id), window, cx);
        })
        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
            let position = event.position;
            open_menu.dispatch_action(&OpenTabMenu { tab: id, position }, window, cx);
        })
        .child(render_tile_stack(
            ("tab-tiles", id.0 as usize).into(),
            &tab.tiles,
            tokens,
        ))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .gap(px(4.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if tab.active {
                    tokens.text
                } else {
                    tokens.muted
                })
                .child(div().flex_none().child(tab.name))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(tokens.faint)
                        .child(tab.directory),
                ),
        )
        .child(
            // 状态点和 × 占同一个位置：悬停时点让位给 ×（原型 `.tab:hover .dot`）。
            div()
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(CLOSE_BUTTON)
                .children(
                    render_status_dot(("tab-dot", id.0 as usize).into(), tab.dot, tokens).map(
                        |dot| {
                            div()
                                .absolute()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .group_hover("tab", |style| style.invisible())
                                .child(dot)
                        },
                    ),
                )
                .child(render_close_button(id, target, tokens)),
        )
}

/// 标签右端的 ×：只在悬停这个标签时出现，点了关闭整个标签。
fn render_close_button(id: TabId, target: &FocusHandle, tokens: &Tokens) -> impl IntoElement {
    let target = target.clone();
    let pressed = tokens.pressed;
    let text = tokens.text;
    div()
        .id(("tab-close", id.0 as usize))
        .test_support()
        .aria_label(t!("tabs.close"))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(CLOSE_BUTTON)
        .rounded(px(9.))
        .text_color(tokens.icon)
        .invisible()
        .group_hover("tab", |style| style.visible())
        .hover(move |style| style.bg(pressed).text_color(text))
        .on_click(move |_, window, cx| {
            // 不让外面的标签再把自己切到前面。
            cx.stop_propagation();
            target.dispatch_action(&CloseTab(id), window, cx);
        })
        .child(Icon::new(CatalogIcon::Close).size(px(12.)))
}

fn render_separator(hidden: bool, tokens: &Tokens) -> impl IntoElement {
    div()
        .flex_none()
        .w(SEPARATOR_WIDTH)
        .h(px(16.))
        .bg(tokens.separator)
        .when(hidden, |this| this.invisible())
}
