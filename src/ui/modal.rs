//! 唯一的一种弹窗：窗口上方居中、距顶 64、宽 400、圆角 12、外圈细线 + 阴影，背后压暗。
//!
//! - 头部（可选）高 36：图标 + 标题（500）+ 淡色说明，文字按基线对齐、图标单独居中。连接的问答框是
//!   服务器图标 + 主机名 + `user@地址:端口`；新建凭据、命令片段是各自的图标 + 「新建凭据」+ 名字这类；
//! - 正文四周 12 / 14 / 14、上下各一条细线，项之间 8；
//! - 底部高 40、没有底色：按钮靠右，主按钮强调色，按钮上带快捷键（`连接 Enter`）。
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
    Elevation, MONO_FONT_FAMILY, Tokens, TypeScale, UI_FONT_FAMILY, WEIGHT_NORMAL, WEIGHT_SEMIBOLD,
    baseline_shift, opacity, size, ui_font,
};
use crate::ui::checkbox::{CheckBox, CheckState};
use crate::ui::form::{FieldNote, form_field};

/// 头部、底部的高度。
const HEAD: Pixels = px(40.);
const FOOT: Pixels = px(44.);

/// 带标题和页脚的弹窗，保留底部留白后的正文限高。
pub(crate) fn modal_body_max_height(
    viewport: Pixels,
    bottom_gap: Pixels,
    minimum: Pixels,
) -> Pixels {
    (viewport - size::MODAL_TOP - HEAD - FOOT - bottom_gap).max(minimum)
}

#[cfg(test)]
mod geometry_tests {
    use gpui_kit::px;

    use super::{FOOT, HEAD, modal_body_max_height};
    use crate::theme::size;

    // 这里不能用 `use super::*`：它带进 `gpui_kit::*` 里的 `test` 宏，会盖住内建的 `#[test]`。
    #[test]
    fn body_height_reserves_the_rendered_frame_and_respects_the_minimum() {
        let bottom = px(48.);
        let minimum = px(240.);
        let fixed = size::MODAL_TOP + HEAD + FOOT + bottom;
        assert_eq!(
            modal_body_max_height(fixed + px(300.), bottom, minimum),
            px(300.)
        );
        assert_eq!(
            modal_body_max_height(fixed + px(120.), bottom, minimum),
            minimum
        );
    }
}

/// 紧凑模态弹窗外框（`MODAL_SM = 400px`）：确认、连接提示、主机密钥、保险库、凭据、片段、SFTP。
pub fn modal_dialog(dialog: Dialog, tokens: &Tokens) -> Dialog {
    dialog_frame(dialog, size::MODAL_SM, tokens)
}

/// 宽模态弹窗外框（`MODAL_MD = 480px`）：新增/编辑主机等复杂表单。
pub fn modal_dialog_wide(dialog: Dialog, tokens: &Tokens) -> Dialog {
    dialog_frame(dialog, size::MODAL_MD, tokens)
}

fn dialog_frame(dialog: Dialog, width: Pixels, tokens: &Tokens) -> Dialog {
    dialog
        .w(width)
        .margin_top(size::MODAL_TOP)
        .p_0()
        .rounded(size::RADIUS_MODAL)
        // 底色和投影都画在外框上：外框是圆角的，阴影跟着圆角走；放在里面那层的话，
        // 它会被弹出层的方框裁成直角阴影，面板角上就会露出一块方形的暗斑。
        .bg(tokens.popup)
        .shadow(Elevation::Modal.shadow(tokens.popup))
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
    let has_head = head.is_some();
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .font(ui_font())
        .text_size(scale.body)
        .text_color(tokens.text)
        .rounded(size::RADIUS_MODAL)
        // 面板自己画底：内容层不透出下面的窗口，圆角也和外框一致。
        .bg(tokens.popup)
        .children(head.map(|head| {
            let shift = baseline_shift(
                window,
                (UI_FONT_FAMILY, scale.title),
                (UI_FONT_FAMILY, scale.body),
                HEAD,
            );
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .h(HEAD)
                .px(px(20.))
                .child(
                    div()
                        .flex_none()
                        .text_color(tokens.icon)
                        .child(Icon::new(head.icon).size(size::ICON_LG)),
                )
                .child(
                    div()
                        .id("modal-title")
                        .test_support()
                        .aria_label(head.title.clone())
                        .flex_none()
                        .line_height(HEAD)
                        .text_size(scale.title)
                        .font_weight(WEIGHT_SEMIBOLD)
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
                .py(px(12.))
                .px(px(20.))
                .when(has_head, |body| body.border_t(size::LINE))
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
                .gap(px(8.))
                .h(FOOT)
                .px(px(20.))
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
    _window: &Window,
    cx: &App,
) -> impl IntoElement + use<I, L> {
    let disabled = on_click.is_none();
    let scale = TypeScale::current(cx);
    let label = label.into();
    let hover = match tone {
        ModalButtonTone::Danger => tokens.err_soft,
        ModalButtonTone::Primary => tokens.brand_hover,
        ModalButtonTone::Line => tokens.sel,
        ModalButtonTone::Plain => tokens.hover,
    };
    let keys_color = match tone {
        ModalButtonTone::Primary => tokens.on_brand_muted,
        _ => tokens.st_dim,
    };
    // 文案和快捷键合成一段文本：中文字形来自回退字体，和拉丁字形的基线位置不同，
    // 分成两个元素各自排版会差一两个像素；同一行文本里的字共用一条基线。
    let (text, keys_range) = match keys {
        Some(keys) => {
            let start = label.len();
            let text: SharedString = format!("{label}  {keys}").into();
            (text, Some(start..start + 2 + keys.len()))
        }
        None => (label.clone(), None),
    };
    let highlights = keys_range.into_iter().map(|range| {
        (
            range,
            HighlightStyle {
                color: Some(keys_color),
                ..Default::default()
            },
        )
    });
    div()
        .id(id.into())
        .test_support()
        .aria_label(label.clone())
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(size::CONTROL_LG)
        .px(px(12.))
        .rounded(size::RADIUS_MD)
        .text_size(scale.sm)
        .line_height(px(16.))
        .font_weight(WEIGHT_NORMAL)
        .text_color(tokens.label)
        .whitespace_nowrap()
        .map(|button| match tone {
            ModalButtonTone::Plain => button,
            ModalButtonTone::Primary => button
                .bg(tokens.brand)
                .border_1()
                .border_color(tokens.brand)
                .text_color(tokens.on_brand),
            ModalButtonTone::Line => button
                .bg(tokens.tab_active)
                .border_1()
                .border_color(tokens.line_strong),
            ModalButtonTone::Danger => button
                .text_color(tokens.err)
                .border_1()
                .border_color(tokens.err_line),
        })
        .when(disabled, |button| button.opacity(opacity::DISABLED))
        .when_some(on_click, |button, on_click| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .on_click(on_click)
        })
        .child(StyledText::new(text).with_highlights(highlights))
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

/// 一行带图标的标题（`.cp-warn`）：`danger` 时红字 500（在已有头部的对话框内作警示副标题），
/// 非 `danger` 时作为无头部弹窗的主标题（15px Semibold + 16px 图标）。
pub fn modal_warn(
    icon: CatalogIcon,
    text: impl Into<SharedString>,
    danger: bool,
    tokens: &Tokens,
    cx: &App,
) -> Div {
    let scale = TypeScale::current(cx);
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(if danger { 20. } else { 22. }))
        .text_color(if danger { tokens.err } else { tokens.text })
        .map(|row| {
            if danger {
                row.font_weight(WEIGHT_NORMAL)
            } else {
                row.text_size(scale.title).font_weight(WEIGHT_SEMIBOLD)
            }
        })
        .child(
            div()
                .flex_none()
                .relative()
                .when(!danger, |icon| icon.top(px(-0.5)).text_color(tokens.icon))
                .child(Icon::new(icon).size(size::ICON_LG)),
        )
        .child(
            div()
                .line_height(px(if danger { 20. } else { 22. }))
                .child(text.into()),
        )
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
        .rounded(size::RADIUS_MD)
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
                        .text_size(scale.body)
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
    crate::ui::input::frame(div(), tokens, invalid, focused)
        .when(disabled, |field| field.opacity(opacity::DISABLED))
        .flex()
        .items_center()
        .pl(crate::ui::input::INSET)
        .pr(crate::ui::input::TRAILING)
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
                    .px_0()
                    .text_size(TypeScale::current(cx).sm)
                    .line_height(px(16.))
                    .disabled(disabled)
                    .when(kind == ModalInputKind::Secret, |input| input.mask_toggle()),
            ),
        )
}

/// 多行输入框：有内容时用等宽字（命令片段的命令、贴进来的私钥），为空显示占位提示时用界面字体。
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
    let read = state.read(cx);
    let focused = read.focus_handle(cx).is_focused(window);
    let empty = read.value().is_empty();
    div()
        .id(id)
        .test_support()
        .aria_label(label)
        .w_full()
        .rounded(size::RADIUS_MD)
        .bg(tokens.input)
        .border_1()
        .border_color(if invalid {
            tokens.err
        } else if focused {
            tokens.brand
        } else {
            tokens.line_strong
        })
        .child(
            Textarea::new(state)
                .appearance(false)
                .m(px(-2.))
                .font_family(if empty {
                    UI_FONT_FAMILY
                } else {
                    MONO_FONT_FAMILY
                })
                .text_size(TypeScale::current(cx).sm),
        )
}

/// 勾选框：12 的方块，勾上时是强调色底、对比色勾；后面是文字和淡色说明。
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
        .h(size::CONTROL_SM)
        .cursor_pointer()
        .on_click(on_toggle)
        // 方框本体和 SFTP 权限矩阵共用；整行的 id、无障碍、标签和点击留在这一行上。
        .child(
            CheckBox::new(if checked {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            })
            .render(tokens),
        )
        .child(div().flex_none().line_height(size::CONTROL_SM).child(label))
        .children(note.map(|note| {
            div()
                .ml(px(2.))
                .min_w_0()
                .truncate()
                .line_height(size::CONTROL_SM)
                .text_color(tokens.st_dim)
                .child(note)
        }))
}

/// 带名字的一栏（新建主机页、设主密码这类）：和 `form::form_field` 同一套标签、间距和错误行，
/// 输入框换成 `modal_input`；红字的元素 id 是 `<field>-error`（测试靠它）。
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
) -> AnyElement {
    form_field(
        Some(label.clone()),
        modal_input(
            field,
            label,
            input,
            kind,
            error.is_some(),
            tokens,
            window,
            cx,
        ),
        error
            .map(|message| FieldNote::Error { field, message })
            .into_iter()
            .collect(),
        tokens,
    )
}
