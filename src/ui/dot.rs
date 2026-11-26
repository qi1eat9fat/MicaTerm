//! 状态圆点：卡片标题行、传输队列、SFTP 连接、编辑器未保存、Agent 面板共用。
//!
//! 只画一个实心圆；要不要外圈光晕、元素 id 和无障碍名称由调用方再挂。

use gpui_kit::*;

/// 一个实心圆点。
pub fn dot(color: Hsla, size: Pixels) -> Div {
    div().flex_none().size(size).rounded_full().bg(color)
}
