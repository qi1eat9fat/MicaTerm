//! 无头 UI 集成测试：在无头窗口里渲染生产环境的 `Workspace`，用真实的鼠标、键盘事件驱动它。
//!
//! 只有这一个测试程序，按功能分模块：`tests/` 下每多一个文件就是多一个测试程序，
//! 要再链接一遍整个 GPUI。共用的夹具在 `support/`。
//!
//! M1 的模块先全部声明好，各工单只往自己的文件里写测试：
//! - `window`：窗口壳（01）
//! - `terminal_card`：本地终端卡片（02）
//! - `terminal_input`：终端交互（03）
//! - `tabs`：多标签（04）
//! - `split`：卡片分屏（05）
//! - `programs`：程序图块与状态点（06）
//! - `settings`：设置对话框（07）
//! - `hosts`：主机栏（11）
//! - `host_dialog`：主机对话框（11）
//! - `snippets`：命令片段（19）
//! - `remote_card`：密码登录远程卡片（12）
//! - `quick_connect`：快速连接与最近连接（18）
//! - `split_remote`：分屏复用连接（16）
//! - `route`：跳板与代理（15）
//! - `vault`：保险库界面（13）
//! - `remote_activity`：远程卡片被动解析（17）
//! - `credentials`：凭据（14）
//! - `dialogs`：对话框的层（M2 code review）
//! - `sftp_tab`：SFTP 标签的入口和浏览（20）
//! - `sftp_files`：文件操作（21）
//! - `sftp_transfer`：传输（22）
//! - `editor`：编辑器标签（23）
//! - `preview`：预览（24）

mod support;

mod credentials;
mod dialogs;
mod editor;
mod host_dialog;
mod hosts;
mod preview;
mod programs;
mod quick_connect;
mod remote_activity;
mod remote_card;
mod route;
mod settings;
mod sftp_files;
mod sftp_tab;
mod sftp_transfer;
mod snippets;
mod split;
mod split_remote;
mod tabs;
mod terminal_card;
mod terminal_input;
mod vault;
mod window;
