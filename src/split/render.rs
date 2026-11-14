//! 照着分屏树画：每个分支嵌套一层 `h_resizable` / `v_resizable`，叶子由调用方画。
//!
//! 树是唯一的事实来源，gpui 的 resizable 组件（`ResizableState`）按像素记尺寸，只是树的镜像：
//! - 每条分隔线按 `NodeId` 留一个 `ResizableState`，跟着树增删；
//! - 树的比例变了（键盘推分隔线、双击复位、拖到界外），调 `show_ratio` 把像素推过去；
//! - 拖完间隙，resizable 组件在松开鼠标时派发 `ResizeDivider`，窗口壳把比例写回树；
//! - resizable 组件默认的最小尺寸是 100px，和 0.08 的下限对不上：按上一帧量到的容器尺寸
//!   把两边限制在 0.08–0.92 之间，拖的时候就停在界限上。
//!
//! 分隔：窗格之间没有间隙，只有一条 1px 细线（26 号 `.sep-line`）。库自己的拖动柄（宽 9、绝对定位）
//! 骑在两个窗格的交界上，线画在柄的正中；柄里是 `("divider", NodeId)`，双击它恢复成一半。

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::base::{
    ResizableState, ResizeHandleContext, ResizeHandleRenderer, h_resizable, resizable_panel,
    v_resizable,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::tree::{Axis, MAX_RATIO, MIN_RATIO, Node, NodeId, SplitTree};
use crate::app::{ResetDivider, ResizeDivider};
use crate::card::CardId;
use crate::theme::{Tokens, size};

/// 一条分隔线在 gpui 里的镜像：它那个 resizable 组件的状态，和量到的容器长度。
struct DividerMirror {
    state: Entity<ResizableState>,
    /// 上一帧量到的容器沿分屏方向的长度：按它换算 0.08–0.92 的像素界限。
    extent: Rc<Cell<Pixels>>,
}

/// 一棵分屏树全部分隔线在 gpui 里的镜像，按分隔线的 `NodeId` 存。
#[derive(Default)]
pub struct DividerMirrors {
    mirrors: HashMap<NodeId, DividerMirror>,
}

impl DividerMirrors {
    /// 树里每条分隔线都有镜像，删掉的分隔线的镜像丢掉。分屏、关卡片以后调。
    pub fn sync(&mut self, tree: &SplitTree, cx: &mut App) {
        let mut nodes = Vec::new();
        collect_dividers(tree.root(), &mut nodes);
        self.mirrors.retain(|node, _| nodes.contains(node));
        for node in nodes {
            self.mirrors.entry(node).or_insert_with(|| DividerMirror {
                state: cx.new(|_| ResizableState::default()),
                extent: Rc::new(Cell::new(px(0.))),
            });
        }
    }

    /// 把树里分隔线 `node` 的比例推给它的 resizable 组件：键盘推分隔线、双击复位、拖到界外以后调。
    /// 还没量过尺寸（这一帧刚分出来）时什么都不做，第一帧本来就是一半。
    pub fn show_ratio(&self, node: NodeId, ratio: f32, window: &mut Window, cx: &mut App) {
        let Some(mirror) = self.mirrors.get(&node) else {
            return;
        };
        mirror.state.update(cx, |state, cx| {
            let total: f32 = state.sizes().iter().map(|size| f32::from(*size)).sum();
            if state.sizes().len() != 2 || total <= 0. {
                return;
            }
            let first = px(total * ratio);
            if (state.sizes()[0] - first).abs() > px(0.5) {
                state.resize_panel(0, first, window, cx);
            }
        });
    }

    /// 画树：放大时只画放大的那张，否则一层层嵌套 resizable 组件。`card` 画一张卡片。
    pub fn render(&self, tree: &SplitTree, card: &dyn Fn(CardId) -> AnyElement) -> AnyElement {
        match tree.zoomed() {
            Some(zoomed) => card(zoomed),
            None => self.render_node(tree.root(), card),
        }
    }

    fn render_node(&self, node: &Node, card: &dyn Fn(CardId) -> AnyElement) -> AnyElement {
        let (id, axis, first, second) = match node {
            Node::Card(id) => return card(*id),
            Node::Split {
                id,
                axis,
                first,
                second,
                ..
            } => (id, axis, first, second),
        };
        let Some(mirror) = self.mirrors.get(id) else {
            // 还没同步镜像（不该发生）：照样画出两边，只是不能拖。
            return div()
                .flex()
                .size_full()
                .child(self.render_node(first, card))
                .child(self.render_node(second, card))
                .into_any_element();
        };
        let node = *id;
        let axis = *axis;
        let extent = mirror.extent.get();
        // 两边都限制在 0.08–0.92；还没量过尺寸时不限（默认的 100px 下限也不要）。
        let range = if extent > px(0.) {
            extent * MIN_RATIO..extent * MAX_RATIO
        } else {
            px(0.)..Pixels::MAX
        };
        let group = match axis {
            Axis::Horizontal => h_resizable(("split", node.0 as usize)),
            Axis::Vertical => v_resizable(("split", node.0 as usize)),
        };
        let group = group
            .with_state(&mirror.state)
            .with_handle_appearance(divider_appearance(node))
            .on_resize(move |state, window, cx| {
                let sizes = state.read(cx).sizes();
                let total: f32 = sizes.iter().map(|size| f32::from(*size)).sum();
                if sizes.len() == 2 && total > 0. {
                    let ratio = f32::from(sizes[0]) / total;
                    window.dispatch_action(Box::new(ResizeDivider { node, ratio }), cx);
                }
            })
            .child(
                // 两边一开始都从 0 起按 flex 平分；量过以后由 resizable 组件按像素摆。
                resizable_panel()
                    .size(px(0.))
                    .size_range(range.clone())
                    .child(
                        div()
                            .flex()
                            .size_full()
                            .min_w_0()
                            .min_h_0()
                            .child(self.render_node(first, card)),
                    ),
            )
            .child(
                resizable_panel().size(px(0.)).size_range(range).child(
                    div()
                        .flex()
                        .size_full()
                        .min_w_0()
                        .min_h_0()
                        .child(self.render_node(second, card)),
                ),
            );
        let extent = mirror.extent.clone();
        div()
            .relative()
            .flex()
            .flex_1()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(group)
            // 在 prepaint 阶段记下容器的长度，下一帧按它换算界限。
            .child(
                canvas(
                    move |bounds, _, _| {
                        extent.set(match axis {
                            Axis::Horizontal => bounds.size.width,
                            Axis::Vertical => bounds.size.height,
                        })
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    }
}

fn collect_dividers(node: &Node, nodes: &mut Vec<NodeId>) {
    if let Node::Split {
        id, first, second, ..
    } = node
    {
        nodes.push(*id);
        collect_dividers(first, nodes);
        collect_dividers(second, nodes);
    }
}

/// 拖动柄里画的东西：盖满整个柄的透明块 `("divider", node)`，正中一条 1px 的 `--line`；
/// 双击它恢复成一半。拖动本身、光标和命中区都还是库的。
fn divider_appearance(node: NodeId) -> ResizeHandleRenderer {
    Rc::new(move |context: &ResizeHandleContext, _, cx| {
        let line = Tokens::current(cx).line;
        let horizontal = context.axis() == gpui_kit::Axis::Horizontal;
        Some(
            div()
                .id(("divider", node.0 as usize))
                .test_support()
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .when(!horizontal, |handle| handle.flex_col())
                .child(div().flex_none().bg(line).map(|rule| {
                    if horizontal {
                        rule.w(size::LINE).h_full()
                    } else {
                        rule.h(size::LINE).w_full()
                    }
                }))
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    if event.click_count == 2 {
                        window.dispatch_action(Box::new(ResetDivider(node)), cx);
                    }
                })
                .into_any_element(),
        )
    })
}
