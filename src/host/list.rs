// 参考 since2006/shell-rs @ b2a307a: src/host/host_panel.rs 重写
// 裁剪：主机平铺，没有分组树（不用 gpui-component 的 TreeState）；行的样式按全部界面原型的 `.host`；
// 右键菜单不用 gpui-component 的 `context_menu`（它会泄漏菜单实体，照 `TabRail` 自己开关）。

//! 主机栏里的搜索框和平铺主机列表（原型 `.search`、`.slist`、`.host`）。
//!
//! - 每行是系统徽章、名称，下一行 `user@地址[:端口]`；悬停提示 `用户@地址:端口` 和备注；
//! - 单击选中，双击或选中后回车连接（派发 `ConnectHost`）；选中项和悬停项是胶囊形底色；
//! - 搜索按名称、地址、用户名，空格隔开的几个词都要匹配；自动高亮第一个，↑↓ 移动，回车连接
//!   高亮的那台，Esc 清空；
//! - 不在搜索时可以拖动排序，有插入线；搜索时暂停拖动；
//! - 右键菜单见 `menu`。
//!
//! 主机数据在 `HostStore`，这里只持有界面状态（搜索词、选中、高亮、拖动、开着的菜单），
//! 观察 store 变化：新建或复制出来的主机自动选上。

use std::collections::HashSet;

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use qishell_hosts::{Host, HostDrop, HostId, host_matches};

use super::mark::HostMark;
use super::menu::build_host_menu;
use super::store::HostStore;
use crate::app::{ConnectHost, MoveHost};
use crate::i18n::t;
use crate::theme::Tokens;

/// 行高（`.host`）。
const ROW_HEIGHT: Pixels = px(42.);
/// 徽章直径（`.host .badge`）。
const BADGE: Pixels = px(24.);
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
    drag: Option<DragState>,
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
    view: Entity<PopupMenu>,
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
            drag: None,
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

    /// 焦点给搜索框（Ctrl+Shift+H）。
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| search.focus(window, cx));
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

    /// 在 `position`（窗口坐标）为 `host` 开右键菜单，顺带选中它；之前开着的那个放掉。
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
            |this, menu: &Entity<PopupMenu>, _: &DismissEvent, window, cx| {
                if this.menu.as_ref().is_some_and(|open| &open.view == menu) {
                    this.menu = None;
                    window.focus(&this.focus_handle, cx);
                }
                cx.notify();
            },
        );
        window.focus(&view.focus_handle(cx), cx);
        self.selected = Some(host);
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

    fn render_search(&self, tokens: &Tokens, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.search.focus_handle(cx).is_focused(window);
        let searching = self.searching();
        div()
            .id("host-search-box")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .mx(px(2.))
            .mb(px(8.))
            .h(px(30.))
            .px(px(8.))
            .rounded(px(8.))
            .text_size(px(12.5))
            .text_color(tokens.faint)
            .map(|search| {
                if focused {
                    search.bg(tokens.field_focused).shadow(vec![BoxShadow {
                        color: tokens.accent,
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(0.),
                        spread_radius: px(1.5),
                        inset: true,
                    }])
                } else {
                    search.bg(tokens.hover)
                }
            })
            // 搜索框里的 ↑↓、Esc 先到这里：移动高亮、清空搜索，不交给输入框，也不冒泡到终端。
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
                    cx.stop_propagation();
                }
            }))
            .child(Icon::new(IconName::Search).size(px(14.)))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.search)
                        .id("host-search")
                        .aria_label(t!("host.list.search"))
                        .appearance(false)
                        .small(),
                ),
            )
            .map(|search| {
                if searching {
                    let hover = tokens.pressed;
                    search.child(
                        div()
                            .id("host-search-clear")
                            .test_support()
                            .aria_label(t!("host.list.clear_search"))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(20.))
                            .rounded(px(7.))
                            .cursor_pointer()
                            .hover(move |style| style.bg(hover))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_search(window, cx);
                                this.focus_search(window, cx);
                            }))
                            .child(Icon::new(IconName::Close).size(px(12.))),
                    )
                } else {
                    search.child(render_kbd(search_shortcut(), tokens))
                }
            })
    }

    fn render_row(
        &self,
        host: &Host,
        index: usize,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = host.id;
        let searching = self.searching();
        let selected = !searching && self.selected == Some(id);
        let highlighted = searching && self.highlighted == index;
        let pressed = self.menu.as_ref().is_some_and(|menu| menu.host == id);
        let dragged = self.drag.is_some_and(|drag| drag.source == id);
        let line = self.drag.and_then(|drag| match drag.drop {
            _ if drag.source == id => None,
            HostDrop::Before(target) if target == id => Some(true),
            HostDrop::After(target) if target == id => Some(false),
            _ => None,
        });
        let hover = tokens.hover;
        let shadow = tokens.shadow;
        let mark = HostMark::of(host);
        let address = list_address(host);
        let tooltip = HostTooltip {
            endpoint: host.endpoint().into(),
            notes: host.notes.clone(),
        };
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
            .gap(px(9.))
            .h(ROW_HEIGHT)
            .pl(px(9.))
            .pr(px(12.))
            .rounded_full()
            .cursor_pointer()
            .map(|row| {
                if selected {
                    row.bg(tokens.tab_active).shadow(vec![
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
                } else if highlighted {
                    row.bg(tokens.selection)
                } else {
                    row.hover(move |style| style.bg(hover))
                }
            })
            .when(pressed && !selected, |row| row.bg(tokens.pressed))
            .when(dragged, |row| row.opacity(0.45))
            .tooltip(move |_, cx| cx.new(|_| tooltip.clone()).into())
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
                            if this.drag != state {
                                this.drag = state;
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
            .child(mark.render(("host-badge", id.0 as usize).into(), BADGE))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(relative(1.3))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(host.name.clone()),
                    )
                    .child(
                        div()
                            .id(("host-address", id.0 as usize))
                            .test_support()
                            .aria_label(address.clone())
                            .text_size(px(11.5))
                            .text_color(tokens.faint)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(address),
                    ),
            )
            .children(line.map(|before| {
                div()
                    .id(("host-drop-line", id.0 as usize))
                    .test_support()
                    .absolute()
                    .left(px(12.))
                    .right(px(12.))
                    .h(px(2.))
                    .rounded(px(2.))
                    .bg(tokens.accent)
                    .map(|line| {
                        if before {
                            line.top(px(-1.))
                        } else {
                            line.bottom(px(-1.))
                        }
                    })
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
        if !cx.has_active_drag() {
            self.drag = None;
        }
        let tokens = Tokens::current(cx);
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
            .map(|(index, host)| self.render_row(host, index, &tokens, cx).into_any_element())
            .collect();
        let list = if total == 0 {
            render_empty(
                t!("workspace.host_sidebar.empty"),
                Some(t!("workspace.host_sidebar.empty_hint")),
                &tokens,
            )
            .into_any_element()
        } else if rows.is_empty() {
            render_empty(
                t!("host.list.no_match", query = self.query.trim()),
                None,
                &tokens,
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
                .flex_1()
                .min_h_0()
                .pt(px(2.))
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
            .child(self.render_search(&tokens, window, cx))
            .child(list)
            .children(self.menu.as_ref().map(|menu| {
                deferred(
                    anchored()
                        .position(menu.position)
                        .snap_to_window_with_margin(px(8.))
                        .child(menu.view.clone()),
                )
                .with_priority(POPUP_PRIORITY)
            }))
    }
}

/// 列表里没有行时的那句话（原型 `.noresult`）。
fn render_empty(text: SharedString, hint: Option<SharedString>, tokens: &Tokens) -> impl IntoElement {
    div()
        .id("host-list-empty")
        .test_support()
        .aria_label(text.clone())
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
        .pt(px(24.))
        .px(px(10.))
        .text_size(px(12.5))
        .text_color(tokens.faint)
        .child(text)
        .when_some(hint, |empty, hint| {
            empty.child(div().text_size(px(11.5)).child(hint))
        })
}

/// 按键提示（原型 `.kbd`）。
fn render_kbd(keys: &'static str, tokens: &Tokens) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(5.))
        .bg(tokens.field)
        .border_1()
        .border_color(tokens.field_border)
        .text_size(px(11.))
        .text_color(tokens.muted)
        .whitespace_nowrap()
        .child(keys)
}

/// 搜索框里提示的快捷键：聚焦主机搜索（spec 第 8 节）。
fn search_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "\u{21e7}\u{2318}H"
    } else {
        "Ctrl+Shift+H"
    }
}

/// 名称下面那一行：`user@地址`，端口不是 22 时加 `:端口`（原型 `hAddr`）。
pub fn list_address(host: &Host) -> SharedString {
    if host.port == qishell_hosts::SSH_PORT {
        format!("{}@{}", host.user, host.address).into()
    } else {
        format!("{}@{}:{}", host.user, host.address, host.port).into()
    }
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
            .px(px(10.))
            .py(px(4.))
            .rounded(px(8.))
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.panel_border)
            .text_size(px(12.5))
            .text_color(tokens.text)
            .child(self.name.clone())
    }
}

/// 悬停主机时的提示（原型 `.tip`）：`用户@地址:端口`，有备注时下一行是备注。
#[derive(Clone)]
struct HostTooltip {
    endpoint: SharedString,
    notes: SharedString,
}

impl Render for HostTooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let label = if self.notes.is_empty() {
            self.endpoint.to_string()
        } else {
            format!("{}\n{}", self.endpoint, self.notes)
        };
        div()
            .id("host-tooltip")
            .test_support()
            .aria_label(label)
            .flex()
            .flex_col()
            .px(px(9.))
            .py(px(5.))
            .rounded(px(7.))
            .bg(tokens.tooltip)
            .text_color(tokens.tooltip_text)
            .text_size(px(11.5))
            .line_height(relative(1.45))
            .child(self.endpoint.clone())
            .when(!self.notes.is_empty(), |tip| tip.child(self.notes.clone()))
    }
}
