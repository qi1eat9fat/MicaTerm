//! 程序图块（21×16，圆角 4.5，外圈 1px）：卡片头部最左边、标签上叠放的那块。
//! 02 只有 shell 图块（原型 `PROG.pwsh` 的 #334155 / #86efac）；工单 06 换成程序名表查出来的。

use gpui_kit::component::Icon;
use gpui_kit::*;

use crate::app::CatalogIcon;
use crate::theme::{TILE_HEIGHT, TILE_WIDTH, Tokens};

/// shell 的图块。`id` 给测试查。
pub fn render_program_tile(id: ElementId, tokens: &Tokens) -> impl IntoElement + use<> {
    div()
        .id(id)
        .test_support()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(TILE_WIDTH)
        .h(TILE_HEIGHT)
        .rounded(px(4.5))
        .bg(rgb(0x334155))
        .text_color(rgb(0x86efac))
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
        .child(Icon::new(CatalogIcon::Terminal).size(px(11.)))
}
