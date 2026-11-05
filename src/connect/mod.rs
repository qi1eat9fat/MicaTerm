//! 连接时的界面：连接要问用户的事（主机密钥、密码、验证码）的对话框。
//! 远程卡片（窗口壳转来连接表的问题）和主机对话框的「测试连接」共用（工单 12）。
//!
//! 功能模块不依赖窗口壳：对话框的回答交给调用方给的回调。

mod prompt_dialog;

pub use prompt_dialog::{PromptDialog, open_prompt_dialog};
