//! 卡片分屏（工单 05）：
//! - `tree`：分屏树的纯逻辑（`SplitTree`、`NodeId`、`Axis`、`Direction`），不依赖 gpui，有单元测试；
//! - `render`：照着树嵌套 `h_resizable` / `v_resizable` 画出来，比例同步进 gpui 的分屏面板。
//!
//! 只认 `CardId`，不认卡片实体：叶子画什么由调用方给。

mod tree;

pub use tree::*;
