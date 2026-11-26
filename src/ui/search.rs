//! 紧凑搜索框：主机栏（填充底）和卡片选择器（下划线头）共用。
//!
//! 两处只有外观不同（高度、内边距、清空按钮大小、是否填充底）；搜索图标、输入框、
//! 清空按钮的悬停/提示/无障碍名称都走这里。

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{Tokens, TypeScale, size, space};
use crate::ui::icon_button::TextTooltip;

/// 搜索框的外观。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchChrome {
    /// 主机栏：高 20、填充底（聚焦时更深）、左右 7。
    Filled,
    /// 卡片选择器：高 24、底部分隔线、左右 8。
    Underline,
}

/// 一条带输入框和清空按钮的搜索栏。
pub struct SearchBar<'a> {
    bar_id: &'static str,
    input: &'a Entity<InputState>,
    input_id: &'static str,
    input_label: SharedString,
    clear_id: &'static str,
    clear_label: SharedString,
    has_query: bool,
    focused: bool,
    grow: bool,
    chrome: SearchChrome,
    tokens: &'a Tokens,
    scale: TypeScale,
    on_clear: Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
}

impl<'a> SearchBar<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bar_id: &'static str,
        input: &'a Entity<InputState>,
        input_id: &'static str,
        input_label: impl Into<SharedString>,
        clear_id: &'static str,
        clear_label: impl Into<SharedString>,
        chrome: SearchChrome,
        tokens: &'a Tokens,
        scale: TypeScale,
        on_clear: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            bar_id,
            input,
            input_id,
            input_label: input_label.into(),
            clear_id,
            clear_label: clear_label.into(),
            has_query: false,
            focused: false,
            grow: false,
            chrome,
            tokens,
            scale,
            on_clear: Box::new(on_clear),
        }
    }

    /// 有查询时才画清空按钮。
    pub fn query(mut self, has_query: bool) -> Self {
        self.has_query = has_query;
        self
    }

    /// 填充底在聚焦时更深。
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// 占满剩下的宽度（主机栏），否则按内容宽度（选择器）。
    pub fn grow(mut self, grow: bool) -> Self {
        self.grow = grow;
        self
    }

    /// 画出来；调用方可以继续在返回的元素上挂按键处理。
    pub fn render(self) -> Stateful<Div> {
        let (height, padding, icon_size, clear_size) = match self.chrome {
            SearchChrome::Filled => (size::SEARCH_INPUT, px(7.), px(11.), px(16.)),
            SearchChrome::Underline => (size::CONTROL_MD, space::SPACE_8, size::ICON_SM, px(18.)),
        };
        let mut bar = div()
            .id(self.bar_id)
            .flex()
            .items_center()
            .gap(space::SPACE_6)
            .h(height)
            .px(padding)
            .text_size(self.scale.sm)
            .line_height(px(14.))
            .text_color(self.tokens.st_dim);
        bar = if self.grow {
            bar.flex_1().min_w_0()
        } else {
            bar.flex_none()
        };
        bar = match self.chrome {
            SearchChrome::Filled => bar.rounded(size::RADIUS_MD).bg(if self.focused {
                self.tokens.sel
            } else {
                self.tokens.hover
            }),
            SearchChrome::Underline => bar.border_b(size::LINE).border_color(self.tokens.line),
        };
        let hover = self.tokens.hover;
        let text = self.tokens.text;
        let clear_label = self.clear_label;
        bar.child(Icon::new(IconName::Search).size(icon_size))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(self.input)
                        .id(self.input_id)
                        .aria_label(self.input_label)
                        .appearance(false)
                        .small()
                        .px_0()
                        .text_size(self.scale.sm)
                        .line_height(px(14.)),
                ),
            )
            .when(self.has_query, |bar| {
                let tooltip = clear_label.clone();
                bar.child(
                    div()
                        .id(self.clear_id)
                        .test_support()
                        .aria_label(clear_label.clone())
                        .tooltip(move |_, cx| TextTooltip::view(tooltip.clone(), cx))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(clear_size)
                        .rounded(size::RADIUS_SM)
                        .cursor_pointer()
                        .hover(move |style| style.bg(hover).text_color(text))
                        .on_mouse_down(MouseButton::Left, |_, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                        })
                        .on_click(self.on_clear)
                        .child(Icon::new(IconName::Close).size(px(10.))),
                )
            })
    }
}
