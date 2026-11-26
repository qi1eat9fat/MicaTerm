//! 26 号的图标按钮（`.ib`）：圆角 6 的方块，悬停加 `--hover` 底；按下（展开着面板、当前所在）是 `--sel` 底。
//! 点了经窗口壳的焦点派发 action。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{Tokens, TypeScale, size};

/// 图标按钮。`label` 是无障碍名称，测试也靠它断言。
pub struct IconButton {
    id: ElementId,
    icon: Icon,
    label: SharedString,
    size: Pixels,
    icon_size: Pixels,
    radius: Pixels,
    /// 按下的样子：右侧栏里展开着面板的图标、当前所在的「文件」（26 号 `.rail .ib.on`）。
    pressed: bool,
    /// 常驻的浅底和细边，图标是正文色（侧栏顶部的「新建主机」，26 号 `.ib.pri`）。
    primary: bool,
    /// 变灰：现在做不了，悬停说为什么。
    dimmed: bool,
    /// 自定义悬停提示；未指定时使用按钮的无障碍名称。
    tooltip: Option<SharedString>,
    /// 提示后面淡色写这个 action 的快捷键（有绑定时）。
    tooltip_keys: Option<Box<dyn Action>>,
    /// 图标的颜色，默认 `--icon`。
    icon_color: Option<Hsla>,
}

/// 悬停时的一句提示（深底浅字、圆角 6）。图标按钮用它，列表行要悬停看全文时也用它。
/// 后面可以淡色跟一个快捷键（`向右分屏  Ctrl+Shift+D`）。
pub struct TextTooltip {
    id: &'static str,
    text: SharedString,
    detail: Option<SharedString>,
    keys: Option<SharedString>,
}

impl TextTooltip {
    /// 给 `.tooltip(…)` 用的视图。
    pub fn view(text: SharedString, cx: &mut App) -> AnyView {
        Self::build("icon-button-tooltip", text, None, None, cx)
    }

    /// 带快捷键的提示。
    pub fn with_keys(text: SharedString, keys: Option<SharedString>, cx: &mut App) -> AnyView {
        Self::build("icon-button-tooltip", text, None, keys, cx)
    }

    /// 两行提示：第一行地址/名称，第二行备注。`id` 给需要断言提示的调用方。
    pub fn with_detail(
        id: &'static str,
        text: SharedString,
        detail: Option<SharedString>,
        cx: &mut App,
    ) -> AnyView {
        Self::build(id, text, detail, None, cx)
    }

    fn build(
        id: &'static str,
        text: SharedString,
        detail: Option<SharedString>,
        keys: Option<SharedString>,
        cx: &mut App,
    ) -> AnyView {
        cx.new(|_| Self {
            id,
            text,
            detail,
            keys,
        })
        .into()
    }
}

/// `action` 的快捷键写成 `Ctrl+Shift+D`；没有绑定时是 `None`。
fn keys_of(action: &dyn Action, window: &Window) -> Option<SharedString> {
    let binding = window.highest_precedence_binding_for_action(action)?;
    let stroke = binding.keystrokes().first()?;
    Some(gpui_kit::component::kbd::Kbd::format(stroke.as_keystroke()).into())
}

impl Render for TextTooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let label = match &self.detail {
            Some(detail) => format!("{}\n{detail}", self.text),
            None => self.text.to_string(),
        };
        // 长文本（一整条命令、长路径）不能把提示条撑成横贯窗口的一条：限宽换行，最多四行。
        let tip = div()
            .id(self.id)
            .test_support()
            .aria_label(label)
            .max_w(size::TOOLTIP_MAX_WIDTH)
            .line_clamp(size::TOOLTIP_MAX_LINES)
            .px(px(8.))
            .py(px(4.))
            .rounded(size::RADIUS_MD)
            .bg(tokens.tooltip)
            .text_color(tokens.tooltip_text)
            .text_size(scale.body);
        if self.detail.is_some() {
            tip.flex()
                .flex_col()
                .line_height(relative(1.45))
                .child(self.text.clone())
                .children(self.detail.clone())
        } else {
            tip.flex()
                .items_baseline()
                .gap(px(10.))
                .child(self.text.clone())
                .when_some(self.keys.clone(), |tip, keys| {
                    tip.child(
                        div()
                            .flex_none()
                            .text_size(scale.body)
                            .text_color(tokens.faint)
                            .child(keys),
                    )
                })
        }
    }
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: impl Into<Icon>, label: SharedString) -> Self {
        Self {
            id: id.into(),
            icon: icon.into(),
            label,
            size: size::CONTROL_MD,
            icon_size: size::ICON_MD,
            radius: size::RADIUS_SM,
            pressed: false,
            primary: false,
            dimmed: false,
            tooltip: None,
            tooltip_keys: None,
            icon_color: None,
        }
    }

    /// Compact 20×20 icon button (12px icon, 4px radius).
    pub fn sm(mut self) -> Self {
        self.size = size::CONTROL_SM;
        self.icon_size = size::ICON_SM;
        self.radius = size::RADIUS_SM;
        self
    }

    /// Default 24×24 icon button (14px icon, 4px radius).
    pub fn md(mut self) -> Self {
        self.size = size::CONTROL_MD;
        self.icon_size = size::ICON_MD;
        self.radius = size::RADIUS_SM;
        self
    }

    /// 图标本身的边长（默认跟着 `sm` / `md` 走）。个别图标画出来偏大（刷新、转圈）时单独调小一号。
    pub fn icon_size(mut self, icon_size: Pixels) -> Self {
        self.icon_size = icon_size;
        self
    }

    /// 按下的样子：`--sel` 底，图标正文色。
    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    /// 常驻浅底和细边（`.ib.pri`）。
    pub fn primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }

    /// 变灰：现在做不了。
    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    /// 悬停时的提示。
    pub fn tooltip(mut self, tooltip: SharedString) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    /// 提示后面淡色写 `action` 的快捷键。
    pub fn tooltip_keys(mut self, action: Box<dyn Action>) -> Self {
        self.tooltip_keys = Some(action);
        self
    }

    /// 图标的颜色（非焦点卡片状态栏上的按钮跟着淡）。
    pub fn icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }

    /// 点了经 `target`（窗口壳的焦点）派发 `action`：焦点为空时也能送到窗口壳。
    pub fn dispatch(
        self,
        action: Box<dyn Action>,
        target: &FocusHandle,
        tokens: &Tokens,
    ) -> AnyElement {
        let target = target.clone();
        self.render(
            move |_, window, cx| target.dispatch_action(action.as_ref(), window, cx),
            tokens,
        )
    }

    /// 点了直接调用本地回调，不依赖窗口壳、不派发 action（预览头部的缩放、关闭这类）。
    /// 尺寸、悬停、提示、置灰、按下、常驻底、图标颜色都和 [`dispatch`](Self::dispatch) 一致。
    pub fn on_click<F>(self, on_click: F, tokens: &Tokens) -> AnyElement
    where
        F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    {
        self.render(on_click, tokens)
    }

    /// 两种点击出口共用同一段渲染，保证样子完全相同。
    fn render<F>(self, on_click: F, tokens: &Tokens) -> AnyElement
    where
        F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    {
        let hover = tokens.hover;
        let text = tokens.text;
        let dimmed = self.dimmed;
        let tooltip = self.tooltip.unwrap_or_else(|| self.label.clone());
        let keys = self.tooltip_keys;
        div()
            .id(self.id)
            .test_support()
            .aria_label(self.label)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(self.size)
            .rounded(self.radius)
            .text_color(self.icon_color.unwrap_or(tokens.icon))
            .when(self.primary, |button| {
                button
                    .bg(tokens.hover)
                    .border_1()
                    .border_color(tokens.line)
                    .text_color(tokens.text)
            })
            .when(self.pressed, |button| {
                button.bg(tokens.sel).text_color(tokens.text)
            })
            .when(dimmed, |button| button.opacity(0.35))
            .when(!dimmed, |button| {
                button
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover).text_color(text))
            })
            .tooltip(move |window, cx| {
                let keys = keys
                    .as_ref()
                    .and_then(|action| keys_of(action.as_ref(), window));
                TextTooltip::with_keys(tooltip.clone(), keys, cx)
            })
            .on_click(on_click)
            .child(self.icon.size(self.icon_size))
            .into_any_element()
    }
}
