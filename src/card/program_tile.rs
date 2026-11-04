//! 程序图块（21×16，圆角 4.5，外圈 1px）：卡片头部最左边那块，和标签上叠放的那一叠。
//! 彩色底加单色 `Icon`，颜色和图标由程序名表（`crate::programs`）查出来。
// 参考 gorex 的模型重写：tabs.go 的 `tiles`、`drawTile`；叠放的样子取全部界面原型的 `.tile`。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::programs::Program;
use crate::theme::{TILE_HEIGHT, TILE_WIDTH, Tokens};

/// 叠在后面的图块每块往右错开多少。
const TILE_STEP: Pixels = px(5.);
/// 标签上最多叠几块。
pub const MAX_STACKED_TILES: usize = 3;

/// 一块图块：`glyph` 为假时只画底色（叠在后面的那几块）。
fn tile(program: &Program, glyph: bool, tokens: &Tokens) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(TILE_WIDTH)
        .h(TILE_HEIGHT)
        .rounded(px(4.5))
        .bg(rgb(program.tile_bg))
        .text_color(rgb(program.tile_fg))
        .when(program.border, |tile| {
            tile.border(px(0.5)).border_color(hsla(0., 0., 0., 0.12))
        })
        .shadow(vec![
            BoxShadow {
                color: tokens.rim,
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 0., 0.18),
                offset: point(px(0.), px(0.5)),
                blur_radius: px(1.5),
                spread_radius: px(0.),
                inset: false,
            },
        ])
        .when(glyph, |tile| {
            tile.child(Icon::default().path(program.glyph).size(px(11.)))
        })
}

/// 卡片头部的图块。`id` 给测试查，无障碍名称是程序名。
pub fn render_program_tile(
    id: ElementId,
    program: &Program,
    tokens: &Tokens,
) -> impl IntoElement + use<> {
    tile(program, true, tokens)
        .id(id)
        .test_support()
        .aria_label(program.name.clone())
}

/// 标签上叠放的图块：`programs[0]`（焦点卡片的）在最前面、带图标，后面的往右错开 5、
/// 上下各缩 0.8 往里藏，只露出边。最多 `MAX_STACKED_TILES` 块。
/// 无障碍名称是从前到后的程序名，用 ` · ` 隔开。
pub fn render_tile_stack(
    id: ElementId,
    programs: &[Program],
    tokens: &Tokens,
) -> impl IntoElement + use<> {
    let programs = &programs[..programs.len().min(MAX_STACKED_TILES)];
    let count = programs.len().max(1);
    let label = programs
        .iter()
        .map(|program| program.name.as_str())
        .collect::<Vec<_>>()
        .join(" · ");
    // 后面的先画，前面的盖在上面。
    let tiles = programs.iter().enumerate().rev().map(|(index, program)| {
        let inset = px(0.8) * index.min(1) as f32;
        tile(program, index == 0, tokens)
            .absolute()
            .left(TILE_STEP * index as f32)
            .top(inset)
            .h(TILE_HEIGHT - inset * 2.)
    });
    div()
        .id(id)
        .test_support()
        .aria_label(label)
        .relative()
        .flex_none()
        .w(TILE_WIDTH + TILE_STEP * (count - 1) as f32)
        .h(TILE_HEIGHT)
        .children(tiles)
}
