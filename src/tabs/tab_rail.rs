//! 标签轨道（原型 `.track`）和里面的标签（`.tab`）：宽固定 180、高 28，叠放的程序图块、名称、目录和状态点。
//! 点标签切过去；悬停时状态点让位给 ×，点了关闭整个标签；右键菜单三项；放不下时轨道横向滚动。
//! 按住标签左右拖动改顺序（工单 29）：标题栏里有插入线，拖到轨道两端自动横向滚动，
//! 拖过轨道右端（`+`、拖动区）算放到最后；拖动中不切换在前的标签。
//! 渲染不读实体，只拿窗口壳取好的快照；点击、菜单、拖动排序都经窗口壳的焦点派发 action。
// 参考 gorex 的模型重写：tabs.go 的标签外观、悬停时出现 ×。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{TabDrop, TabId};
use crate::app::{
    ActivateTab, CatalogIcon, CloseOtherTabs, CloseTab, CloseTabsToRight, MoveTab, OpenTabMenu,
};
use crate::card::{StatusDot, render_status_dot, render_tile_stack};
use crate::i18n::t;
use crate::programs::Program;
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

/// 标签轨道跨帧要记住的东西：横向滚动的位置、上一次露出来的在前标签、开着的右键菜单、拖动。
#[derive(Default)]
pub struct TabRail {
    scroll: ScrollHandle,
    /// 在前的标签一换，就把它滚进视野；没换时不动，用户自己滚开的位置保留。
    revealed: Cell<Option<TabId>>,
    /// 下一帧不管换没换，都把在前的标签滚进视野（又切到它一次时）。
    reveal_requested: Cell<bool>,
    menu: Option<OpenMenu>,
    /// 正在拖的标签：标题栏的拖动监听写，渲染时读来画插入线、自动滚动。
    drag: Rc<Cell<Option<TabDrag>>>,
    /// 上一帧画的标签顺序：松开时按它算放到第几个。
    order: Rc<RefCell<Vec<TabId>>>,
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

/// 拖动中的一刻：拖着哪个标签，鼠标在哪里（窗口坐标），在不在标题栏里。
#[derive(Clone, Copy, Debug, PartialEq)]
struct TabDrag {
    source: TabId,
    pointer: Point<Pixels>,
    over_title_bar: bool,
}

/// 标签之间的竖线（`.tsep`）。
const SEPARATOR_WIDTH: Pixels = px(1.);
/// × 按钮的边长（`.tab .x`）。
const CLOSE_BUTTON: Pixels = px(18.);
/// 拖到离轨道两端这么近（或者过了两端）时自动横向滚动。
const AUTO_SCROLL_EDGE: Pixels = px(32.);
/// 自动滚动每帧滚多少。
const AUTO_SCROLL_STEP: Pixels = px(8.);

impl TabRail {
    /// 轨道（底色圆角条）和里面的标签；没有标签时什么都不画。
    /// `target` 是窗口壳的焦点：点击和菜单都经它派发，焦点在哪里都送得到窗口壳。
    /// 拖着标签时顺带自动滚动轨道（要下一帧时向 `window` 要）。
    pub fn render(
        &self,
        tabs: Vec<TabSnapshot>,
        target: &FocusHandle,
        tokens: &Tokens,
        window: &Window,
        cx: &App,
    ) -> Option<impl IntoElement + use<>> {
        // 拖动在别处放下或取消了：插入线和变淡都收掉。
        if !cx.has_active_drag() {
            self.drag.set(None);
        }
        *self.order.borrow_mut() = tabs.iter().map(|tab| tab.id).collect();
        if tabs.is_empty() {
            self.revealed.set(None);
            return None;
        }
        self.reveal_active(&tabs);
        let active = tabs.iter().position(|tab| tab.active);
        let menu_tab = self.menu_tab();
        let count = tabs.len();
        let drag = self.drag.get();
        let line = drag.and_then(|drag| {
            let order = self.order.borrow();
            let drop = self.follow_drag(drag, &order, window)?;
            // 放回原处不画线。
            drop.destination(&order, drag.source)?;
            line_position(drop, &order)
        });
        let mut children = Vec::with_capacity(count * 2);
        for (index, tab) in tabs.into_iter().enumerate() {
            let look = TabLook {
                pressed: menu_tab == Some(tab.id),
                dragged: drag.is_some_and(|drag| drag.source == tab.id),
                line: line.and_then(|(at, edge)| (at == index).then_some(edge)),
            };
            children.push(render_tab(tab, look, target, tokens).into_any_element());
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

    /// 让标题栏接住拖着的标签（工单 29）：在标题栏里移动时记下位置（插入线、自动滚动看它），
    /// 在标题栏里松开时按松开的位置派发 `MoveTab`。拖动区本身不挂任何处理器，照常拖窗口。
    pub fn accept_drops(&self, bar: Stateful<Div>, target: &FocusHandle) -> Stateful<Div> {
        let drag = self.drag.clone();
        let dropped = self.drag.clone();
        let scroll = self.scroll.clone();
        let order = self.order.clone();
        let target = target.clone();
        bar.on_drag_move(move |event: &DragMoveEvent<DraggedTab>, _, cx| {
            let pointer = event.event.position;
            drag.set(Some(TabDrag {
                source: event.drag(cx).tab.id,
                pointer,
                over_title_bar: event.bounds.contains(&pointer),
            }));
        })
        .on_drop(move |dragged: &DraggedTab, window, cx| {
            dropped.set(None);
            let tab = dragged.tab.id;
            let drop = drop_at(window.mouse_position().x, &scroll, &order.borrow());
            if let Some(drop) = drop {
                target.dispatch_action(&MoveTab { tab, drop }, window, cx);
            }
        })
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

    /// 拖着标签的这一帧：鼠标在轨道两端就滚一步、要下一帧接着滚；返回此刻松开会放到哪里。
    /// 鼠标不在标题栏里时松开就取消，返回 `None`。
    fn follow_drag(&self, drag: TabDrag, order: &[TabId], window: &Window) -> Option<TabDrop> {
        if !drag.over_title_bar {
            return None;
        }
        let track = self.scroll.bounds();
        let x = drag.pointer.x;
        let step = if x < track.left() + AUTO_SCROLL_EDGE {
            AUTO_SCROLL_STEP
        } else if x > track.right() - AUTO_SCROLL_EDGE {
            -AUTO_SCROLL_STEP
        } else {
            px(0.)
        };
        let offset = self.scroll.offset();
        let scrolled = (offset.x + step).clamp(-self.scroll.max_offset().x, px(0.));
        if scrolled != offset.x {
            self.scroll.set_offset(point(scrolled, offset.y));
            window.request_animation_frame();
        }
        drop_at(x, &self.scroll, order)
    }
}

/// 鼠标在 `x`（窗口坐标）时松开，标签放到哪里。`order` 是轨道里标签的顺序。
/// 停在某个标签上：左半边放在它前面，右半边放在它后面（看的是标签露出来的位置：布局位置加上滚动）；
/// 过了轨道右端（`+`、拖动区）放到最后，过了左端（「展开主机栏」）放到第一个。
fn drop_at(x: Pixels, scroll: &ScrollHandle, order: &[TabId]) -> Option<TabDrop> {
    let track = scroll.bounds();
    if x > track.right() {
        return Some(TabDrop::Last);
    }
    if x < track.left() {
        return Some(TabDrop::First);
    }
    let offset = scroll.offset().x;
    for (index, id) in order.iter().enumerate() {
        let bounds = scroll.bounds_for_item(index * 2)?;
        if x < bounds.center().x + offset {
            return Some(TabDrop::Before(*id));
        }
        if x <= bounds.right() + offset {
            return Some(TabDrop::After(*id));
        }
    }
    // 最后一个标签右边、轨道的内边距里。
    order.last().map(|id| TabDrop::After(*id))
}

/// 插入线画在第几个标签的哪一边。
fn line_position(drop: TabDrop, order: &[TabId]) -> Option<(usize, Edge)> {
    let index_of = |target: TabId| order.iter().position(|id| *id == target);
    match drop {
        TabDrop::Before(target) => Some((index_of(target)?, Edge::Left)),
        TabDrop::After(target) => Some((index_of(target)?, Edge::Right)),
        TabDrop::First => Some((0, Edge::Left)),
        TabDrop::Last => Some((order.len().checked_sub(1)?, Edge::Right)),
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

/// 插入线画在标签的哪一边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    Left,
    Right,
}

/// 标签随轨道状态变的样子。
struct TabLook {
    /// 这个标签的右键菜单开着（原型 `.tab.rc`）。
    pressed: bool,
    /// 正拖着它：变淡，跟着鼠标的是它的预览。
    dragged: bool,
    /// 插入线落在它的哪一边。
    line: Option<Edge>,
}

fn render_tab(
    tab: TabSnapshot,
    look: TabLook,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
    let hover = tokens.hover;
    let id = tab.id;
    let activate = target.clone();
    let open_menu = target.clone();
    let dot = tab.dot;
    let dragged = DraggedTab { tab: tab.clone() };
    render_tab_face(&tab, ("tab-tiles", id.0 as usize).into(), tokens)
        .id(("tab", id.0 as usize))
        .test_support()
        .group("tab")
        .aria_label(format!("{} {}", tab.name, tab.directory))
        .relative()
        .cursor_pointer()
        .when(tab.active, |this| {
            this.bg(tokens.tab_active).shadow(raised(tokens.shadow))
        })
        .when(!tab.active, |this| this.hover(move |style| style.bg(hover)))
        .when(look.pressed, |this| this.bg(tokens.pressed))
        .when(look.dragged, |this| this.opacity(0.45))
        // 按下标签不把焦点交给窗口壳：拖动排序（或拖出去取消）之后焦点还在原来的卡片里。
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .on_click(move |_, window, cx| {
            activate.dispatch_action(&ActivateTab(id), window, cx);
        })
        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
            let position = event.position;
            open_menu.dispatch_action(&OpenTabMenu { tab: id, position }, window, cx);
        })
        // 预览按住的位置跟着鼠标，看起来就是标签本身被拿起来。
        .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
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
                    render_status_dot(("tab-dot", id.0 as usize).into(), dot, tokens).map(|dot| {
                        div()
                            .absolute()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .group_hover("tab", |style| style.invisible())
                            .child(dot)
                    }),
                )
                .child(render_close_button(id, target, tokens)),
        )
        .children(look.line.map(|edge| render_drop_line(edge, tokens)))
}

/// 标签的外形和左边的内容（图块、名称、目录）：轨道里的标签和拖动预览共用。
fn render_tab_face(tab: &TabSnapshot, tiles: ElementId, tokens: &Tokens) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(9.))
        .w(TAB_WIDTH)
        .h(TAB_HEIGHT)
        .pl(px(6.))
        .pr(px(4.))
        .rounded(px(14.))
        .child(render_tile_stack(tiles, &tab.tiles, tokens))
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
                .child(div().flex_none().child(tab.name.clone()))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(tokens.faint)
                        .child(tab.directory.clone()),
                ),
        )
}

/// 在前的标签（和拖动预览）的浮起阴影。
fn raised(shadow: Hsla) -> Vec<BoxShadow> {
    vec![
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
    ]
}

/// 拖动时的插入线：竖在标签左边或右边的缝上（盖住竖线）。
fn render_drop_line(edge: Edge, tokens: &Tokens) -> impl IntoElement {
    div()
        .id("tab-drop-line")
        .test_support()
        .absolute()
        .top(px(4.))
        .bottom(px(4.))
        .w(px(2.))
        .rounded(px(1.))
        .bg(tokens.accent)
        .map(|line| match edge {
            Edge::Left => line.left(px(-1.5)),
            Edge::Right => line.right(px(-1.5)),
        })
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

/// 拖着的标签，和跟着鼠标的那块预览：标签本身的样子，浮起来。
#[derive(Clone)]
struct DraggedTab {
    tab: TabSnapshot,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let tab = TabSnapshot {
            active: true,
            ..self.tab.clone()
        };
        render_tab_face(&tab, ("tab-drag-tiles", tab.id.0 as usize).into(), &tokens)
            .bg(tokens.tab_active)
            .shadow(raised(tokens.shadow))
            .opacity(0.92)
    }
}
