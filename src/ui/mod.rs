//! 各功能模块共用的界面件（不依赖窗口壳，也不认任何功能模块的类型）：
//! - `dialog`：对话框的层，按身份打开、关闭（[`DialogLayer`]）；窗口里的对话框一律经它开关；
//! - `confirm`：确认框（删除主机、删除凭据、删除命令片段、重置保险库共用）；
//! - `form`：表单对话框的通用件（分组、一行字段、输入框、锁定的秘密框、标题、底部按钮）；
//! - `modal`：唯一的一种弹窗的样子（26 号「连接提示」），问答框、解锁、确认框都用它；
//! - `checkbox`：复选框的方框本体（三态、只读），整行勾选框和 SFTP 权限矩阵共用；
//! - `drag`：拖动排序共用的件（拖动状态的收尾、被拖的项变淡、插入线），主机栏和标签轨道共用；
//! - `icon_button`：26 号的图标按钮（`.ib`）；
//! - `dock`：右侧停靠面板的外框、标题行、分组标题（凭据、片段共用）；
//! - `empty`：列表/面板没有内容时的那一句话（主机栏、凭据、片段、选择器、文件列表共用）；
//! - `search`：紧凑搜索框（主机栏、卡片选择器共用）；
//! - `count`：数量徽章（停靠面板分组头、传输队列标题和批次行共用）；
//! - `dot`：状态圆点（卡片、传输队列、SFTP 连接、编辑器、Agent 面板共用）；
//! - `input`：单行输入框/下拉框的框（设置页和模态共用）；
//! - `resize`：拖动柄共用的件（什么都不画的柄）。

pub mod checkbox;
pub mod confirm;
pub mod count;
pub mod dialog;
pub mod dock;
pub mod dot;
pub mod drag;
pub mod empty;
pub mod form;
pub mod icon_button;
pub mod input;
pub mod modal;
pub mod resize;
pub mod search;
pub mod table;
pub mod toggle;

pub use dialog::DialogLayer;
pub use dialog::focus_after_dialogs;
pub use dot::dot;
