// 来自 since2006/shell-rs @ b2a307a: src/app/quit.rs
//! 退出保护（工单 23）：退出会丢东西（未保存的修改、未完成的传输）时先问。
//!
//! 放在全局量里，不挂在窗口壳的元素上：对话框关掉以后焦点是空的，action 只到窗口的根，
//! 到不了窗口壳；全局监听不管焦点在哪都听得到。

use std::rc::Rc;

use gpui_kit::*;

type Ask = Rc<dyn Fn(&mut Window, &mut App) -> bool>;

/// 问窗口能不能退出。`ask` 返回 true 是拦下了：它已经问了用户，用户同意时自己退出。
struct QuitGuard {
    window: AnyWindowHandle,
    ask: Ask,
}

impl Global for QuitGuard {}

/// 从现在起，退出都先在 `window` 里过一遍 `ask`。
pub fn set_quit_guard(
    window: AnyWindowHandle,
    ask: impl Fn(&mut Window, &mut App) -> bool + 'static,
    cx: &mut App,
) {
    cx.set_global(QuitGuard {
        window,
        ask: Rc::new(ask),
    });
}

/// `Quit` action：退出，除非被拦下。
pub(super) fn quit(cx: &mut App) {
    let Some(guard) = cx.try_global::<QuitGuard>() else {
        cx.quit();
        return;
    };
    let (window, ask) = (guard.window, guard.ask.clone());
    // action 派发时窗口正在更新，等更新完才能问。
    cx.defer(move |cx| {
        let held = window
            .update(cx, |_, window, cx| ask(window, cx))
            .unwrap_or(false);
        if !held {
            cx.quit();
        }
    });
}

/// 在窗口里问：退出被拦下了吗。关窗口（系统的关闭按钮、Alt+F4、我们自己的 ×）走这里。
pub fn quit_held_back(window: &mut Window, cx: &mut App) -> bool {
    match cx.try_global::<QuitGuard>().map(|guard| guard.ask.clone()) {
        Some(ask) => ask(window, cx),
        None => false,
    }
}
