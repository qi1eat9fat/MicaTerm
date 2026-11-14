//! 对话框的层（M2 code review S1）：按身份关闭，只关自己那一层；上面叠着的层原样留着。

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{
    InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _,
    TestSupportExt as _, div,
};
use micaterm::ui::DialogLayer;
use micaterm::ui::confirm::{
    Choice, ChoiceButton, ChoiceOutcome, ChoiceTone, ConfirmLine, open_choice_dialog,
};

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

/// 编辑器关闭、被别人改过、传输冲突共用的多按钮框：开一个「重新加载 | 跳过 / 覆盖」加勾选框的。
fn open_choices(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    outcome: Rc<std::cell::RefCell<Option<ChoiceOutcome>>>,
) {
    in_frame(cx, handle, move |window, cx| {
        open_choice_dialog(
            Choice {
                id: "conflict",
                title: "文件已存在".into(),
                subtitle: Some("上传 3 项".into()),
                lines: vec![ConfirmLine::Detail {
                    label: "已有的".into(),
                    value: "1.5 KB".into(),
                }],
                check: Some("本批剩下的冲突都这样处理".into()),
                buttons: vec![
                    ChoiceButton::new("reload", "重新加载".into()).left(),
                    ChoiceButton::new("skip", "跳过".into()),
                    ChoiceButton::new("overwrite", "覆盖".into()).tone(ChoiceTone::Primary),
                ],
            },
            DialogLayer::new(),
            Box::new(move |result, _, _| *outcome.borrow_mut() = Some(result)),
            window,
            cx,
        );
    });
}

#[gpui_kit::test]
fn a_choice_dialog_says_which_button_and_whether_the_box_was_checked(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);
    let outcome = Rc::new(std::cell::RefCell::new(None));
    open_choices(cx, handle, outcome.clone());
    in_frame(cx, handle, |window, _| {
        let text = window.find("conflict-text");
        assert_eq!(text.label(), Some("已有的 1.5 KB"));
    });

    in_frame(cx, handle, |window, cx| window.click("conflict-check", cx));
    in_frame(cx, handle, |window, cx| {
        window.click("conflict-overwrite", cx)
    });

    assert_eq!(
        *outcome.borrow(),
        Some(ChoiceOutcome {
            button: Some("overwrite"),
            checked: true,
        })
    );
    assert!(!shown(cx, handle, "conflict"));
}

#[gpui_kit::test]
fn escape_on_a_choice_dialog_is_no_choice(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);
    let outcome = Rc::new(std::cell::RefCell::new(None));
    open_choices(cx, handle, outcome.clone());

    in_frame(cx, handle, |window, cx| window.press("escape", cx));

    assert_eq!(
        *outcome.borrow(),
        Some(ChoiceOutcome {
            button: None,
            checked: false,
        })
    );
    assert!(!shown(cx, handle, "conflict"));
}
