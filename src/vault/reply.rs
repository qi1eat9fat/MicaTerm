//! 保险库对话框的共用件：回答只交一次的 [`Reply`]，和打开小对话框的 [`open_small_dialog`]。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::*;

use crate::ui::DialogLayer;

/// 对话框的回答：`true` 是做成了（设好了、解开了、重置了），`false` 是取消（包括 Esc）。
type Answer = Box<dyn FnOnce(bool, &mut Window, &mut App)>;

/// 对话框的回答只交一次：按钮交了，Esc 关对话框时就不再交。也记着对话框是哪一层：
/// 后台算完再关时只关自己（上面可能已经叠了连接的问答框）。
#[derive(Clone)]
pub(super) struct Reply {
    answer: Rc<RefCell<Option<Answer>>>,
    layer: DialogLayer,
}

impl Reply {
    pub(super) fn new(answer: impl FnOnce(bool, &mut Window, &mut App) + 'static) -> Self {
        Self {
            answer: Rc::new(RefCell::new(Some(Box::new(answer)))),
            layer: DialogLayer::new(),
        }
    }

    /// 不关心回答。
    pub(super) fn ignored() -> Self {
        Self::new(|_, _, _| {})
    }

    /// 对话框是哪一层。
    pub(super) fn layer(&self) -> &DialogLayer {
        &self.layer
    }

    pub(super) fn send(&self, done: bool, window: &mut Window, cx: &mut App) {
        let answer = self.answer.borrow_mut().take();
        if let Some(answer) = answer {
            answer(done, window, cx);
        }
    }
}

/// 关掉这个对话框（只关它自己那一层），再交回答。
pub(super) fn close_and_reply(reply: &Reply, done: bool, window: &mut Window, cx: &mut App) {
    reply.layer.close(window, cx);
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
    let layer = reply.layer.clone();
    layer.open_dismissable(
        move |dialog, _, _| {
            dialog
                .w(width)
                .p_0()
                .rounded(px(14.))
                .close_button(false)
                .overlay_closable(false)
                // 回车由框里的输入框自己处理（解锁、设主密码在后台算完才关）：gpui-component 的
                // `Confirm` 不关框，否则还没算完就当成取消关掉了（工单 33 发现）。
                .on_ok(|_, _, _| false)
                .child(view.clone())
        },
        move |window, cx| reply.send(false, window, cx),
        window,
        cx,
    );
    window.focus(&focus, cx);
}
