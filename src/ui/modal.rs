//! 唯一的一种弹窗（26 号「连接提示」「主机密钥变了」「解锁保险库」，照 Zed `RemoteConnectionModal`）：
//! 窗口上方居中、距顶 72、宽 448、圆角 8、外圈 `--line-strong` + 阴影，不压暗背景。
//!
//! - 头部（可选）高 40：图标 + 标题（13/500）+ 淡色说明，文字按基线对齐、图标单独居中。连接的问答框是
//!   服务器图标 + 主机名 + `user@地址:端口`；新建凭据、命令片段是各自的图标 + 「新建凭据」+ 名字这类；
//! - 正文四周 12 / 14 / 14、上下各一条细线，项之间 8；
//! - 底部高 44、没有底色：按钮靠右，主按钮浅灰底，按钮上带快捷键（`连接 Enter`）。
//!
//! 弹窗的开关、叠放仍走 [`DialogLayer`](super::DialogLayer)；这里只管样子：[`modal_dialog`] 把
//! gpui-component 的 `Dialog` 收成这个外框，[`render_modal`] 画里面三段，其余是正文里常用的几样。

use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::CatalogIcon;
use crate::theme::{
    MONO_FONT_FAMILY, Tokens, TypeScale, UI_FONT_FAMILY, WEIGHT_MEDIUM, baseline_shift, size,
};

/// 头部、底部的高度（26 号 `.cp-h`、`.cp-f`）。
const HEAD: Pixels = px(40.);
const FOOT: Pixels = px(44.);

/// 把 `Dialog` 收成 26 号的弹窗外框：宽 448、距顶 72、没有内边距（里面三段自己留）、没有右上角 ×、
/// 不点遮罩关闭。背景、外圈、圆角来自界面主题（`theme::component`）。
pub fn modal_dialog(dialog: Dialog) -> Dialog {
    dialog
        .w(size::MODAL_WIDTH)
        .margin_top(size::MODAL_TOP)
        .p_0()
        .rounded(size::RADIUS_LG)
        .close_button(false)
        .overlay_closable(false)
}

/// 头部：说明这个弹窗是什么、关于谁。标题的元素 id 是 `modal-title`，淡字是 `modal-detail`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModalHead {
    /// 前面的图标：连接的问答框是服务器。
    pub icon: CatalogIcon,
    pub title: SharedString,
    /// 后面的淡字：`user@地址:端口`、编辑的是哪一条。
    pub detail: Option<SharedString>,
}

/// 弹窗里面三段：头部（可选）、正文、底部。
pub fn render_modal(
    head: Option<ModalHead>,
    body: impl IntoElement,
    footer: impl IntoElement,
    tokens: &Tokens,
    window: &Window,
    cx: &App,
) -> Div {
    let scale = TypeScale::current(cx);
    div()
        .flex()
        .flex_col()
        .text_size(scale.ui)
        .text_color(tokens.text)
        .children(head.map(|head| {
            // 13 的主机名和 12 的地址字号不同：地址挪到主机名的基线上（盒子居中时两段的字底差约 1px）。
            let shift = baseline_shift(
                window,
                (UI_FONT_FAMILY, scale.side),
                (UI_FONT_FAMILY, scale.ui),
                HEAD,
            );
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .h(HEAD)
                .px(px(14.))
                .child(
                    div()
                        .flex_none()
                        .text_color(tokens.icon)
                        .child(Icon::new(head.icon).size(px(14.))),
                )
                .child(
                    div()
                        .id("modal-title")
                        .test_support()
                        .aria_label(head.title.clone())
                        .flex_none()
                        .line_height(HEAD)
                        .text_size(scale.side)
                        .font_weight(WEIGHT_MEDIUM)
                        .child(head.title),
                )
                .children(head.detail.map(|detail| {
                    div()
                        .id("modal-detail")
                        .test_support()
                        .aria_label(detail.clone())
                        .min_w_0()
                        .truncate()
                        .relative()
                        .top(shift)
                        .line_height(HEAD)
                        .text_color(tokens.st_dim)
                        .child(detail)
                }))
        }))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .pt(px(12.))
                .px(px(14.))
                .pb(px(14.))
                .border_t(size::LINE)
                .border_b(size::LINE)
                .border_color(tokens.line)
                .child(body),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_end()
                .gap(px(6.))
                .h(FOOT)
                .px(px(10.))
                .child(footer),
        )
}

/// 按钮的样子（26 号 `.hbtn`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalButtonTone {
    /// 没有底色和边，悬停才有底色（「取消」）。
    Plain,
    /// 浅灰底（默认的那个：回车就是它）。
    Primary,
    /// 描边的次要按钮（26 号 `.hbtn.line`：「测试连接」）。
    Line,
    /// 红字、淡红描边（「更新并连接」这类会丢东西的）。
    Danger,
}

/// 点击的处理。
pub type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// 一个按钮：文字后面跟淡色快捷键（`Esc`、`Enter`），快捷键按基线对齐再上移 0.5（用户定）。
/// `on_click` 是 `None` 时淡掉、不响应（在后台跑着的时候）。
#[allow(clippy::too_many_arguments)]
pub fn modal_button<I: Into<ElementId>, L: Into<SharedString>>(
    id: I,
    label: L,
    keys: Option<&'static str>,
    tone: ModalButtonTone,
    on_click: Option<OnClick>,
    tokens: &Tokens,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<I, L> {
    let disabled = on_click.is_none();
    let scale = TypeScale::current(cx);
    let label = label.into();
    let shift = baseline_shift(
        window,
        (UI_FONT_FAMILY, scale.ui),
        (MONO_FONT_FAMILY, scale.mono_small),
        size::BUTTON,
    ) - px(0.5);
    let hover = match tone {
        ModalButtonTone::Danger => tokens.err.opacity(0.08),
        _ => tokens.hover,
    };
    div()
        .id(id.into())
        .test_support()
        .aria_label(label.clone())
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(size::BUTTON)
        .px(px(12.))
        .rounded(size::RADIUS)
        .text_color(tokens.label)
        .whitespace_nowrap()
        .map(|button| match tone {
            ModalButtonTone::Plain => button,
            ModalButtonTone::Primary => button.bg(tokens.sel).text_color(tokens.text),
            ModalButtonTone::Line => button.border_1().border_color(tokens.line_strong),
            ModalButtonTone::Danger => button
                .text_color(tokens.err)
                .border_1()
                .border_color(tokens.err.opacity(0.45)),
        })
        .when(disabled, |button| button.opacity(0.5))
        .when_some(on_click, |button, on_click| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .on_click(on_click)
        })
        .child(div().line_height(size::BUTTON).child(label))
        .children(keys.map(|keys| {
            div()
                .relative()
                .top(shift)
                .line_height(size::BUTTON)
                .font_family(MONO_FONT_FAMILY)
                .text_size(scale.mono_small)
                .text_color(tokens.st_dim)
                .child(keys)
        }))
}

/// 正文的那句问话（`.cp-q`）：次要色。
pub fn modal_question(text: impl Into<SharedString>, tokens: &Tokens) -> Div {
    div()
        .text_color(tokens.text2)
        .line_height(relative(1.6))
        .child(text.into())
}

/// 正文末尾的说明（`.cp-n`）：淡色。
pub fn modal_note(text: impl Into<SharedString>, tokens: &Tokens) -> Div {
    div().text_color(tokens.st_dim).child(text.into())
}

/// 一行带图标的标题（`.cp-warn`）：`danger` 时红字 500，否则正文色、图标是图标色。
/// 上轻下重的图标（锁）上移 0.5（`.cp-warn.plain svg`）。
pub fn modal_warn(
    icon: CatalogIcon,
    text: impl Into<SharedString>,
    danger: bool,
    tokens: &Tokens,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(20.))
        .text_color(if danger { tokens.err } else { tokens.text })
        .when(danger, |row| row.font_weight(WEIGHT_MEDIUM))
        .child(
            div()
                .flex_none()
                .relative()
                .when(!danger, |icon| icon.top(px(-0.5)).text_color(tokens.icon))
                .child(Icon::new(icon).size(px(if danger { 16. } else { 14. }))),
        )
        .child(div().line_height(px(20.)).child(text.into()))
}

/// 指纹框里的一行：标签、内容，`old` 时内容淡掉（记下的旧密钥）。
pub struct FingerprintRow {
    pub label: SharedString,
    pub value: SharedString,
    pub old: bool,
}

/// 指纹框（`.cp-fp`）：内容底、细边、圆角 6；标签用界面字体淡色，内容用等宽字体、不折行。
pub fn modal_fingerprint(rows: Vec<FingerprintRow>, tokens: &Tokens, cx: &App) -> Div {
    let scale = TypeScale::current(cx);
    div()
        .flex()
        .flex_col()
        .py(px(8.))
        .px(px(10.))
        .rounded(size::RADIUS)
        .bg(tokens.content)
        .border_1()
        .border_color(tokens.line)
        .line_height(relative(1.6))
        .children(rows.into_iter().map(|row| {
            div()
                .flex()
                .items_baseline()
                .gap(px(14.))
                .whitespace_nowrap()
                .child(
                    div()
                        .flex_none()
                        .w(px(44.))
                        .text_color(tokens.st_dim)
                        .child(row.label),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .font_family(MONO_FONT_FAMILY)
                        .text_size(scale.ui)
                        .text_color(if row.old { tokens.st_dim } else { tokens.text })
                        .child(row.value),
                )
        }))
}

/// 输入框的写法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalInputKind {
    /// 普通文字。
    Text,
    /// 密码：右边一个眼睛，点了显示明文。
    Secret,
    /// 等宽字体（服务器问的验证码这类）。
    Mono,
}

/// 输入框（`.cp-in`）：高 30、内容底、细边，聚焦时边深一档；`invalid` 时边是红的。
#[allow(clippy::too_many_arguments)]
pub fn modal_input(
    id: impl Into<ElementId>,
    label: SharedString,
    input: &Entity<InputState>,
    kind: ModalInputKind,
    invalid: bool,
    tokens: &Tokens,
    window: &Window,
    cx: &App,
) -> Div {
    input_box(id, label, input, kind, invalid, false, tokens, window, cx)
}

/// 改不了的输入框：样子同 [`modal_input`]，淡掉一半，不能聚焦、不能打字（SFTP 属性里没有权利改的权限）。
pub fn modal_input_disabled(
    id: impl Into<ElementId>,
    label: SharedString,
    input: &Entity<InputState>,
    kind: ModalInputKind,
    tokens: &Tokens,
    window: &Window,
    cx: &App,
) -> Div {
    input_box(id, label, input, kind, false, true, tokens, window, cx)
}

#[allow(clippy::too_many_arguments)]
fn input_box(
    id: impl Into<ElementId>,
    label: SharedString,
    input: &Entity<InputState>,
    kind: ModalInputKind,
    invalid: bool,
    disabled: bool,
    tokens: &Tokens,
    window: &Window,
    cx: &App,
) -> Div {
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    div()
        .when(disabled, |field| field.opacity(0.5))
        .flex()
        .items_center()
        .h(size::INPUT)
        .pl(px(8.))
        .pr(px(4.))
        .rounded(size::RADIUS)
        .bg(tokens.content)
        .border_1()
        .border_color(if invalid {
            tokens.err
        } else if focused {
            tokens.icon
        } else {
            tokens.line_strong
        })
        .when(kind == ModalInputKind::Mono, |field| {
            field.font_family(MONO_FONT_FAMILY)
        })
        .child(
            div().flex_1().min_w_0().child(
                Input::new(input)
                    .id(id)
                    .aria_label(label)
                    .appearance(false)
                    .small()
                    // 内边距由外面这层给（左 8）；Input 自己那层清掉，不然文字离边框差不多 16。
                    .px_0()
                    // `.small()` 的字是 0.875rem（约 10.5）；26 的输入框跟界面字号走。
                    .text_size(TypeScale::current(cx).ui)
                    .disabled(disabled)
                    .when(kind == ModalInputKind::Secret, |input| input.mask_toggle()),
            ),
        )
}

/// 多行输入框：和 [`modal_input`] 一个样子（内容底、细边，聚焦时边深一档，`invalid` 时红边），
/// 等宽字（命令片段的命令、贴进来的私钥）。元素 id 是 `id`，外面这层就是它，点哪儿都进到框里。
#[allow(clippy::too_many_arguments)]
pub fn modal_textarea(
    id: &'static str,
    label: SharedString,
    state: &Entity<TextareaState>,
    invalid: bool,
    tokens: &Tokens,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    div()
        .id(id)
        .test_support()
        .aria_label(label)
        .w_full()
        .rounded(size::RADIUS)
        .bg(tokens.content)
        .border_1()
        .border_color(if invalid {
            tokens.err
        } else if focused {
            tokens.icon
        } else {
            tokens.line_strong
        })
        .child(
            // Textarea 把中档的内边距（左右 10、上下 8）设进编辑区，外面写的内边距去不掉：
            // 四周往外撑 2，字离边框就是左右 8、上下 6，和单行框的字对齐。
            Textarea::new(state)
                .appearance(false)
                .m(px(-2.))
                .font_family(MONO_FONT_FAMILY)
                .text_size(TypeScale::current(cx).mono_small),
        )
}

/// 勾选框（`.cp-chk`）：12 的方块（100/125/150/200% 下都是整像素），勾上时是正文色底、白勾；
/// 后面是文字和淡色说明。点整行切换（`on_toggle`）。
pub fn modal_checkbox<I: Into<ElementId>, L: Into<SharedString>>(
    id: I,
    checked: bool,
    label: L,
    note: Option<SharedString>,
    on_toggle: OnClick,
    tokens: &Tokens,
) -> impl IntoElement + use<I, L> {
    let label = label.into();
    div()
        .id(id.into())
        .test_support()
        .aria_label(label.clone())
        .role(Role::CheckBox)
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(20.))
        .cursor_pointer()
        .on_click(on_toggle)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(size::CHECKBOX)
                .rounded(px(3.))
                .map(|box_| {
                    if checked {
                        box_.bg(tokens.text)
                            .text_color(tokens.content)
                            .child(Icon::new(CatalogIcon::Check).size(px(10.)))
                    } else {
                        box_.border_1().border_color(tokens.line_strong)
                    }
                }),
        )
        .child(div().line_height(px(20.)).child(label))
        .children(note.map(|note| {
            div()
                .ml(px(2.))
                .line_height(px(20.))
                .text_color(tokens.st_dim)
                .child(note)
        }))
}

/// 带名字的一栏（新建主机页、设主密码这类）：字段名在上（`.mf-l`）、输入框在下，出错时下面一行红字，
/// 红字的元素 id 是 `<field>-error`（和 `form::FieldNote` 一样，测试靠它）。
#[allow(clippy::too_many_arguments)]
pub fn modal_field(
    field: &'static str,
    label: SharedString,
    input: &Entity<InputState>,
    kind: ModalInputKind,
    error: Option<SharedString>,
    tokens: &Tokens,
    window: &Window,
    cx: &App,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(div().text_color(tokens.label).child(label.clone()))
        .child(modal_input(
            field,
            label,
            input,
            kind,
            error.is_some(),
            tokens,
            window,
            cx,
        ))
        .children(error.map(|message| {
            div()
                .id(SharedString::from(format!("{field}-error")))
                .test_support()
                .aria_label(message.clone())
                .text_color(tokens.err)
                .child(message)
        }))
}
