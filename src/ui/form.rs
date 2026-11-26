//! 表单的通用件（26 号「新建主机」的写法）：一组字段、一栏（字段名在上、控件在下，出错时下面一行红字）、
//! 并排的两栏、组之间的细线、锁着的秘密那一行。主机表单页和凭据、命令片段、SFTP 的弹窗共用
//! （spec 第 4 节「编辑表单」）；输入框、按钮这些弹窗里的件在 `ui::modal`。

use gpui_kit::component::Icon;
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use zeroize::Zeroizing;

use crate::app::{CatalogIcon, UnlockVault};
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale, size};

/// 表单里出错的字段和原因，按字段的元素 id 记。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldErrors(Vec<(&'static str, SharedString)>);

impl FieldErrors {
    /// 记下 `field` 出错的原因；同一个字段只记第一条。
    pub fn set(&mut self, field: &'static str, message: SharedString) {
        if self.get(field).is_none() {
            self.0.push((field, message));
        }
    }

    pub fn get(&self, field: &str) -> Option<&SharedString> {
        self.0
            .iter()
            .find(|(name, _)| *name == field)
            .map(|(_, message)| message)
    }

    /// 用户改了这个字段：它的错误先收起来，保存时再查。返回是否真有错误被收起。
    pub fn clear(&mut self, field: &str) -> bool {
        let before = self.0.len();
        self.0.retain(|(name, _)| *name != field);
        self.0.len() != before
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 一组字段（26 号「新建主机」）：不再套带标题的框，字段一个接一个往下排、间距 14；
/// 组和组之间由调用方画一条细线（[`form_divider`]）。
pub fn form_group(rows: impl IntoIterator<Item = AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .children(rows)
        .into_any_element()
}

/// 组之间的细线（26 号 `.hp-div`）。
pub fn form_divider(tokens: &Tokens) -> AnyElement {
    div()
        .flex_none()
        .h(size::LINE)
        .bg(tokens.line)
        .into_any_element()
}

/// 一栏（26 号 `.mf`）：字段名在上（`label` 是 `None` 时没有）、控件在下，再下面是 `notes`：
/// 出错的原因（红字，id 是 `<field>-error`，原因写在无障碍名称里，测试靠它断言）或提示（灰字）。
pub fn form_field(
    label: Option<SharedString>,
    control: impl IntoElement,
    notes: Vec<FieldNote>,
    tokens: &Tokens,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(4.))
        .children(label.map(|label| div().text_color(tokens.text).child(label)))
        .child(control)
        .children(notes.into_iter().map(|note| note.render(tokens)))
        .into_any_element()
}

/// 并排的两栏（26 号 `.mf-two`）：地址 + 端口、用户名 + 密码。`right_width` 给了就是右边那栏的宽
/// （端口 96），否则两栏平分。
pub fn form_pair(left: AnyElement, right: AnyElement, right_width: Option<Pixels>) -> AnyElement {
    div()
        .flex()
        .gap(px(8.))
        .child(div().flex().flex_1().min_w_0().child(left))
        .child(
            div()
                .flex()
                .min_w_0()
                .map(|right| match right_width {
                    Some(width) => right.flex_none().w(width),
                    None => right.flex_1(),
                })
                .child(right),
        )
        .into_any_element()
}

/// 字段下面的一行字：出错的原因（红）或提示（灰）。
pub enum FieldNote {
    Error {
        field: &'static str,
        message: SharedString,
    },
    Hint(SharedString),
}

impl FieldNote {
    /// `field` 出错时的那一行，没出错时什么都没有。
    pub fn error_of(field: &'static str, errors: &FieldErrors) -> Option<Self> {
        errors.get(field).map(|message| FieldNote::Error {
            field,
            message: message.clone(),
        })
    }

    /// 画这一行字：错误是红字（元素 id 是 `<field>-error`，原因写在无障碍名称里），提示是灰字。
    pub fn render(self, tokens: &Tokens) -> AnyElement {
        match self {
            FieldNote::Error { field, message } => div()
                .id(SharedString::from(format!("{field}-error")))
                .test_support()
                .aria_label(message.clone())
                .line_height(relative(1.45))
                .text_color(tokens.err)
                .child(message)
                .into_any_element(),
            FieldNote::Hint(hint) => div()
                .line_height(relative(1.45))
                .text_color(tokens.st_dim)
                .child(hint)
                .into_any_element(),
        }
    }
}

/// 输入框里的字，当秘密拿（密码、口令、主密码、验证码）：用完就清零。
pub fn typed_secret(input: &Entity<InputState>, cx: &App) -> Zeroizing<String> {
    Zeroizing::new(input.read(cx).value().to_string())
}

/// 秘密存过、保险库却锁着时代替输入框的那一行（原型 `.lockedf`）：「已保存，保险库锁定中 · 解锁后修改」，
/// 右边「解锁…」派发 `UnlockVault`。`row`、`unlock` 是这一行和「解锁…」的元素 id。
pub fn locked_secret(
    row: &'static str,
    unlock: &'static str,
    tokens: &Tokens,
    scale: TypeScale,
) -> impl IntoElement + use<> {
    let text = t!("vault.field.locked");
    let accent = tokens.text;
    div()
        .id(row)
        .test_support()
        .aria_label(text.clone())
        .flex()
        .items_center()
        .gap(px(8.))
        .h(size::INPUT)
        .px(px(8.))
        .rounded(size::RADIUS_MD)
        .bg(tokens.input)
        .border_1()
        .border_color(tokens.line_strong)
        .text_size(scale.sm)
        .line_height(px(16.))
        .text_color(tokens.st_dim)
        .child(Icon::new(CatalogIcon::Lock).size(size::ICON_MD))
        .child(div().flex_1().min_w_0().truncate().child(text))
        .child(
            div()
                .id(unlock)
                .test_support()
                .aria_label(t!("vault.field.unlock"))
                .flex_none()
                .text_color(accent)
                .cursor_pointer()
                .on_click(|_, window, cx| window.dispatch_action(Box::new(UnlockVault), cx))
                .child(t!("vault.field.unlock")),
        )
}
