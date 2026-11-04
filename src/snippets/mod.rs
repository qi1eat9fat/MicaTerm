//! 命令片段：右侧栏里所有主机共用的命令，点一下发给焦点卡片（输入或直接执行）。
//! 片段本身在主机存储里（`host::Snippet`，和主机同一个数据库）；这里是面板和对话框。
//! 把命令发给哪张卡片、发不出去怎么办由窗口壳决定（`workspace/handlers/snippets.rs`）。

mod dialog;
mod panel;

pub use dialog::{SnippetForm, confirm_delete_snippet, open_snippet_dialog};
pub use panel::{SnippetPanel, SnippetPanelProps, build_snippet_menu};
