//! 原型里的图标按钮（`.ib`）：圆角方块，悬停时加底色。点了经窗口壳的焦点派发 action。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{ICON_BUTTON, ICON_BUTTON_RADIUS, Tokens};

/// 图标按钮。`label` 是无障碍名称，测试也靠它断言。
pub struct IconButton {
    id: ElementId,
    icon: Icon,
    label: SharedString,
    size: Pixels,
    radius: Pixels,
    /// 显示成按下的样子（右侧栏里展开着面板的图标，原型 `.rail .ib.on`）。
    pressed: bool,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: impl Into<Icon>, label: SharedString) -> Self {
        Self {
            id: id.into(),
            icon: icon.into(),
            label,
            size: ICON_BUTTON,
            radius: ICON_BUTTON_RADIUS,
            pressed: false,
        }
    }

    /// 边长和圆角，默认是原型 `.ib` 的 26 / 10。图标约占边长的 0.6。
    pub fn size(mut self, size: Pixels, radius: Pixels) -> Self {
        self.size = size;
        self.radius = radius;
        self
    }

    /// 显示成按下的样子：底色同活动标签，带阴影。
    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
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
            .text_color(tokens.icon)
            .when(self.pressed, |button| {
                button
                    .bg(tokens.tab_active)
                    .text_color(tokens.text)
                    .shadow(vec![BoxShadow {
                        color: tokens.shadow_focused,
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(4.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
            })
            .cursor_pointer()
            .hover(move |style| style.bg(hover).text_color(text))
            .on_click(move |_, window, cx| target.dispatch_action(action.as_ref(), window, cx))
            .child(self.icon.size(self.size * 0.6))
    }
}
