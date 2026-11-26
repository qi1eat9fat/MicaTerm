//! 程序图块与小徽章：一律实心圆角方块——`tile_bg` 做底、`tile_fg` 画图标（或写字），
//! 浅色底描一圈细边；断开时方块变灰。标签、卡片标题行和新建卡片面板共用这一种画法。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::programs::Program;
use crate::theme::{Tokens, TypeScale, WEIGHT_SEMIBOLD, size};

/// 标签和卡片顶行前面的实心方块（边长 `size::TAB_BADGE`）。
pub fn render_tab_tile(
    id: ElementId,
    program: &Program,
    tokens: &Tokens,
) -> impl IntoElement + use<> {
    let scale = TypeScale::from_base(crate::theme::INTERFACE_FONT_SIZE);
    // 断开的卡片：仍是同样大小的实心方块，只是褪成中性灰、图标用底色挖空，
    // 免得底色太浅让方块看着比别的图块小一圈。
    let (background, ink): (Hsla, Hsla) = if program.off {
        (tokens.faint, tokens.content)
    } else {
        (rgb(program.tile_bg).into(), rgb(program.tile_fg).into())
    };
    let glyph = match &program.letter {
        Some(letter) => div()
            .text_size(scale.xs)
            .line_height(scale.xs)
            .font_weight(WEIGHT_SEMIBOLD)
            .child(letter.clone())
            .into_any_element(),
        None => Icon::default()
            .path(program.glyph)
            .size(size::TAB_BADGE_GLYPH)
            .into_any_element(),
    };
    div()
        .id(id)
        .test_support()
        .aria_label(program.name.clone())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::TAB_BADGE)
        .rounded(size::RADIUS_SM)
        .bg(background)
        .text_color(ink)
        // 浅色底的品牌图块（Codex、Bun）描一圈细边，在白底上分得开。
        .when(program.border, |tile| {
            tile.border_1().border_color(tokens.line)
        })
        .child(glyph)
}
