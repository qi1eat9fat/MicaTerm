//! 26 号的图标按钮（`.ib`）：圆角 6 的方块，悬停加 `--hover` 底；按下（展开着面板、当前所在）是 `--sel` 底。
//! 点了经窗口壳的焦点派发 action。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{MONO_FONT_FAMILY, Tokens, TypeScale, size};

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
    /// 悬停时的提示。
    tooltip: Option<SharedString>,
    /// 提示后面淡色写这个 action 的快捷键（有绑定时）。
    tooltip_keys: Option<Box<dyn Action>>,
    /// 图标的颜色，默认 `--icon`。
    icon_color: Option<Hsla>,
}

/// 悬停时的一句提示（深底浅字、圆角 6）。图标按钮用它，列表行要悬停看全文时也用它。
/// 后面可以淡色跟一个快捷键（`向右分屏  Ctrl+Shift+D`）。
pub struct TextTooltip {
    text: SharedString,
    keys: Option<SharedString>,
}

impl TextTooltip {
    /// 给 `.tooltip(…)` 用的视图。
    pub fn view(text: SharedString, cx: &mut App) -> AnyView {
        Self::with_keys(text, None, cx)
    }

    /// 带快捷键的提示。
    pub fn with_keys(text: SharedString, keys: Option<SharedString>, cx: &mut App) -> AnyView {
        cx.new(|_| Self { text, keys }).into()
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
        div()
            .id("icon-button-tooltip")
            .test_support()
            .aria_label(self.text.clone())
            .flex()
            .items_baseline()
            .gap(px(10.))
            .px(px(8.))
            .py(px(4.))
            .rounded(size::RADIUS)
            .bg(tokens.tooltip)
            .text_color(tokens.tooltip_text)
            .text_size(scale.ui)
            .child(self.text.clone())
            .when_some(self.keys.clone(), |tip, keys| {
                tip.child(
                    div()
                        .font_family(MONO_FONT_FAMILY)
                        .text_size(scale.mono_small)
                        .text_color(tokens.faint)
                        .child(keys),
                )
            })
    }
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: impl Into<Icon>, label: SharedString) -> Self {
        Self {
            id: id.into(),
            icon: icon.into(),
            label,
            size: size::BUTTON,
            icon_size: size::ICON,
            radius: size::RADIUS,
            pressed: false,
            primary: false,
            dimmed: false,
            tooltip: None,
            tooltip_keys: None,
            icon_color: None,
        }
    }

    /// 按钮边长和图标边长，默认 28 / 16。
    pub fn size(mut self, size: Pixels, icon_size: Pixels) -> Self {
        self.size = size;
        self.icon_size = icon_size;
        self
    }

    /// 圆角，默认 6。
    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = radius;
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
    ) -> impl IntoElement + use<> {
        let target = target.clone();
        let hover = tokens.hover;
        let text = tokens.text;
        let dimmed = self.dimmed;
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
            .when_some(self.tooltip, |button, tooltip| {
                let keys = self.tooltip_keys;
                button.tooltip(move |window, cx| {
                    let keys = keys
                        .as_ref()
                        .and_then(|action| keys_of(action.as_ref(), window));
                    TextTooltip::with_keys(tooltip.clone(), keys, cx)
                })
            })
            .on_click(move |_, window, cx| target.dispatch_action(action.as_ref(), window, cx))
            .child(self.icon.size(self.icon_size))
    }
}
