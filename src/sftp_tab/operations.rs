// 参考 since2006/shell-rs @ b2a307a: src/explorer/pane_operations.rs 改写
// 裁剪：去掉「查找文件」的删除；状态集中在 `OperationState`。
//! 做确认过的文件操作：远程的发给工作线程，本地的放到后台。不管哪种，这一侧都忙到有结果，然后重读目录。
//! 问用户的框在 `file_dialogs.rs`、`properties_dialog.rs`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;

use super::{CardSide, ConnectionState, NewEntryKind, SftpTab, SftpTabId};
use crate::app::{SftpAction, SftpDispatch as _, SftpTabCommand};
use crate::backend::sftp::{
    LocalDirectoryProvider, PermissionEdit, RemoteOperation, RemotePath, SftpCommand,
    describe_sftp_error,
};
use crate::i18n::t;
use crate::ui::DialogLayer;

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

impl FileOperation {
    fn failure_title(&self) -> fn() -> SharedString {
        match self {
            FileOperation::Delete(_) => || t!("sftp_files.operation.delete_failed"),
            FileOperation::Rename { .. } => || t!("sftp_files.operation.rename_failed"),
            FileOperation::Create { .. } => || t!("sftp_files.operation.create_failed"),
            FileOperation::Permissions { .. } => || t!("sftp_files.operation.permissions_failed"),
        }
    }

    /// 重读目录以后选中哪一行。
    fn result_name(&self) -> Option<String> {
        match self {
            FileOperation::Rename { to, .. } => Some(to.clone()),
            FileOperation::Create { name, .. } => Some(name.clone()),
            _ => None,
        }
    }

    fn to_remote(&self, directory: &str) -> Result<RemoteOperation> {
        let directory = RemotePath::new(directory)?;
        let paths = |names: &[String]| -> Result<Vec<RemotePath>> {
            names.iter().map(|name| directory.join(name)).collect()
        };
        Ok(match self {
            FileOperation::Delete(names) => RemoteOperation::Delete {
                paths: paths(names)?,
            },
            FileOperation::Rename { from, to } => RemoteOperation::Rename {
                from: directory.join(from)?,
                to: directory.join(to)?,
            },
            FileOperation::Create {
                kind: NewEntryKind::Folder,
                name,
            } => RemoteOperation::CreateDirectory {
                path: directory.join(name)?,
            },
            FileOperation::Create {
                kind: NewEntryKind::File,
                name,
            } => RemoteOperation::CreateFile {
                path: directory.join(name)?,
            },
            FileOperation::Permissions {
                names,
                edit,
                recursive,
                add_x_to_dirs,
            } => RemoteOperation::SetPermissions {
                paths: paths(names)?,
                edit: *edit,
                recursive: *recursive,
                add_x_to_dirs: *add_x_to_dirs,
            },
        })
    }

    /// 会阻塞，在后台跑。
    fn run_local(&self, provider: &dyn LocalDirectoryProvider, directory: &Path) -> Result<()> {
        let paths = |names: &[String]| -> Vec<PathBuf> {
            names.iter().map(|name| directory.join(name)).collect()
        };
        match self {
            FileOperation::Delete(names) => provider.trash(&paths(names)),
            FileOperation::Rename { from, to } => {
                provider.rename(&directory.join(from), &directory.join(to))
            }
            FileOperation::Create {
                kind: NewEntryKind::Folder,
                name,
            } => provider.create_dir(&directory.join(name)),
            FileOperation::Create {
                kind: NewEntryKind::File,
                name,
            } => provider.create_file(&directory.join(name)),
            FileOperation::Permissions {
                names,
                edit,
                recursive,
                add_x_to_dirs,
            } => provider.set_permissions(&paths(names), *edit, *recursive, *add_x_to_dirs),
        }
    }
}

/// 一个操作有结果时要做的事。
struct PendingOperation {
    side: CardSide,
    /// 失败通知的标题。
    failure: fn() -> SharedString,
    select: Option<String>,
    /// 新建的文件：建好就用编辑器打开（WinSCP 也这样）。
    edit: Option<String>,
}

/// `SftpTab` 里归文件操作的状态：等结果的操作（按请求号）和下一个请求号。
#[derive(Default)]
pub struct OperationState {
    next_id: u64,
    pending: HashMap<u64, PendingOperation>,
    /// 开着的删除、名称或属性框：同时只开一个。
    pub(super) dialog: Option<DialogLayer>,
}

/// 对话框发操作用：对话框比连接活得久时，远程的操作作废；本地的和连接无关。
#[derive(Clone)]
pub(super) struct OperationSender {
    tab: SftpTabId,
    side: CardSide,
    generation: u64,
    dispatch: FocusHandle,
}

impl OperationSender {
    pub(super) fn send(&self, operation: FileOperation, window: &mut Window, cx: &mut App) {
        let side = self.side;
        let action = SftpAction::new(self.tab, SftpTabCommand::Operate { side, operation });
        let action = if side.is_remote() {
            action.with_generation(self.generation)
        } else {
            action
        };
        self.dispatch.dispatch_sftp_action(&action, window, cx);
    }
}

impl SftpTab {
    pub(super) fn operation_sender(&self, side: CardSide) -> OperationSender {
        OperationSender {
            tab: self.id(),
            side,
            generation: self.generation(),
            dispatch: self.dispatch.clone(),
        }
    }

    /// 这一侧现在能改文件：够得着，而且没有别的操作在跑。
    fn can_modify(&self, side: CardSide, cx: &App) -> bool {
        (!side.is_remote() || self.state == ConnectionState::Connected)
            && !self.card(side).read(cx).is_busy()
    }

    /// 能开始问用户：这一侧能改文件，也没有别的操作框开着。
    pub(super) fn can_start_operation(&self, side: CardSide, window: &Window, cx: &App) -> bool {
        self.can_modify(side, cx)
            && !self
                .operations
                .dialog
                .as_ref()
                .is_some_and(|layer| layer.is_open(window, cx))
    }

    /// 标签要关了：开着的操作框一起关。
    pub(super) fn close_operation_dialog(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(layer) = self.operations.dialog.take() {
            layer.close(window, cx);
        }
    }

    /// 做确认过的操作：远程的发给工作线程，本地的放到后台。
    pub(super) fn start_operation(
        &mut self,
        side: CardSide,
        operation: FileOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_modify(side, cx) {
            return;
        }
        let card = self.card(side).clone();
        let directory = card.read(cx).path();
        self.operations.next_id += 1;
        let id = self.operations.next_id;
        let edit = match &operation {
            FileOperation::Create {
                kind: NewEntryKind::File,
                name,
            } => Some(card.read(cx).child_path_of(name)),
            _ => None,
        };
        self.operations.pending.insert(
            id,
            PendingOperation {
                side,
                failure: operation.failure_title(),
                select: operation.result_name(),
                edit,
            },
        );
        card.update(cx, |card, cx| card.set_busy(true, cx));
        if side.is_remote() {
            match operation.to_remote(&directory) {
                Ok(operation) => self.send(SftpCommand::Operate {
                    request_id: id,
                    operation,
                }),
                Err(error) => {
                    self.finish_operation(id, Err(describe_sftp_error(&error)), window, cx)
                }
            }
        } else {
            let provider = self.local_files.clone();
            cx.spawn_in(window, async move |this, cx| {
                let result = cx
                    .background_spawn(async move {
                        operation
                            .run_local(provider.as_ref(), Path::new(&directory))
                            .map_err(|error| format!("{error:#}"))
                    })
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.finish_operation(id, result, window, cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// 操作有结果了（`SftpEvent::Operated`，或者本地的后台任务）。
    pub(super) fn finish_operation(
        &mut self,
        request_id: u64,
        result: Result<(), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.operations.pending.remove(&request_id) else {
            return;
        };
        let card = self.card(pending.side).clone();
        card.update(cx, |card, cx| {
            card.set_busy(false, cx);
            if let (Ok(()), Some(name)) = (&result, pending.select) {
                card.select_after_load(name);
            }
        });
        match result {
            Err(message) => window
                .push_notification(Notification::error(message).title((pending.failure)()), cx),
            Ok(()) => {
                if let Some(path) = pending.edit {
                    let command = SftpTabCommand::Edit {
                        side: pending.side,
                        path: Some(path),
                    };
                    self.dispatch.dispatch_sftp_action(
                        &SftpAction::new(self.id(), command),
                        window,
                        cx,
                    );
                }
            }
        }
        // 失败了也重读：删除可能已经删掉了一部分。
        self.reload(pending.side, window, cx);
    }

    /// 连接断了：等着工作线程回答的操作不会再有回答。
    pub(super) fn abandon_remote_operations(&mut self, cx: &mut Context<Self>) {
        self.operations
            .pending
            .retain(|_, pending| !pending.side.is_remote());
        self.remote()
            .clone()
            .update(cx, |card, cx| card.set_busy(false, cx));
    }
}
