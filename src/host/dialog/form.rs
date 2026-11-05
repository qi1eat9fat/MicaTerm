//! 表单对话框的通用件，照全部界面原型的样式：分组（`.grp`）、一行字段（`.fr`：左边名称，
//! 右边控件，出错时下面一行红字写原因）、输入框（`.in`）。主机、凭据、命令片段的对话框共用
//! （spec 第 4 节「编辑表单」）。

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::Tokens;

/// 字段名称那一列的宽度（`.fr` 的 `grid-template-columns: 76px 1fr`）。
const LABEL_WIDTH: Pixels = px(76.);

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

/// 一组字段（原型 `.grp`）：分组底、12 圆角、细描边，左上角是组名（`.gt`）。
pub fn form_group(
    title: SharedString,
    rows: impl IntoIterator<Item = AnyElement>,
    tokens: &Tokens,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .mb(px(14.))
        .pt(px(12.))
        .px(px(12.))
        .pb(px(2.))
        .rounded(px(12.))
        .bg(tokens.group)
        .border_1()
        .border_color(tokens.line)
        .child(
            div()
                .mb(px(10.))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.muted)
                .child(title),
        )
        .children(rows)
        .into_any_element()
}

/// 一行字段（原型 `.fr`）：左边名称（分段按钮那一行没有），右边控件，下面是 `notes`：
/// 出错的原因（红字，id 是 `<field>-error`，原因写在无障碍名称里，测试靠它断言）或提示（灰字）。
pub fn form_field(
    label: Option<SharedString>,
    control: impl IntoElement,
    notes: Vec<FieldNote>,
    tokens: &Tokens,
) -> AnyElement {
    div()
        .flex()
        .mb(px(10.))
        .gap(px(10.))
        .child(
            div()
                .flex_none()
                .w(LABEL_WIDTH)
                .h(px(30.))
                .flex()
                .items_center()
                .text_size(px(12.5))
                .text_color(tokens.muted)
                .children(label),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(3.))
                .child(control)
                .children(notes.into_iter().map(|note| note.render(tokens))),
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

    fn render(self, tokens: &Tokens) -> AnyElement {
        match self {
            FieldNote::Error { field, message } => div()
                .id(SharedString::from(format!("{field}-error")))
                .test_support()
                .aria_label(message.clone())
                .text_size(px(11.5))
                .line_height(relative(1.45))
                .text_color(tokens.bad)
                .child(message)
                .into_any_element(),
            FieldNote::Hint(hint) => div()
                .text_size(px(11.5))
                .line_height(relative(1.45))
                .text_color(tokens.faint)
                .child(hint)
                .into_any_element(),
        }
    }
}

/// 输入框（原型 `.in`）：元素 id 就是字段名，出错时描红。
pub fn text_input(
    field: &'static str,
    label: SharedString,
    state: &Entity<InputState>,
    invalid: bool,
    tokens: &Tokens,
) -> Input {
    Input::new(state)
        .id(field)
        .aria_label(label)
        .small()
        .when(invalid, |input| input.border_color(tokens.bad))
}
