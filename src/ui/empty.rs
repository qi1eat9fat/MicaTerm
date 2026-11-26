//! 列表或面板没有内容时的那一句话（主机栏、凭据、片段、选择器、文件列表）。
//!
//! 各处以前各画一份；这里按放置位置统一尺寸、颜色、第二行提示和图标块，
//! 调用方只给元素 id、文字和可选的提示/图标。

use gpui_kit::component::Icon;
use gpui_kit::*;

use crate::theme::{Tokens, TypeScale, size, space};

/// 空状态放在什么容器里。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmptyPlacement {
    /// 停靠面板（凭据、片段）：靠上，上下留白 16 / 24。
    Dock,
    /// 填满高度、垂直居中，底部留 40（主机栏没有主机）。
    HostCentered,
    /// 列表顶部留 16（搜索没有匹配）。
    HostTop,
    /// 卡片选择器：一行，左右 10、上下 14，小一号字。
    Picker,
    /// 跳板主机选择器：一块，左右 10、上下 6，靠左。
    RoutePicker,
    /// 文件列表：填满、居中，上面一块 28 的图标底。
    FileList,
}

/// 一句话的空状态。
pub struct EmptyState {
    id: ElementId,
    text: SharedString,
    hint: Option<SharedString>,
    icon: Option<Icon>,
    placement: EmptyPlacement,
}

impl EmptyState {
    pub fn new(
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
        placement: EmptyPlacement,
    ) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            hint: None,
            icon: None,
            placement,
        }
    }

    /// 下面小一号的提示。
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// 只在 [`EmptyPlacement::FileList`] 用：图标画在 28 见方的浅底块里。
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// 画出来。
    pub fn render(self, tokens: &Tokens, scale: TypeScale) -> AnyElement {
        let Self {
            id,
            text,
            hint,
            icon,
            placement,
        } = self;
        let mut container = div()
            .id(id)
            .test_support()
            .aria_label(text.clone())
            .flex()
            .items_center();
        if placement != EmptyPlacement::Picker {
            container = container.flex_col();
        }
        let container = match placement {
            EmptyPlacement::Dock => container
                .gap(space::SPACE_4)
                .px(space::SPACE_16)
                .py(space::SPACE_24),
            EmptyPlacement::HostCentered => container
                .px(space::SPACE_8)
                .flex_1()
                .justify_center()
                .pb(px(40.)),
            EmptyPlacement::HostTop => container.px(space::SPACE_8).pt(space::SPACE_16),
            EmptyPlacement::Picker => container.justify_center().px(px(10.)).py(px(14.)),
            EmptyPlacement::RoutePicker => container.items_start().px(px(10.)).py(space::SPACE_6),
            EmptyPlacement::FileList => container
                .gap(space::SPACE_8)
                .flex_1()
                .justify_center()
                .px(px(16.)),
        };
        let text_size = match placement {
            EmptyPlacement::Picker => scale.sm,
            _ => scale.body,
        };
        container
            .text_size(text_size)
            .text_color(tokens.st_dim)
            .children(icon.map(|icon| {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .rounded(size::RADIUS_LG)
                    .bg(tokens.hover)
                    .text_color(tokens.faint)
                    .child(icon.size(size::ICON_MD))
            }))
            .child(match placement {
                EmptyPlacement::Picker => div().text_center().child(text),
                _ => div().child(text),
            })
            .children(hint.map(|hint| {
                div()
                    .text_size(scale.sm)
                    .text_color(tokens.faint)
                    .text_center()
                    .child(hint)
            }))
            .into_any_element()
    }
}
