//! 拖动排序共用的界面件（主机栏的主机、标题栏的标签）：
//! - [`DragSlot`]：拖动中的状态，拖动在别处放下或取消后自动作废；
//! - [`fade_while_dragged`]：被拖着的那一项变淡，跟着鼠标的是它的预览；
//! - [`render_drop_line`]：插入线，骑在要放下的那条缝上。

use std::cell::Cell;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::theme::Tokens;

/// 被拖着的那一项的不透明度。
const DRAGGED_OPACITY: f32 = 0.45;
/// 插入线的粗细。
const LINE_THICKNESS: Pixels = px(2.);

/// 拖动中的状态（拖着谁、要放到哪里）。拖动在别处放下、或者拖出窗口取消了，
/// 放下的处理器不会被调到，所以每帧渲染开头 [`settle`](Self::settle) 一次，把过期的清掉，
/// 插入线和变淡跟着收掉。
pub struct DragSlot<T>(Cell<Option<T>>);

impl<T> Default for DragSlot<T> {
    fn default() -> Self {
        Self(Cell::new(None))
    }
}

impl<T: Copy + PartialEq> DragSlot<T> {
    /// 窗口里已经没有拖动了就清掉。
    pub fn settle(&self, cx: &App) {
        if !cx.has_active_drag() {
            self.0.set(None);
        }
    }

    pub fn get(&self) -> Option<T> {
        self.0.get()
    }

    /// 记下拖动的这一刻；和原来一样时返回 `false`（不用重画）。
    pub fn set(&self, state: Option<T>) -> bool {
        self.0.replace(state) != state
    }

    pub fn take(&self) -> Option<T> {
        self.0.take()
    }
}

/// `dragged` 时把这一项变淡。
pub fn fade_while_dragged<E: Styled + FluentBuilder>(element: E, dragged: bool) -> E {
    element.when(dragged, |element| element.opacity(DRAGGED_OPACITY))
}

/// 插入线贴在元素的哪条边上。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropEdge {
    Top,
    Bottom,
    Left,
    Right,
}

/// 插入线：强调色、2px 粗的圆头线，绝对定位在元素（要 `relative`）的 `edge` 边上，
/// 沿着这条边两头各缩进 `inset`；`shift` 是线的中线再往边外挪多少（让线骑在两项之间的竖线上）。
pub fn render_drop_line(
    id: ElementId,
    edge: DropEdge,
    inset: Pixels,
    shift: Pixels,
    tokens: &Tokens,
) -> impl IntoElement + use<> {
    let outside = -(LINE_THICKNESS / 2. + shift);
    div()
        .id(id)
        .test_support()
        .absolute()
        .rounded(LINE_THICKNESS / 2.)
        .bg(tokens.accent)
        .map(|line| match edge {
            DropEdge::Top | DropEdge::Bottom => line.left(inset).right(inset).h(LINE_THICKNESS),
            DropEdge::Left | DropEdge::Right => line.top(inset).bottom(inset).w(LINE_THICKNESS),
        })
        .map(|line| match edge {
            DropEdge::Top => line.top(outside),
            DropEdge::Bottom => line.bottom(outside),
            DropEdge::Left => line.left(outside),
            DropEdge::Right => line.right(outside),
        })
}
