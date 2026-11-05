//! 确认框（原型 `.dlg.confirm`）：标题、一段话、右下「取消」和红色的确认按钮。删除主机、删除凭据、
//! 重置保险库、「在另一台电脑上创建」共用。
//!
//! 确认要做的事可以在后台跑（重置保险库）：跑的时候两个按钮都不能点，跑完由它自己关掉这一层
//! （只关自己，上面叠着的问答框不受影响）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::*;

use super::DialogLayer;
use crate::theme::Tokens;

/// 确认框的宽（原型 `.dlg.confirm`）。
const WIDTH: Pixels = px(420.);

/// 确认框里的一行话。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfirmLine {
    /// 一句话。
    Text(SharedString),
    /// 列出的一项（主机名）：缩进，前面带圆点。
    Item(SharedString),
}

impl ConfirmLine {
    fn text(&self) -> &SharedString {
        match self {
            ConfirmLine::Text(text) | ConfirmLine::Item(text) => text,
        }
    }
}

/// 确认框写什么。元素 id：框是 `id`，那段话是 `<id>-text`（全部文字一行一句写在无障碍名称里，
/// 测试靠它断言），按钮是 `<id>-cancel`、`<id>-confirm`。
pub struct Confirm {
    pub id: &'static str,
    pub title: SharedString,
    pub lines: Vec<ConfirmLine>,
    pub cancel: SharedString,
    pub confirm: SharedString,
}

/// 点了确认按钮要做的事。返回 `None` 就是做完了，确认框随即关掉；返回 `Some(task)` 是在后台跑，
/// 跑的时候按钮不能点，跑完由它自己关掉这一层（参数就是这一层）。
pub type OnConfirm = Box<dyn FnOnce(&DialogLayer, &mut Window, &mut App) -> Option<Task<()>>>;

/// 没确认（「取消」或 Esc）时要做的事。
pub type OnCancel = Box<dyn FnOnce(&mut Window, &mut App)>;

/// 确认框的内容。
pub struct ConfirmDialog {
    confirm: Confirm,
    layer: DialogLayer,
    on_confirm: Option<OnConfirm>,
    on_cancel: Rc<RefCell<Option<OnCancel>>>,
    working: Option<Task<()>>,
    focus_handle: FocusHandle,
}

impl ConfirmDialog {
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(on_confirm) = self.on_confirm.take() else {
            return;
        };
        // 确认了就不再算取消。
        self.on_cancel.borrow_mut().take();
        match on_confirm(&self.layer, window, cx) {
            Some(task) => {
                self.working = Some(task);
                cx.notify();
            }
            None => self.layer.close(window, cx),
        }
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layer.close(window, cx);
        cancelled(&self.on_cancel, window, cx);
    }

    /// 全部文字，一行一句。
    fn description(&self) -> SharedString {
        self.confirm
            .lines
            .iter()
            .map(|line| line.text().as_ref())
            .collect::<Vec<_>>()
            .join("\n")
            .into()
    }
}

fn cancelled(on_cancel: &Rc<RefCell<Option<OnCancel>>>, window: &mut Window, cx: &mut App) {
    let on_cancel = on_cancel.borrow_mut().take();
    if let Some(on_cancel) = on_cancel {
        on_cancel(window, cx);
    }
}

impl Focusable for ConfirmDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ConfirmDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let id = self.confirm.id;
        let working = self.working.is_some();
        div()
            .id(id)
            .test_support()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .text_color(tokens.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(46.))
                    .px(px(18.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.confirm.title.clone()),
            )
            .child(
                div()
                    .id(SharedString::from(format!("{id}-text")))
                    .test_support()
                    .aria_label(self.description())
                    .flex()
                    .flex_col()
                    .px(px(18.))
                    .pb(px(10.))
                    .text_size(px(12.5))
                    .line_height(relative(1.6))
                    .text_color(tokens.muted)
                    .children(self.confirm.lines.iter().map(|line| match line {
                        ConfirmLine::Text(text) => div().child(text.clone()),
                        ConfirmLine::Item(item) => {
                            div().pl(px(18.)).child(format!("\u{2022} {item}"))
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(12.))
                    .pb(px(14.))
                    .pl(px(18.))
                    .pr(px(14.))
                    .child(div().flex_1())
                    .child(
                        Button::new(SharedString::from(format!("{id}-cancel")))
                            .small()
                            .label(self.confirm.cancel.clone())
                            .disabled(working)
                            .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("{id}-confirm")))
                            .danger()
                            .small()
                            .label(self.confirm.confirm.clone())
                            .disabled(working)
                            .on_click(cx.listener(|this, _, window, cx| this.confirm(window, cx))),
                    ),
            )
    }
}

/// 叠一个确认框，是 `layer` 那一层。不点遮罩关闭；Esc 等于「取消」。焦点给确认框。
pub fn open_confirm_dialog(
    confirm: Confirm,
    layer: DialogLayer,
    on_confirm: OnConfirm,
    on_cancel: OnCancel,
    window: &mut Window,
    cx: &mut App,
) {
    let on_cancel = Rc::new(RefCell::new(Some(on_cancel)));
    let view = cx.new(|cx| ConfirmDialog {
        confirm,
        layer: layer.clone(),
        on_confirm: Some(on_confirm),
        on_cancel: on_cancel.clone(),
        working: None,
        focus_handle: cx.focus_handle(),
    });
    let focus = view.read(cx).focus_handle.clone();
    layer.open_dismissable(
        move |dialog, _, _| {
            dialog
                .w(WIDTH)
                .p_0()
                .rounded(px(14.))
                .close_button(false)
                .overlay_closable(false)
                .child(view.clone())
        },
        move |window, cx| cancelled(&on_cancel, window, cx),
        window,
        cx,
    );
    window.focus(&focus, cx);
}
