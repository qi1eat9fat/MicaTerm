//! 复选框的方框本体（12 见方）：只画方块和里面的勾/短横，不带行内标签。
//! 弹窗里带标签的一整行（[`modal_checkbox`](super::modal::modal_checkbox)）把它放进整行；
//! SFTP 权限矩阵的每格就是它本身（三态里的「混合」、只读、整格可点）。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::CatalogIcon;
use crate::theme::{Tokens, opacity, size};

/// 勾选框方框的三种样子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    /// 勾上：强调色底、对比色勾。
    Checked,
    /// 不一致（混合）：强调色底、对比色短横。
    Mixed,
    /// 没勾：细边。
    Unchecked,
}

impl CheckState {
    /// 无障碍状态。
    fn toggled(self) -> Toggled {
        match self {
            Self::Checked => Toggled::True,
            Self::Mixed => Toggled::Mixed,
            Self::Unchecked => Toggled::False,
        }
    }
}

/// 复选框的方框本体：尺寸 `CHECKBOX`、圆角 `RADIUS_SM`，三种样子；只读时淡一半、不注册点击。
pub struct CheckBox {
    state: CheckState,
    /// 方框自己的元素 id 和无障碍名称（权限矩阵的每格要能被找到、被读屏）。
    identity: Option<(ElementId, SharedString)>,
    readonly: bool,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl CheckBox {
    /// 一个方框。
    pub fn new(state: CheckState) -> Self {
        Self {
            state,
            identity: None,
            readonly: false,
            on_click: None,
        }
    }

    /// 把元素 id、测试标记和无障碍角色/状态放在方框上（权限矩阵用；带整行标签的
    /// `modal_checkbox` 把它们留在整行上）。`label` 是读屏念的名字，没有时传空串。
    pub fn identity(mut self, id: impl Into<ElementId>, label: SharedString) -> Self {
        self.identity = Some((id.into(), label));
        self
    }

    /// 只读：淡一半、点不动。
    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    /// 点了调用本地回调；只读时不注册。点击要挂在方框自己身上，所以要先 [`identity`](Self::identity)；
    /// 没有元素 id 的方框（`modal_checkbox` 里那个）由外层整行接收点击。
    pub fn on_click(
        mut self,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(on_click));
        self
    }

    /// 画出来。
    pub fn render(self, tokens: &Tokens) -> AnyElement {
        let state = self.state;
        let square = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(size::CHECKBOX)
            .rounded(size::RADIUS_SM);
        let square = match state {
            CheckState::Checked => square
                .bg(tokens.brand)
                .text_color(tokens.on_brand)
                .child(Icon::new(CatalogIcon::Check).size(size::ICON_SM)),
            CheckState::Mixed => square
                .bg(tokens.brand)
                .child(div().w(px(6.)).h(px(1.5)).bg(tokens.on_brand)),
            CheckState::Unchecked => square.border_1().border_color(tokens.line_strong),
        };
        if let Some((id, label)) = self.identity {
            let square = square
                .id(id)
                .test_support()
                .aria_label(label)
                .role(Role::CheckBox)
                .aria_toggled(state.toggled());
            // 只读：淡一半，注册进来的点击也不生效。
            let square = if self.readonly {
                square.opacity(opacity::DISABLED)
            } else if let Some(on_click) = self.on_click {
                square.cursor_pointer().on_click(on_click)
            } else {
                square
            };
            square.into_any_element()
        } else {
            // 带整行标签的 `modal_checkbox` 由整行接收点击，方框本身只负责样子。
            square
                .when(self.readonly, |square| square.opacity(opacity::DISABLED))
                .into_any_element()
        }
    }
}
