//! 全部用户 action，按领域分文件，在这里汇总导出。
//!
//! 一个命令一个处理器：按钮、菜单、快捷键都派发这里的 action，由窗口壳统一处理
//! （处理器在 `workspace/handlers/` 下同名领域的文件里）。各工单只往自己领域的文件里加 action：
//! `window`（窗口壳、侧栏）、`hosts`（主机栏、主机对话框，11 建）、`tabs`（标签）、`cards`（卡片，02 建、05 补全）、`settings`（设置）。
//! 命名空间一律是 `qishell`；终端自己的 action 在终端 crate 里（命名空间 `qishell_terminal`）。

mod cards;
mod hosts;
mod settings;
mod tabs;
mod window;

pub use cards::*;
pub use hosts::*;
pub use settings::*;
pub use tabs::*;
pub use window::*;
