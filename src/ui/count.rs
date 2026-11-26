//! 数量徽章：停靠面板分组头、传输队列标题和批次行共用。
//!
//! 浅底、全圆、小一号字；以前三处各画一份，批次行还比另外两处高 1px、字重轻一档。

use gpui_kit::*;

use crate::theme::{Tokens, TypeScale, WEIGHT_SEMIBOLD};

/// 一个显示数量的胶囊。
pub fn count_badge(count: impl ToString, tokens: &Tokens, scale: TypeScale) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .px(px(5.))
        .h(px(14.))
        .rounded_full()
        .bg(tokens.sel)
        .text_size(scale.xs)
        .line_height(px(14.))
        .font_weight(WEIGHT_SEMIBOLD)
        .text_color(tokens.text2)
        .child(count.to_string())
}
