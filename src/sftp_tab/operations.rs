//! 文件操作：删除、重命名、新建、权限（工单 21 填）。
//!
//! 工单 20 只留好入口：`SftpTab::execute` 和 `on_event` 的分支都转调这里的函数，
//! `OperationState` 是 `SftpTab` 里归这个工单的状态，`FileOperation` 已经在命令枚举里。
//! 上游参考 shell-rs 的 `src/explorer/pane_operations.rs`、`file_dialogs.rs`、`properties_dialog.rs`。

use gpui_kit::*;

use super::{NewEntryKind, SftpTab};
use crate::backend::sftp::PermissionEdit;

/// 用户确认过的文件操作，名字相对于这一侧的当前目录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOperation {
    Delete(Vec<String>),
    Rename {
        from: String,
        to: String,
    },
    Create {
        kind: NewEntryKind,
        name: String,
    },
    Permissions {
        names: Vec<String>,
        edit: PermissionEdit,
        recursive: bool,
        add_x_to_dirs: bool,
    },
}

/// `SftpTab` 里归文件操作的状态：等结果的操作（按请求号）和下一个请求号。
#[derive(Default)]
pub struct OperationState {}

impl SftpTab {
    /// 问要不要删除这一侧选中的（Delete）。
    pub(super) fn confirm_delete(
        &mut self,
        _remote: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 重命名（F2）。
    pub(super) fn open_rename(
        &mut self,
        _remote: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 新建文件夹或文件。
    pub(super) fn open_new(
        &mut self,
        _remote: bool,
        _kind: NewEntryKind,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 属性（F9）。
    pub(super) fn open_properties(
        &mut self,
        _remote: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 做确认过的操作：远程的发给工作线程，本地的放到后台。
    pub(super) fn start_operation(
        &mut self,
        _remote: bool,
        _operation: FileOperation,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 工作线程回答了操作（`SftpEvent::Operated`）。
    pub(super) fn finish_operation(
        &mut self,
        _request_id: u64,
        _result: Result<(), String>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 连接断了：等着工作线程回答的操作不会再有回答。
    pub(super) fn abandon_remote_operations(&mut self, _cx: &mut Context<Self>) {}
}
