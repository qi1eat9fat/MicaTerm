//! 各功能模块共用的界面件（不依赖窗口壳，也不认任何功能模块的类型）：
//! - `dialog`：对话框的层，按身份打开、关闭（[`DialogLayer`]）；窗口里的对话框一律经它开关；
//! - `confirm`：确认框（删除主机、删除凭据、删除命令片段、重置保险库共用）；
//! - `form`：表单对话框的通用件（分组、一行字段、输入框、锁定的秘密框）。

pub mod confirm;
pub mod dialog;
pub mod form;

pub use dialog::DialogLayer;
