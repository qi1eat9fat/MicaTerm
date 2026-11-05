//! 对话框的层（M2 code review S1）：按身份关闭，只关自己那一层；上面叠着的层原样留着。

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{
    InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _,
    TestSupportExt as _, div,
};
use qishell::ui::DialogLayer;

use crate::support::*;

fn shown(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &'static str) -> bool {
    in_frame(cx, handle, move |window, _| window.try_find(id).is_some())
}

/// 叠一层，里面是元素 id 为 `id` 的一块。
fn open(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    id: &'static str,
    dismissed: Rc<Cell<u32>>,
) -> DialogLayer {
    in_frame(cx, handle, move |window, cx| {
        let layer = DialogLayer::new();
        layer.open_dismissable(
            move |dialog, _, _| {
                dialog.child(div().id(id).test_support().size(px(40.)).into_any_element())
            },
            move |_, _| dismissed.set(dismissed.get() + 1),
            window,
            cx,
        );
        layer
    })
}

#[gpui_kit::test]
fn closing_a_lower_layer_keeps_the_one_above(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);
    let dismissed = Rc::new(Cell::new(0));
    let below = open(cx, handle, "layer-below", dismissed.clone());
    let above = open(cx, handle, "layer-above", dismissed.clone());

    in_frame(cx, handle, |window, cx| below.close(window, cx));
    assert!(!shown(cx, handle, "layer-below"));
    assert!(shown(cx, handle, "layer-above"));
    in_frame(cx, handle, |window, cx| {
        assert!(!below.is_open(window, cx));
        assert!(above.is_open(window, cx));
    });

    // 关过的再关一次什么都不做。
    in_frame(cx, handle, |window, cx| below.close(window, cx));
    assert!(shown(cx, handle, "layer-above"));

    in_frame(cx, handle, |window, cx| above.close(window, cx));
    assert!(!shown(cx, handle, "layer-above"));
    in_frame(cx, handle, |window, cx| {
        assert!(!window.has_active_dialog(cx))
    });
    // 经 `close` 关的不算被 Esc 关掉。
    assert_eq!(dismissed.get(), 0);
}

#[gpui_kit::test]
fn escape_closes_the_top_layer_and_says_so(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);
    let dismissed = Rc::new(Cell::new(0));
    let below = open(cx, handle, "layer-below", dismissed.clone());
    let above = open(cx, handle, "layer-above", dismissed.clone());

    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    assert_eq!(dismissed.get(), 1);
    assert!(!shown(cx, handle, "layer-above"));
    assert!(shown(cx, handle, "layer-below"));
    in_frame(cx, handle, |window, cx| {
        assert!(!above.is_open(window, cx));
        assert!(below.is_open(window, cx));
    });

    // Esc 关掉的层也从记录里去掉了：再关下面那层不会误关别的。
    in_frame(cx, handle, |window, cx| below.close(window, cx));
    in_frame(cx, handle, |window, cx| {
        assert!(!window.has_active_dialog(cx))
    });
}
