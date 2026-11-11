//! 对话框的层：按身份打开、关闭（spec 第 4 节，M2 code review S1）。
//!
//! gpui-component 的 `close_dialog` 只弹最上面那层、也不调那层的 `on_close`。对话框在后台算完
//! （解锁的 Argon2、重置、设主密码）再关自己时，连接的问答框可能已经叠在上面：照旧调 `close_dialog`
//! 就把问答框关掉了，回答丢失、连接卡在「正在连接」。
//!
//! 这里给每层一个 [`DialogLayer`]：本窗口的对话框一律经它打开、关闭，记着自己在第几层。关的是最上面那层
//! 就直接弹掉；关的是下面的层，就先把上面的层收起、关掉自己、再把上面的层原样叠回去（构造函数还在），
//! 焦点还给原来的地方。Esc、点遮罩这类 gpui-component 自己关掉的，经 `on_close` 从记录里去掉，
//! 再交给调用方的 `dismissed`。
//!
//! 约定：窗口里的对话框都经这里打开和关闭，不直接调 `open_dialog` / `close_dialog`，否则记录对不上。
//!
//! 对话框开着时别处要焦点（新标签、分屏的新卡片，工单 32），经 [`focus_after_dialogs`]：不抢框里的焦点，
//! 等这个窗口的对话框都关了再给。

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::dialog::{ANIMATION_DURATION, Dialog};
use gpui_kit::*;

/// 每次渲染时把 `Dialog` 搭成这层的样子。
type Build = Rc<dyn Fn(Dialog, &mut Window, &mut App) -> Dialog>;

/// 被 Esc、遮罩等关掉时要做的事。
type Dismissed = Rc<dyn Fn(&mut Window, &mut App)>;

/// 一层对话框的身份。先 [`DialogLayer::new`] 拿到身份（对话框的视图存着它，好关自己），
/// 再 [`DialogLayer::open`] 叠上去。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogLayer {
    id: u64,
}

impl Default for DialogLayer {
    fn default() -> Self {
        Self::new()
    }
}

struct Layer {
    id: u64,
    build: Build,
}

/// 每个窗口开着哪些层，从下往上。
#[derive(Default)]
struct DialogLayers(HashMap<WindowId, Vec<Layer>>);

impl Global for DialogLayers {}

fn window_id(window: &Window) -> WindowId {
    window.window_handle().window_id()
}

impl DialogLayer {
    /// 一个新身份，还没打开。
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// 叠到最上面。`build` 每次渲染都调。
    pub fn open(
        &self,
        build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.open_dismissable(build, |_, _| {}, window, cx);
    }

    /// 叠到最上面；被 Esc、遮罩这类 gpui-component 自己的途径关掉时调 `dismissed`
    /// （经 [`DialogLayer::close`] 关时不调）。
    pub fn open_dismissable(
        &self,
        build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
        dismissed: impl Fn(&mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.is_open(window, cx) {
            return;
        }
        let id = self.id;
        let dismissed: Dismissed = Rc::new(dismissed);
        let build: Build = Rc::new(move |dialog, window, cx| {
            let dismissed = dismissed.clone();
            build(dialog, window, cx).on_close(move |_, window, cx| {
                forget(id, window, cx);
                dismissed(window, cx);
            })
        });
        let wid = window_id(window);
        cx.default_global::<DialogLayers>()
            .0
            .entry(wid)
            .or_default()
            .push(Layer {
                id,
                build: build.clone(),
            });
        window.open_dialog(cx, move |dialog, window, cx| build(dialog, window, cx));
    }

    /// 这层还开着。
    pub fn is_open(&self, window: &Window, cx: &App) -> bool {
        cx.try_global::<DialogLayers>()
            .and_then(|layers| layers.0.get(&window_id(window)))
            .is_some_and(|stack| stack.iter().any(|layer| layer.id == self.id))
    }

    /// 只关这一层，上面叠着的层原样留着。已经关了就什么都不做。
    pub fn close(&self, window: &mut Window, cx: &mut App) {
        let wid = window_id(window);
        let Some(stack) = cx
            .try_global::<DialogLayers>()
            .and_then(|layers| layers.0.get(&wid))
        else {
            return;
        };
        let Some(index) = stack.iter().position(|layer| layer.id == self.id) else {
            return;
        };
        let above = stack.len() - index - 1;
        let reopened: Vec<(u64, Build)> = stack[index + 1..]
            .iter()
            .map(|layer| (layer.id, layer.build.clone()))
            .collect();
        // 焦点在最上面那层里：叠回去以后还给它。
        let focused = (above > 0).then(|| window.focused(cx)).flatten();
        if let Some(stack) = cx.global_mut::<DialogLayers>().0.get_mut(&wid) {
            stack.truncate(index);
        }
        for _ in 0..=above {
            window.close_dialog(cx);
        }
        for (id, build) in reopened {
            cx.global_mut::<DialogLayers>()
                .0
                .entry(wid)
                .or_default()
                .push(Layer {
                    id,
                    build: build.clone(),
                });
            window.open_dialog(cx, move |dialog, window, cx| build(dialog, window, cx));
        }
        if let Some(focused) = focused {
            window.focus(&focused, cx);
        }
        if above == 0 && !any_open(window, cx) {
            focus_pending(window, cx);
        }
    }
}

/// gpui-component 自己关掉了第 `id` 层（Esc、遮罩）：从记录里去掉。
fn forget(id: u64, window: &mut Window, cx: &mut App) {
    let wid = window_id(window);
    if !cx.has_global::<DialogLayers>() {
        return;
    }
    if let Some(stack) = cx.global_mut::<DialogLayers>().0.get_mut(&wid) {
        stack.retain(|layer| layer.id != id);
    }
    if !any_open(window, cx) && has_pending(window, cx) {
        // gpui-component 关完动画（`ANIMATION_DURATION`）后才把焦点还给打开前的地方：等它还完再交出去。
        let wait = *ANIMATION_DURATION + Duration::from_millis(1);
        window
            .spawn(cx, async move |cx| {
                cx.background_executor().timer(wait).await;
                cx.update(|window, cx| {
                    if !any_open(window, cx) {
                        focus_pending(window, cx);
                    }
                })
                .ok();
            })
            .detach();
    }
}

/// 这个窗口有没有开着的对话框（经 [`DialogLayer`] 打开的）。
pub fn any_open(window: &Window, cx: &App) -> bool {
    cx.try_global::<DialogLayers>()
        .and_then(|layers| layers.0.get(&window_id(window)))
        .is_some_and(|stack| !stack.is_empty())
}

/// 等对话框都关了再拿焦点的地方，每个窗口一个（后来的顶替先来的）。
#[derive(Default)]
struct PendingFocus(HashMap<WindowId, WeakFocusHandle>);

impl Global for PendingFocus {}

/// 把焦点交给 `focus`（工单 32）：没有对话框开着就立刻给；开着（比如连接的问答框、解锁框）就不抢，
/// 等这个窗口的对话框都关了再给它。等的时候又有别的地方要焦点，给后来的那个。
/// 新标签、分屏这类「新卡片拿焦点」都经这里，用户正在框里打字时不会被打断。
pub fn focus_after_dialogs(focus: &FocusHandle, window: &mut Window, cx: &mut App) {
    if !any_open(window, cx) {
        window.focus(focus, cx);
        return;
    }
    cx.default_global::<PendingFocus>()
        .0
        .insert(window_id(window), focus.downgrade());
}

fn has_pending(window: &Window, cx: &App) -> bool {
    cx.try_global::<PendingFocus>()
        .is_some_and(|pending| pending.0.contains_key(&window_id(window)))
}

/// 对话框都关了：把等着的焦点交出去（那张卡片已经关了就算了）。
fn focus_pending(window: &mut Window, cx: &mut App) {
    if !cx.has_global::<PendingFocus>() {
        return;
    }
    let pending = cx.global_mut::<PendingFocus>().0.remove(&window_id(window));
    if let Some(focus) = pending.and_then(|focus| focus.upgrade()) {
        window.focus(&focus, cx);
    }
}
