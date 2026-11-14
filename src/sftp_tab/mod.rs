//! SFTP 标签（工单 20）：一台主机的 WinSCP 式双栏文件标签，有自己的一条连接，不能分屏。
//!
//! - `tab`：`SftpTab`，持有两张文件卡片、连接的命令通道；`execute` 是全部命令的入口；
//! - `file_card`：一侧的状态和界面；`path_bar`：路径栏；`file_listing`：表格的行和列，渲染回调只用快照；
//! - `model`、`selection`、`transfer_queue`：纯逻辑，没有 gpui 实体；`marquee`：框选；`file_icons`：文件图标；
//! - `file_menu`：右键菜单，还没实现的项用开关藏着；
//! - `operations`：做确认过的文件操作；`file_dialogs`、`properties_dialog`：问用户的框（工单 21）。
//!
//! 后面的工单只填自己的文件，不用改 `tab.rs` 的大 match：
//! - 工单 21（文件操作）：`operations.rs`（`OperationState`），
//! - 工单 22（传输）：`transfers.rs`（`TransferState`），
//! - 工单 23（编辑器）、24（预览）：`file_open.rs`（`FileOpenState`）；
//!   预览的框在 `preview`、`image_preview`。

mod file_card;
mod file_dialogs;
mod file_icons;
mod file_listing;
mod file_menu;
mod file_open;
mod image_preview;
mod marquee;
mod model;
mod operations;
mod path_bar;
mod preview;
mod properties_dialog;
mod queue_card;
mod selection;
mod tab;
mod transfer_dialog;
mod transfer_queue;
mod transfers;

pub use file_card::{CardSide, ConnectionState, FileCard, LoadIntent};
pub use file_icons::{FileIcon, FileIconKind};
pub use file_listing::FileListing;
pub use model::*;
pub use operations::FileOperation;
pub use preview::{IMAGE_LIMIT, PreviewKind};
pub use selection::{ClickMode, CursorMotion, Selection};
pub use tab::{SftpTab, SftpTabEvent, SftpTabOptions};
pub use transfer_queue::{
    QueueEntry, QueueId, QueueState, Removal, TransferJob, TransferQueue, percent,
};
