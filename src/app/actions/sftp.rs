//! SFTP 标签的 action（工单 20 一次定全，21、22、23 只填实现，见 `sftp_tab` 的模块说明）。
//!
//! 菜单、拖放、对话框、按键都走同一组命令 `SftpTabCommand`。两种派发：
//! - `SftpAction`：指名是哪个标签的（菜单、拖放、对话框），可以带连接的 `generation`：连接换过就作废；
//! - `SftpShortcut`：按键，发给在前的 SFTP 标签。
// 参考 since2006/shell-rs @ b2a307a: src/app/actions.rs 的 `ExplorerCommand`、`ExplorerAction`

use std::path::PathBuf;

use gpui_kit::Action;

use crate::backend::sftp::TransferAnswer;
use crate::sftp_tab::{
    CardSide, CursorMotion, FileOperation, NewEntryKind, SftpTabId, TransferJob,
};

/// SFTP 标签的命令。`side` 说是哪一侧的卡片。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SftpTabCommand {
    /// 去 `path`（路径栏的某一级、双击目录）。
    Navigate {
        side: CardSide,
        path: String,
    },
    /// 路径栏变成输入框（点空白处、Ctrl+O）。
    EditPath {
        side: CardSide,
    },
    /// 路径栏输入框里按了回车：去输入的目录，去不了就描红、留在输入框。
    SubmitPath {
        side: CardSide,
        text: String,
    },
    /// 回上级目录（Backspace、`..`）。
    Up {
        side: CardSide,
    },
    /// 重读当前目录（Ctrl+R）。
    Refresh {
        side: CardSide,
    },
    /// 回车：进光标所在的目录，文件就编辑。
    Open {
        side: CardSide,
    },
    /// 用编辑器打开 `path`，没写就是光标所在的文件（F4，工单 23）。
    Edit {
        side: CardSide,
        path: Option<String>,
    },
    /// 预览 `path`，没写就是光标所在的文件（工单 24）。
    Preview {
        side: CardSide,
        path: Option<String>,
    },
    /// 复制当前目录的路径（空白处的右键菜单）。
    CopyPath {
        side: CardSide,
    },
    /// 复制选中项的路径，一行一个（行上的右键菜单）。
    CopySelectedPaths {
        side: CardSide,
    },
    /// 焦点给这一侧的文件列表（Tab）。
    FocusCard {
        side: CardSide,
    },
    MoveCursor {
        side: CardSide,
        motion: CursorMotion,
        extend: bool,
    },
    ToggleSelection {
        side: CardSide,
    },
    SelectAll {
        side: CardSide,
    },
    /// 删除这一侧选中的，先确认（Delete，工单 21）。
    Delete {
        side: CardSide,
    },
    /// 重命名（F2，工单 21）。
    Rename {
        side: CardSide,
    },
    /// 新建文件夹或文件（工单 21）。
    New {
        side: CardSide,
        kind: NewEntryKind,
    },
    /// 属性（F9，工单 21）。
    Properties {
        side: CardSide,
    },
    /// 做确认过的操作（工单 21）。
    Operate {
        side: CardSide,
        operation: FileOperation,
    },
    /// F5：上传本地选中的，或者下载远程选中的（工单 22）。
    Transfer {
        side: CardSide,
    },
    /// 用原生选择框挑要上传的（工单 22）。
    ChooseFiles,
    /// 问要不要把这些本地路径上传到 `target`（拖放、选择框，工单 22）。
    UploadPaths {
        paths: Vec<PathBuf>,
        target: String,
    },
    /// 问要不要把这些远程路径下载到 `target`（拖放、预览的「下载…」，工单 22、24）。
    DownloadPaths {
        paths: Vec<String>,
        target: String,
    },
    /// 把确认过的一批放进传输队列（工单 22）。
    Enqueue(TransferJob),
    /// 回答传输的问题（工单 22）。
    Answer {
        request_id: u64,
        answer: TransferAnswer,
    },
    /// 停下正在跑的一批，进度留着（工单 22）。
    CancelTransfer,
    /// 接着跑停下的一批（工单 22）。
    ResumeTransfer,
    /// 丢弃停下的那批的续传进度（工单 22）。
    DiscardTransfer,
    SelectQueueEntry {
        id: u64,
    },
    ToggleQueueEntry {
        id: u64,
    },
    RemoveQueueEntry,
    ClearFinishedTransfers,
    /// 重新连接：一条新的 SFTP 连接。批次在跑或停着时不行。
    Reconnect,
    /// 双击光标所在的项目：和回车一样，只是图片预览（工单 24）。
    DoubleClick {
        side: CardSide,
    },
    /// 用原生的文件夹选择框挑要上传的文件夹（工单 35）。
    ChooseFolder,
}

/// 发给 `tab` 的命令。
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct SftpAction {
    tab: SftpTabId,
    command: SftpTabCommand,
    generation: Option<u64>,
}

impl SftpAction {
    pub fn new(tab: SftpTabId, command: SftpTabCommand) -> Self {
        Self {
            tab,
            command,
            generation: None,
        }
    }

    /// 只对这一代连接有效：对话框比连接活得久时，它发的命令作废。
    pub fn with_generation(mut self, generation: u64) -> Self {
        self.generation = Some(generation);
        self
    }

    pub fn tab(&self) -> SftpTabId {
        self.tab
    }

    pub fn command(&self) -> &SftpTabCommand {
        &self.command
    }

    pub fn generation(&self) -> Option<u64> {
        self.generation
    }
}

/// 文件列表的按键：发给在前的 SFTP 标签。
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct SftpShortcut(pub SftpTabCommand);

/// 显示隐藏文件：这一侧的开关，写进设置（Ctrl+Alt+H、空白处的右键菜单）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct ToggleHiddenFiles(pub CardSide);

/// 界面实体在自己的 update 结束以后才派发：窗口壳读它们的最新状态时不会重入。
pub trait SftpDispatch {
    fn dispatch_sftp_action(
        &self,
        action: &SftpAction,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::App,
    );
}

impl SftpDispatch for gpui_kit::FocusHandle {
    fn dispatch_sftp_action(
        &self,
        action: &SftpAction,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::App,
    ) {
        let focus = self.clone();
        let action = action.clone();
        window.defer(cx, move |window, cx| {
            focus.dispatch_action(&action, window, cx)
        });
    }
}
