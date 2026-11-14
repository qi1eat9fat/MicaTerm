//! 程序图块：标签前面那一块（26 号 `.bdg`），彩色底加单色 `Icon`，颜色和图标由程序名表
//! （`crate::programs`）查出来。卡片没有头部、标签不叠放（26 号），只有这一种。
// 参考 gorex 的模型重写：tabs.go 的 `drawTile`。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::programs::Program;
use crate::theme::{Tokens, size};

/// 标签前面那一块的边长（26 号 `.bdg`）。
const TAB_TILE: Pixels = px(14.);

/// 标签前面的那一块（26 号 `.bdg`）：14×14、圆角 4 的方块，只放焦点卡片的一块，不叠放。
/// 远程卡片没认出程序时是系统徽章（系统标志，或者名称首字），认出时是程序的图块。
/// 无障碍名称是程序名。
pub fn render_tab_tile(
    id: ElementId,
    program: &Program,
    tokens: &Tokens,
) -> impl IntoElement + use<> {
    let (background, ink): (Hsla, Hsla) = if program.off {
        (tokens.faint, rgb(0xffffff).into())
    } else {
        (rgb(program.tile_bg).into(), rgb(program.tile_fg).into())
    };
    div()
        .id(id)
        .test_support()
        .aria_label(program.name.clone())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(TAB_TILE)
        .rounded(px(4.))
        .bg(background)
        .text_color(ink)
        .when(program.border && !program.off, |tile| {
            tile.border(px(0.5)).border_color(hsla(0., 0., 0., 0.12))
        })
        .map(|tile| match &program.letter {
            Some(letter) => tile
                .text_size(size::BADGE_TEXT)
                .font_weight(FontWeight::BOLD)
                .line_height(px(9.))
                .child(letter.clone()),
            None => tile.child(Icon::default().path(program.glyph).size(px(10.))),
        })
}
