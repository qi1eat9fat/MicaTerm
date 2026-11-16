//! 拖动柄共用的件：gpui 的 resizable 组件默认在柄上画一条线，我们的分隔线由两边自己画（细边、间隙），
//! 柄里什么都不画，命中区和光标还是库的。SFTP 两栏、传输队列、右侧停靠面板共用。

use std::rc::Rc;

use gpui_kit::base::{ResizeHandleContext, ResizeHandleRenderer};
use gpui_kit::*;

/// 什么都不画的拖动柄：`id` 给测试找它。
pub fn invisible_handle(id: &'static str) -> ResizeHandleRenderer {
    Rc::new(move |_: &ResizeHandleContext, _, _| {
        Some(
            div()
                .id(id)
                .test_support()
                .absolute()
                .inset_0()
                .into_any_element(),
        )
    })
}
