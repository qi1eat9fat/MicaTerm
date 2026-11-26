//! 单行输入框/下拉框的框：设置页和模态共用高度、圆角、底色和描边规则。

use gpui_kit::*;

use crate::theme::{Tokens, size};

/// 框的左右内边距。
pub const INSET: Pixels = px(10.);
/// 右侧留给图标/眼睛的内边距。
pub const TRAILING: Pixels = px(6.);

/// 框的描边：出错危险色、聚焦品牌色，否则细线。
pub fn border_color(tokens: &Tokens, invalid: bool, focused: bool) -> Hsla {
    if invalid {
        tokens.err
    } else if focused {
        tokens.brand
    } else {
        tokens.line_strong
    }
}

/// 给框本身套上高度、圆角、输入底色和描边。
pub fn frame<E: Styled>(element: E, tokens: &Tokens, invalid: bool, focused: bool) -> E {
    element
        .h(size::INPUT)
        .rounded(size::RADIUS_MD)
        .bg(tokens.input)
        .border_1()
        .border_color(border_color(tokens, invalid, focused))
}

#[cfg(test)]
mod tests {
    use super::border_color;
    use crate::theme::Tokens;

    #[test]
    fn the_border_follows_error_then_focus_then_line() {
        let tokens = Tokens::light();
        assert_eq!(border_color(&tokens, true, true), tokens.err);
        assert_eq!(border_color(&tokens, false, true), tokens.brand);
        assert_eq!(border_color(&tokens, false, false), tokens.line_strong);
    }
}
