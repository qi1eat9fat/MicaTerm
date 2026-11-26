// 参考 since2006/shell-rs @ b2a307a: src/host/host_panel.rs 重写
// 裁剪：主机平铺，没有分组树（不用 gpui-component 的 TreeState）；行的样式按全部界面原型的 `.host`；
// 右键菜单不用 gpui-component 的 `context_menu`（它会泄漏菜单实体，照 `TabRail` 自己开关）。

//! 主机栏里的搜索框和平铺主机列表。
//!
//! - 每台一行 36 高，行和行之间空 4：左边 28 的系统徽章，右边名称。徽章和状态标记都不随
//!   连接状态变（2026-11-24 用户定：主机栏不再显示连接状态）。地址放进悬停提示
//!   （`用户@地址:端口` 和备注）；
//! - 单击选中，双击或选中后回车连接（派发 `ConnectHost`）；选中项是 `--sel` 底，悬停 `--hover` 底；
//! - 搜索框常驻在列表上面，Ctrl+Shift+H 聚焦它。按名称、地址、用户名搜索，
//!   空格隔开的几个词都要匹配；自动高亮第一个，↑↓ 移动，回车连接高亮的那台，Esc 清空，
//!   空着再按 Esc 焦点回到列表；
//! - 不在搜索时可以拖动排序，有插入线；搜索时暂停拖动；
//! - 右键菜单见 `menu`。
//!
//! 主机数据在 `HostStore`，这里只持有界面状态（搜索词、选中、高亮、拖动、开着的菜单），
//! 观察 store 变化：新建或复制出来的主机自动选上。

use std::collections::HashSet;

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use micaterm_ui::{Menu, WINDOW_MARGIN};

use micaterm_hosts::{Host, HostDrop, HostId, host_matches};

use super::mark::HostMark;
use super::menu::build_host_menu;
use super::store::HostStore;
use crate::app::{ConnectHost, MoveHost, NewHost};
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale, WEIGHT_NORMAL, size, space};
use crate::ui::empty::{EmptyPlacement, EmptyState};
use crate::ui::search::{SearchBar, SearchChrome};
use crate::ui::drag::{DragSlot, DropEdge, fade_while_dragged, render_drop_line};
use crate::ui::icon_button::TextTooltip;

/// 一台主机一行的高。
const ROW_HEIGHT: Pixels = size::SIDEBAR_ROW;
/// 名称一行的行高。
const NAME_LINE: Pixels = px(18.);
/// 徽章边长。
const BADGE: Pixels = size::HOST_BADGE;
/// 搜索行里「新建主机」的加号边长、图标边长。
const SEARCH_ADD: Pixels = size::CONTROL_SM;
const SEARCH_ADD_ICON: Pixels = size::ICON_SM;
/// 列表的键上下文：选中一行后回车连接。
const HOST_LIST_KEY_CONTEXT: &str = "HostList";

/// 主机栏的列表部分。窗口壳建一个，放在主机栏的「主机 N」标题下面。
pub struct HostList {
    store: Entity<HostStore>,
    /// 窗口壳的焦点：菜单和连接都经它派发，送得到窗口壳的处理器。
    target: FocusHandle,
    /// 列表自己的焦点：点了一行之后回车连接选中的那台。
    focus_handle: FocusHandle,
    search: Entity<InputState>,
    query: String,
    selected: Option<HostId>,
    /// 搜索时高亮的是第几个匹配项。
    highlighted: usize,
    /// 上一次见过的主机：多出来的就是刚新建或复制的，自动选上。
    seen: HashSet<HostId>,
    /// 正在拖动的主机和它要放下的位置。
    drag: DragSlot<DragState>,
    menu: Option<OpenMenu>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DragState {
    source: HostId,
    drop: HostDrop,
}

/// 开着的右键菜单。菜单实体由列表持有，关菜单时一起放掉。
struct OpenMenu {
    host: HostId,
    position: Point<Pixels>,
    view: Entity<Menu>,
    _dismiss: Subscription,
}

impl HostList {
    pub fn new(
        store: Entity<HostStore>,
        target: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(t!("host.list.search")));
        let subscriptions = vec![
            cx.observe_in(&store, window, |this, _, window, cx| {
                this.on_store_changed(window, cx)
            }),
            cx.subscribe_in(
                &search,
                window,
                |this, state, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.query = state.read(cx).value().to_string();
                        this.highlighted = 0;
                        this.scroll.scroll_to_item(0);
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => {
                        if let Some(id) = this.highlighted_host(cx) {
                            this.target.dispatch_action(&ConnectHost(id), window, cx);
                        }
                    }
                    _ => {}
                },
            ),
        ];
        let seen = store.read(cx).hosts().iter().map(|host| host.id).collect();
        Self {
            store,
            target,
            focus_handle: cx.focus_handle(),
            search,
            query: String::new(),
            selected: None,
            highlighted: 0,
            seen,
            drag: DragSlot::default(),
            menu: None,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// 选中的主机。
    pub fn selected_host(&self) -> Option<HostId> {
        self.selected
    }

    /// 搜索时高亮的那台主机；不在搜索或没有匹配时是 `None`。
    pub fn highlighted_host(&self, cx: &App) -> Option<HostId> {
        if !self.searching() {
            return None;
        }
        self.listed_hosts(cx).get(self.highlighted).copied()
    }

    /// 列表里显示的主机，从上到下：搜索时只有匹配的。
    pub fn listed_hosts(&self, cx: &App) -> Vec<HostId> {
        self.store
            .read(cx)
            .hosts()
            .iter()
            .filter(|host| host_matches(host, &self.query))
            .map(|host| host.id)
            .collect()
    }

    /// 把焦点给搜索框（Ctrl+Shift+H）。
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
        cx.notify();
    }

    /// 离开搜索框（空着按 Esc），焦点回到列表。
    fn leave_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// 选中一台主机并滚进视野。
    pub fn select(&mut self, id: HostId, cx: &mut Context<Self>) {
        self.selected = Some(id);
        if let Some(index) = self.listed_hosts(cx).iter().position(|host| *host == id) {
            self.scroll.scroll_to_item(index);
        }
        cx.notify();
    }

    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// store 变了：新出现的主机（刚新建或复制的）选上；搜索词挡住它时清掉搜索。
    fn on_store_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (hosts, created) = {
            let store = self.store.read(cx);
            let hosts: Vec<HostId> = store.hosts().iter().map(|host| host.id).collect();
            let created = hosts
                .iter()
                .rev()
                .find(|id| !self.seen.contains(id))
                .copied();
            (hosts, created)
        };
        self.seen = hosts.iter().copied().collect();
        if self.selected.is_some_and(|id| !self.seen.contains(&id)) {
            self.selected = None;
        }
        if let Some(id) = created {
            let hidden = self
                .store
                .read(cx)
                .host(id)
                .is_some_and(|host| !host_matches(host, &self.query));
            if hidden {
                self.clear_search(window, cx);
            }
            self.select(id, cx);
        }
        let count = self.listed_hosts(cx).len();
        self.highlighted = self.highlighted.min(count.saturating_sub(1));
        cx.notify();
    }

    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.query.clear();
        self.highlighted = 0;
        cx.notify();
    }

    /// 搜索时 ↑↓ 移动高亮。
    fn move_highlight(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self.listed_hosts(cx).len();
        if count == 0 {
            return;
        }
        let next = (self.highlighted as isize + step).clamp(0, count as isize - 1) as usize;
        if next != self.highlighted {
            self.highlighted = next;
            self.scroll.scroll_to_item(next);
            cx.notify();
        }
    }

    /// 在 `position`（窗口坐标）为 `host` 开右键菜单；之前开着的那个放掉。选中项不变。
    fn open_menu(
        &mut self,
        host: HostId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self
            .store
            .read(cx)
            .host(host)
            .cloned()
            .map(|host| build_host_menu(&host, &self.target, window, cx))
        else {
            return;
        };
        let dismiss = cx.subscribe_in(
            &view,
            window,
            |this, menu: &Entity<Menu>, _: &DismissEvent, window, cx| {
                if this.menu.as_ref().is_some_and(|open| &open.view == menu) {
                    this.menu = None;
                    // 菜单项把焦点交给了别处（分屏打开的新卡片、对话框）就不抢回来。
                    let elsewhere = window.focused(cx).is_some_and(|focused| {
                        focused != this.target
                            && focused != this.focus_handle
                            && !menu.focus_handle(cx).contains_focused(window, cx)
                    });
                    if !elsewhere {
                        window.focus(&this.focus_handle, cx);
                    }
                }
                cx.notify();
            },
        );
        window.focus(&view.focus_handle(cx), cx);
        // 选中项不变：被右键的那一行只描一圈细边（26 号「主机右键」，用户定）。
        self.menu = Some(OpenMenu {
            host,
            position,
            view,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    /// 选中一行之后的按键：回车连接选中的那台。
    fn on_list_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "enter"
            && !event.keystroke.modifiers.modified()
            && let Some(id) = self.selected
        {
            self.target.dispatch_action(&ConnectHost(id), window, cx);
            cx.stop_propagation();
        }
    }

    /// 搜索框（空壳的侧栏搜索）：高 22、前景 6% 的底、圆角 6，没有描边；右边是新建主机的加号。
    /// 加号和搜索框同高，方框再小一点，不显得比搜索框重。
    fn render_search(
        &self,
        tokens: &Tokens,
        scale: TypeScale,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let focused = self.search.focus_handle(cx).is_focused(window);
        let searching = self.searching();
        let target = self.target.clone();
        let search = SearchBar::new(
            "host-search-box",
            &self.search,
            "host-search",
            t!("host.list.search"),
            "host-search-clear",
            t!("host.list.clear_search"),
            SearchChrome::Filled,
            tokens,
            scale,
            cx.listener(|this, _, window, cx| {
                this.clear_search(window, cx);
                this.focus_search(window, cx);
            }),
        )
        .query(searching)
        .focused(focused)
        .grow(true)
        .render()
        // 搜索框里的 ↑↓、Esc 先到这里：移动高亮、清空或收起搜索，不交给输入框，也不冒泡到终端。
        .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
            if this.searching() {
                this.move_highlight(-1, cx);
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
            if this.searching() {
                this.move_highlight(1, cx);
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            if this.searching() {
                this.clear_search(window, cx);
            } else {
                this.leave_search(window, cx);
            }
            cx.stop_propagation();
        }));
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(space::SPACE_5)
            .mt(space::SPACE_2)
            .child(search)
            .child(
                div()
                    .id("host-sidebar-new-host")
                    .test_support()
                    .aria_label(t!("workspace.host_sidebar.new_host"))
                    .tooltip(|_, cx| TextTooltip::view(t!("workspace.host_sidebar.new_host"), cx))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(SEARCH_ADD)
                    .rounded(size::RADIUS_MD)
                    .bg(tokens.hover)
                    .cursor_pointer()
                    .hover(move |style| style.bg(tokens.sel))
                    .on_click(move |_, window, cx| target.dispatch_action(&NewHost, window, cx))
                    .child(
                        Icon::new(IconName::Plus)
                            .size(SEARCH_ADD_ICON)
                            .text_color(tokens.st_dim),
                    ),
            )
    }

    fn render_row(
        &self,
        host: &Host,
        index: usize,
        tokens: &Tokens,
        scale: TypeScale,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = host.id;
        let searching = self.searching();
        let selected = !searching && self.selected == Some(id);
        let highlighted = searching && self.highlighted == index;
        let pressed = self.menu.as_ref().is_some_and(|menu| menu.host == id);
        let drag = self.drag.get();
        let dragged = drag.is_some_and(|drag| drag.source == id);
        let line = drag.and_then(|drag| match drag.drop {
            _ if drag.source == id => None,
            HostDrop::Before(target) if target == id => Some(DropEdge::Top),
            HostDrop::After(target) if target == id => Some(DropEdge::Bottom),
            _ => None,
        });
        let hover = tokens.hover;
        let mark = HostMark::of(host);
        let endpoint: SharedString = host.endpoint().into();
        let notes = host.notes.clone();
        let target = self.target.clone();
        let dragged_host = DraggedHost {
            id,
            name: host.name.clone(),
        };
        div()
            .id(("host-row", id.0 as usize))
            .test_support()
            .aria_label(host.name.clone())
            .aria_selected(selected || highlighted)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(space::SPACE_8)
            .h(ROW_HEIGHT)
            .px(space::SPACE_4)
            .rounded(size::RADIUS_MD)
            .text_size(scale.body)
            .font_weight(if selected || highlighted {
                WEIGHT_NORMAL
            } else {
                WEIGHT_NORMAL
            })
            .text_color(tokens.label)
            .cursor_pointer()
            .map(|row| {
                if selected || highlighted {
                    row.bg(tokens.sel).text_color(tokens.text)
                } else {
                    row.hover(move |style| style.bg(hover))
                }
            })
            // 被右键的那一行描一圈细边，选中项不变（26 号主机右键菜单）。
            .when(pressed, |row| {
                row.border_1().border_color(tokens.line_strong)
            })
            .map(|row| fade_while_dragged(row, dragged))
            // 开着右键菜单时不出悬停提示，免得盖住菜单。
            .when(self.menu.is_none(), |row| {
                row.tooltip(move |_, cx| {
                    let detail = (!notes.is_empty()).then(|| notes.clone());
                    TextTooltip::with_detail("host-tooltip", endpoint.clone(), detail, cx)
                })
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                window.focus(&this.focus_handle, cx);
                this.selected = Some(id);
                cx.notify();
                if event.click_count() == 2 {
                    target.dispatch_action(&ConnectHost(id), window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_menu(id, event.position, window, cx);
                    // 焦点已经给了菜单：别让外面列表的 track_focus 在冒泡时又把它抢回去，
                    // 否则 Esc 到不了菜单。
                    window.prevent_default();
                }),
            )
            .when(!searching, |row| {
                row.on_drag(dragged_host, |drag, _, _, cx| cx.new(|_| drag.clone()))
                    .on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<DraggedHost>, _, cx| {
                            if !event.bounds.contains(&event.event.position) {
                                return;
                            }
                            let source = event.drag(cx).id;
                            let relative = (event.event.position.y - event.bounds.top())
                                / event.bounds.size.height;
                            let drop = if relative < 0.5 {
                                HostDrop::Before(id)
                            } else {
                                HostDrop::After(id)
                            };
                            let state = Some(DragState { source, drop });
                            if this.drag.set(state) {
                                cx.notify();
                            }
                        },
                    ))
                    .on_drop(cx.listener(move |this, drag: &DraggedHost, window, cx| {
                        if let Some(state) = this.drag.take()
                            && state.source == drag.id
                            && drag.id != id
                        {
                            this.target.dispatch_action(
                                &MoveHost {
                                    host: drag.id,
                                    drop: state.drop,
                                },
                                window,
                                cx,
                            );
                        }
                        cx.notify();
                    }))
            })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(BADGE)
                    .child(mark.render(
                        ("host-badge", id.0 as usize).into(),
                        BADGE,
                        size::HOST_BADGE_GLYPH,
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .line_height(NAME_LINE)
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(host.name.clone()),
            )
            .children(line.map(|edge| {
                let id = ("host-drop-line", id.0 as usize).into();
                render_drop_line(id, edge, px(12.), px(0.), tokens)
            }))
    }
}

impl Focusable for HostList {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HostList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 拖动在别处放下或取消了：插入线和变淡都收掉。
        self.drag.settle(cx);
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let hosts: Vec<Host> = self
            .store
            .read(cx)
            .hosts()
            .iter()
            .filter(|host| host_matches(host, &self.query))
            .cloned()
            .collect();
        let total = self.store.read(cx).hosts().len();
        let rows: Vec<AnyElement> = hosts
            .iter()
            .enumerate()
            .map(|(index, host)| {
                self.render_row(host, index, &tokens, scale, cx)
                    .into_any_element()
            })
            .collect();
        let list = if total == 0 {
            render_empty(t!("workspace.host_sidebar.empty"), true, &tokens, scale)
                .into_any_element()
        } else if rows.is_empty() {
            render_empty(
                t!("host.list.no_match", query = self.query.trim()),
                false,
                &tokens,
                scale,
            )
            .into_any_element()
        } else {
            div()
                .id("host-rows")
                .key_context(HOST_LIST_KEY_CONTEXT)
                .track_focus(&self.focus_handle)
                .on_key_down(cx.listener(Self::on_list_key))
                .flex()
                .flex_col()
                .gap(space::SPACE_2)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .children(rows)
                .into_any_element()
        };
        div()
            .id("host-list")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px(space::SPACE_6)
            .child(self.render_search(&tokens, scale, window, cx))
            // 搜索框和第一台主机之间留 6。
            .child(div().flex_none().h(px(6.)))
            .child(list)
            .children(self.menu.as_ref().map(|menu| {
                deferred(
                    anchored()
                        .position(menu.position)
                        .snap_to_window_with_margin(WINDOW_MARGIN)
                        .child(menu.view.clone()),
                )
                .with_priority(POPUP_PRIORITY)
            }))
    }
}

/// 列表里没有行时的那句话。一台主机都没有时（`centered`）居中、偏上一点（26 号 `.se`）；
/// 搜索没有匹配时贴在上面。
fn render_empty(
    text: SharedString,
    centered: bool,
    tokens: &Tokens,
    scale: TypeScale,
) -> impl IntoElement {
    let placement = if centered {
        EmptyPlacement::HostCentered
    } else {
        EmptyPlacement::HostTop
    };
    EmptyState::new("host-list-empty", text, placement).render(tokens, scale)
}

/// 拖着的主机，和跟着鼠标的那块预览。
#[derive(Clone)]
struct DraggedHost {
    id: HostId,
    name: SharedString,
}

impl Render for DraggedHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        div()
            .px(space::SPACE_10)
            .py(space::SPACE_4)
            .rounded(size::RADIUS_MD)
            .bg(tokens.popup)
            .border_1()
            .border_color(tokens.line_strong)
            .text_size(TypeScale::current(cx).body)
            .text_color(tokens.text)
            .child(self.name.clone())
    }
}

