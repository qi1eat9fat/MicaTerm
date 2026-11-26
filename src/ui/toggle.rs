//! 开关（`docs/ui-standard.md` §4.4）：轨道 32 × 18、圆角全圆，内边距 2，滑块 14。
//! 开是品牌色轨道；关是前景 20% 的轨道。尺寸固定，没有大号小号。
//!
//! 键盘：设置页里 Tab 能停在开关上，聚焦时轨道外有一圈品牌色光环；Enter / 空格切换
//! （gpui 自己把聚焦元素的这两个键映射成 click），点击也会把焦点交给它。
//!
//! gpui-component 的 `Switch` 只有 28 × 16（small）、36 × 20（medium）、44 × 24（large），
//! 都对不上规范，所以设置页统一用这一个。

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{Tokens, opacity};

/// 轨道宽、高和滑块边长（见 §4.4）。
const TRACK_WIDTH: Pixels = px(32.);
const TRACK_HEIGHT: Pixels = px(18.);
const KNOB: Pixels = px(14.);
const INSET: Pixels = px(2.);

/// 设置里的开关。无障碍角色是 switch，`checked()` 读的就是 `aria_toggled`。
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    label: SharedString,
    checked: bool,
    disabled: bool,
    on_click: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
}

impl Toggle {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        checked: bool,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            checked,
            disabled: false,
            on_click: None,
        }
    }

    /// 变灰：现在改不了。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 拨动时把新状态交给调用方。
    pub fn on_click(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let checked = self.checked;
        let disabled = self.disabled;
        let on_click = self.on_click;
        let id = self.id;
        let focus = window
            .use_keyed_state(id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        let click = on_click;
        div()
            .id(id)
            .test_support()
            .role(Role::Switch)
            .aria_label(self.label)
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            // 外圈留 2 给焦点环，轨道本身还是 32 × 18。
            .p(INSET)
            .rounded_full()
            .track_focus(&focus)
            .tab_stop(!disabled)
            .tab_index(0)
            .when(focused && !disabled, |control| {
                control.bg(tokens.brand_focus_ring)
            })
            .when(disabled, |control| control.opacity(opacity::DISABLED))
            .when(!disabled, |control| {
                control
                    .cursor_pointer()
                    // 聚焦后 gpui 会自己把 Enter / 空格映射成 click（key up 时），
                    // 所以键盘切换不用另写 on_key_down。
                    .on_click(move |_, window, cx| {
                        if let Some(handler) = &click {
                            window.focus(&focus, cx);
                            handler(!checked, window, cx);
                        }
                    })
            })
            .child(
                div()
                    .flex_none()
                    .w(TRACK_WIDTH)
                    .h(TRACK_HEIGHT)
                    .p(INSET)
                    .rounded_full()
                    .flex()
                    .items_center()
                    .bg(if checked { tokens.brand } else { tokens.line_strong })
                    .when(checked, |track| track.justify_end())
                    .child(
                        div()
                            .flex_none()
                            .size(KNOB)
                            .rounded_full()
                            .bg(tokens.switch_knob),
                    ),
            )
    }
}
