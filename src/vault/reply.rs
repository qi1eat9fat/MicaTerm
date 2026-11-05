//! 保险库对话框的共用件：回答只交一次的 [`Reply`]，和打开小对话框的 [`open_small_dialog`]。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

/// 对话框的回答：`true` 是做成了（设好了、解开了、重置了），`false` 是取消（包括 Esc）。
type Answer = Box<dyn FnOnce(bool, &mut Window, &mut App)>;

/// 对话框的回答只交一次：按钮交了，Esc 关对话框时就不再交。
#[derive(Clone)]
pub(super) struct Reply(Rc<RefCell<Option<Answer>>>);

impl Reply {
    pub(super) fn new(answer: impl FnOnce(bool, &mut Window, &mut App) + 'static) -> Self {
        Self(Rc::new(RefCell::new(Some(Box::new(answer)))))
    }

    /// 不关心回答。
    pub(super) fn ignored() -> Self {
        Self::new(|_, _, _| {})
    }

    pub(super) fn send(&self, done: bool, window: &mut Window, cx: &mut App) {
        let answer = self.0.borrow_mut().take();
        if let Some(answer) = answer {
            answer(done, window, cx);
        }
    }
}

/// 关掉最上面那层（就是这个对话框），再交回答。
pub(super) fn close_and_reply(reply: &Reply, done: bool, window: &mut Window, cx: &mut App) {
    window.close_dialog(cx);
    reply.send(done, window, cx);
}

/// 叠一个居中的小对话框：不点遮罩关闭；Esc 关掉时回答「取消」。焦点给 `focus`。
pub(super) fn open_small_dialog<V: Render>(
    view: Entity<V>,
    width: Pixels,
    reply: Reply,
    focus: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, _, _| {
        let reply = reply.clone();
        dialog
            .w(width)
            .p_0()
            .rounded(px(14.))
            .close_button(false)
            .overlay_closable(false)
            .on_close(move |_, window, cx| reply.send(false, window, cx))
            .child(view.clone())
    });
    window.focus(&focus, cx);
}
