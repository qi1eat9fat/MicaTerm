//! 确认框（原型 `.dlg.confirm`）：标题、一段话、右下「取消」和红色的确认按钮。删除主机、删除凭据、
//! 重置保险库、「在另一台电脑上创建」共用。
//!
//! 确认要做的事可以在后台跑（重置保险库）：跑的时候两个按钮都不能点，跑完由它自己关掉这一层
//! （只关自己，上面叠着的问答框不受影响）。
//!
//! 按钮多于两个的（编辑器关闭的「保存 / 不保存 / 取消」、被别人改过的「重新加载 / 取消 / 覆盖」、
//! 传输问题的「取消 / 跳过 / 覆盖」加勾选框）用 [`open_choice_dialog`]。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
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
    /// 一行对比（原型 `.fp`）：灰字的标签加内容，如「已有的  1.5 KB · 2026/11/3 9:58」。
    Detail {
        label: SharedString,
        value: SharedString,
    },
}

impl ConfirmLine {
    /// 这一行的全部文字。
    fn text(&self) -> String {
        match self {
            ConfirmLine::Text(text) | ConfirmLine::Item(text) => text.to_string(),
            ConfirmLine::Detail { label, value } => format!("{label} {value}"),
        }
    }

    fn render(&self, tokens: &Tokens) -> AnyElement {
        match self {
            ConfirmLine::Text(text) => div().child(text.clone()).into_any_element(),
            ConfirmLine::Item(item) => div()
                .pl(px(18.))
                .child(format!("\u{2022} {item}"))
                .into_any_element(),
            ConfirmLine::Detail { label, value } => div()
                .flex()
                .gap(px(10.))
                .child(
                    div()
                        .flex_none()
                        .w(px(52.))
                        .text_color(tokens.faint)
                        .child(label.clone()),
                )
                .child(div().min_w_0().text_color(tokens.text).child(value.clone()))
                .into_any_element(),
        }
    }
}

/// 全部文字，一行一句：写在无障碍名称里，测试靠它断言。
fn lines_text(lines: &[ConfirmLine]) -> SharedString {
    lines
        .iter()
        .map(ConfirmLine::text)
        .collect::<Vec<_>>()
        .join("\n")
        .into()
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
                    .aria_label(lines_text(&self.confirm.lines))
                    .flex()
                    .flex_col()
                    .px(px(18.))
                    .pb(px(10.))
                    .text_size(px(12.5))
                    .line_height(relative(1.6))
                    .text_color(tokens.muted)
                    .children(self.confirm.lines.iter().map(|line| line.render(&tokens))),
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

/// 多按钮框里一个按钮的样子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceTone {
    Normal,
    /// 强调色：主按钮（覆盖、保存）。
    Primary,
    /// 红色：会丢东西的（覆盖别人的修改）。
    Danger,
}

/// 多按钮框里的一个按钮。元素 id 是 `<框的 id>-<按钮的 id>`。
pub struct ChoiceButton {
    pub id: &'static str,
    pub label: SharedString,
    pub tone: ChoiceTone,
    /// 靠左放（原型里「重新加载」「取消」在左边，和右边的按钮隔开）。
    pub left: bool,
}

impl ChoiceButton {
    pub fn new(id: &'static str, label: SharedString) -> Self {
        Self {
            id,
            label,
            tone: ChoiceTone::Normal,
            left: false,
        }
    }

    pub fn tone(mut self, tone: ChoiceTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn left(mut self) -> Self {
        self.left = true;
        self
    }
}

/// 多按钮框写什么。元素 id：框是 `id`，那段话是 `<id>-text`，勾选框是 `<id>-check`。
pub struct Choice {
    pub id: &'static str,
    pub title: SharedString,
    /// 标题后面的灰字：主机名、文件名。
    pub subtitle: Option<SharedString>,
    pub lines: Vec<ConfirmLine>,
    /// 按钮上面的勾选框（「本批剩下的冲突都这样处理」）。
    pub check: Option<SharedString>,
    /// 从左到右。
    pub buttons: Vec<ChoiceButton>,
}

/// 多按钮框的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoiceOutcome {
    /// 点了哪个按钮；`None` 是没选就关了（Esc）。
    pub button: Option<&'static str>,
    /// 勾选框勾着没有。
    pub checked: bool,
}

/// 多按钮框关掉时要做的事，一定调一次（点按钮、Esc 都算）；经 `DialogLayer::close` 从外面收掉时不调。
pub type OnChoice = Box<dyn FnOnce(ChoiceOutcome, &mut Window, &mut App)>;

/// 多按钮框的内容。
pub struct ChoiceDialog {
    choice: Choice,
    checked: bool,
    layer: DialogLayer,
    on_choice: Rc<RefCell<Option<OnChoice>>>,
    focus_handle: FocusHandle,
}

impl ChoiceDialog {
    fn choose(&mut self, button: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.layer.close(window, cx);
        let outcome = ChoiceOutcome {
            button: Some(button),
            checked: self.checked,
        };
        chosen(&self.on_choice, outcome, window, cx);
    }

    fn render_button(&self, button: &ChoiceButton, cx: &mut Context<Self>) -> Button {
        let (id, choice) = (self.choice.id, button.id);
        let view = Button::new(SharedString::from(format!("{id}-{choice}")))
            .small()
            .label(button.label.clone())
            .on_click(cx.listener(move |this, _, window, cx| this.choose(choice, window, cx)));
        match button.tone {
            ChoiceTone::Normal => view,
            ChoiceTone::Primary => view.primary(),
            ChoiceTone::Danger => view.danger(),
        }
    }
}

fn chosen(
    on_choice: &Rc<RefCell<Option<OnChoice>>>,
    outcome: ChoiceOutcome,
    window: &mut Window,
    cx: &mut App,
) {
    let on_choice = on_choice.borrow_mut().take();
    if let Some(on_choice) = on_choice {
        on_choice(outcome, window, cx);
    }
}

impl Focusable for ChoiceDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ChoiceDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let id = self.choice.id;
        let left: Vec<_> = self
            .choice
            .buttons
            .iter()
            .filter(|button| button.left)
            .map(|button| self.render_button(button, cx))
            .collect();
        let right: Vec<_> = self
            .choice
            .buttons
            .iter()
            .filter(|button| !button.left)
            .map(|button| self.render_button(button, cx))
            .collect();
        let check = self.choice.check.clone().map(|label| {
            div().px(px(18.)).pb(px(4.)).child(
                Checkbox::new(SharedString::from(format!("{id}-check")))
                    .small()
                    .checked(self.checked)
                    .label(label)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.checked = *checked;
                        cx.notify();
                    })),
            )
        });
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
                    .gap(px(8.))
                    .h(px(46.))
                    .px(px(18.))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.choice.title.clone()),
                    )
                    .children(self.choice.subtitle.clone().map(|subtitle| {
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(tokens.faint)
                            .child(subtitle)
                    })),
            )
            .child(
                div()
                    .id(SharedString::from(format!("{id}-text")))
                    .test_support()
                    .aria_label(lines_text(&self.choice.lines))
                    .flex()
                    .flex_col()
                    .px(px(18.))
                    .pb(px(10.))
                    .text_size(px(12.5))
                    .line_height(relative(1.6))
                    .text_color(tokens.muted)
                    .children(self.choice.lines.iter().map(|line| line.render(&tokens))),
            )
            .children(check)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(12.))
                    .pb(px(14.))
                    .pl(px(18.))
                    .pr(px(14.))
                    .children(left)
                    .child(div().flex_1())
                    .children(right),
            )
    }
}

/// 叠一个多按钮框，是 `layer` 那一层。不点遮罩关闭；Esc 等于没选（`button` 是 `None`）。焦点给框。
pub fn open_choice_dialog(
    choice: Choice,
    layer: DialogLayer,
    on_choice: OnChoice,
    window: &mut Window,
    cx: &mut App,
) {
    let on_choice = Rc::new(RefCell::new(Some(on_choice)));
    let view = cx.new(|cx| ChoiceDialog {
        choice,
        checked: false,
        layer: layer.clone(),
        on_choice: on_choice.clone(),
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
        move |window, cx| {
            let outcome = ChoiceOutcome {
                button: None,
                checked: false,
            };
            chosen(&on_choice, outcome, window, cx);
        },
        window,
        cx,
    );
    window.focus(&focus, cx);
}
