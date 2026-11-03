//! 标签轨道里的标签（原型 `.track` 里的 `.tab`）：宽 180、高 28，程序图块、名称和目录。
//! 渲染函数不读实体，只拿窗口壳取好的快照。
//! 02 只画标签；工单 04 加点击切换、悬停的 ×、右键菜单和横向滚动，工单 06 加图块叠放和状态点。

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::TabId;
use crate::card::render_program_tile;
use crate::theme::{TAB_HEIGHT, TAB_TRACK_HEIGHT, TAB_WIDTH, Tokens};

/// 画一个标签要的东西。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabSnapshot {
    pub id: TabId,
    /// 焦点卡片的名称（shell 名、主机名）。
    pub name: SharedString,
    /// 焦点卡片的目录。
    pub directory: SharedString,
    pub active: bool,
}

/// 轨道（底色圆角条）和里面的标签；没有标签时什么都不画。
pub fn render_tabs(tabs: Vec<TabSnapshot>, tokens: &Tokens) -> Option<impl IntoElement> {
    if tabs.is_empty() {
        return None;
    }
    Some(
        div()
            .flex()
            .items_center()
            .h(TAB_TRACK_HEIGHT)
            .p(px(2.))
            .rounded(px(16.))
            .bg(tokens.track)
            .min_w_0()
            .overflow_hidden()
            .children(tabs.into_iter().map(|tab| render_tab(tab, tokens))),
    )
}

fn render_tab(tab: TabSnapshot, tokens: &Tokens) -> impl IntoElement {
    let hover = tokens.hover;
    let shadow = tokens.shadow;
    div()
        .id(("tab", tab.id.0 as usize))
        .test_support()
        .aria_label(format!("{} {}", tab.name, tab.directory))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(9.))
        .w(TAB_WIDTH)
        .h(TAB_HEIGHT)
        .pl(px(6.))
        .pr(px(4.))
        .rounded(px(14.))
        .cursor_pointer()
        .when(tab.active, |this| {
            this.bg(tokens.tab_active).shadow(vec![
                BoxShadow {
                    color: shadow,
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: shadow,
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(8.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
        })
        .when(!tab.active, |this| this.hover(move |style| style.bg(hover)))
        .child(render_program_tile(
            ("tab-tile", tab.id.0 as usize).into(),
            tokens,
        ))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .gap(px(4.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if tab.active {
                    tokens.text
                } else {
                    tokens.muted
                })
                .child(div().flex_none().child(tab.name))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(tokens.faint)
                        .child(tab.directory),
                ),
        )
}
