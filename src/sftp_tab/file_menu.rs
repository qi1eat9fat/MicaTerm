// 参考 since2006/shell-rs @ b2a307a: src/explorer/pane_menu.rs 改写
// 裁剪：去掉书签、查找文件、「前往」子菜单和大小格式；空白处的菜单按全部界面原型重排。
//! 文件卡片的右键菜单（原型「文件右键菜单」「空白处右键菜单」）。
//!
//! 菜单项全部按原型的顺序写好；实现还没接上的项用下面的开关藏起来，工单 21、22、23、24 接上实现后
//! 只要把自己的开关改成 `true`（做法同 `host::menu`）。

use gpui_kit::Action;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::prelude::FluentBuilder as _;

use super::{CardSide, NewEntryKind, SftpTabId};
use crate::app::{SftpAction, SftpTabCommand, ToggleHiddenFiles};
use crate::i18n::t;

/// 「编辑」：工单 23 接上了 `SftpTabCommand::Edit`。
pub const SHOW_EDIT: bool = true;
/// 「预览」：工单 24 接上 `SftpTabCommand::Preview` 后改成 `true`。
pub const SHOW_PREVIEW: bool = false;
/// 「上传…」「下载…」：工单 22 接上 `SftpTabCommand::Transfer`、`ChooseFiles` 后改成 `true`。
pub const SHOW_TRANSFER: bool = false;
/// 「删除」「重命名…」「属性…」「新建文件夹…」「新建文件…」：工单 21 接上后改成 `true`。
pub const SHOW_FILE_OPERATIONS: bool = false;

/// 菜单要知道的卡片的事，开菜单时读。
#[derive(Clone, Debug)]
pub(super) struct CardMenuState {
    pub remote: bool,
    pub tab: SftpTabId,
    /// 选中的项目（文件命令的对象）。
    pub targets: Vec<String>,
    /// 只选中一个文件时它的完整路径：编辑器可以试着打开它。
    pub edits_file: Option<String>,
    pub can_go_up: bool,
    /// 改文件的命令现在能跑。
    pub can_modify: bool,
    /// 现在能开始传输。
    pub can_transfer: bool,
    /// 这一侧在显示隐藏文件。
    pub show_hidden: bool,
}

impl CardMenuState {
    fn action(&self, command: SftpTabCommand) -> Box<dyn Action> {
        Box::new(SftpAction::new(self.tab, command))
    }
}

/// 行上的右键：编辑 / 预览 / 下载… / 复制路径 / 删除 / 重命名… / 属性…。
pub(super) fn item_menu(menu: PopupMenu, state: &CardMenuState) -> PopupMenu {
    let remote = state.remote;
    let none = state.targets.is_empty() || !state.can_modify;
    let edit = state.edits_file.clone().filter(|_| SHOW_EDIT);
    let preview = state.edits_file.clone().filter(|_| SHOW_PREVIEW);
    let transfer = if remote {
        t!("sftp_tab.menu.download")
    } else {
        t!("sftp_tab.menu.upload")
    };
    menu.when_some(edit, |menu, path| {
        menu.menu(
            t!("sftp_tab.menu.edit"),
            state.action(SftpTabCommand::Edit {
                remote,
                path: Some(path),
            }),
        )
    })
    .when_some(preview, |menu, path| {
        menu.menu(
            t!("sftp_tab.menu.preview"),
            state.action(SftpTabCommand::Preview {
                remote,
                path: Some(path),
            }),
        )
    })
    .when(
        (SHOW_EDIT || SHOW_PREVIEW) && state.edits_file.is_some(),
        |menu| menu.separator(),
    )
    .when(SHOW_TRANSFER, |menu| {
        menu.menu_with_disabled(
            transfer,
            state.action(SftpTabCommand::Transfer { remote }),
            state.targets.is_empty() || !state.can_transfer,
        )
    })
    .menu_with_disabled(
        t!("sftp_tab.menu.copy_path"),
        state.action(SftpTabCommand::CopySelectedPaths { remote }),
        state.targets.is_empty(),
    )
    .when(SHOW_FILE_OPERATIONS, |menu| {
        menu.separator()
            .menu_with_disabled(
                t!("sftp_tab.menu.delete"),
                state.action(SftpTabCommand::Delete { remote }),
                none,
            )
            .menu_with_disabled(
                t!("sftp_tab.menu.rename"),
                state.action(SftpTabCommand::Rename { remote }),
                none || state.targets.len() != 1,
            )
            .menu_with_disabled(
                t!("sftp_tab.menu.properties"),
                state.action(SftpTabCommand::Properties { remote }),
                none,
            )
    })
}

/// 空白处的右键：新建文件夹… / 新建文件… / 上传… / 刷新 / 上级目录 / 复制路径 / 显示隐藏文件。
pub(super) fn directory_menu(menu: PopupMenu, state: &CardMenuState) -> PopupMenu {
    let remote = state.remote;
    // 远程一侧的「上传…」用原生选择框挑；本地一侧传的是选中的，没选中不能点。
    let (upload, upload_disabled) = if remote {
        (SftpTabCommand::ChooseFiles, !state.can_transfer)
    } else {
        (
            SftpTabCommand::Transfer { remote },
            state.targets.is_empty() || !state.can_transfer,
        )
    };
    menu.when(SHOW_FILE_OPERATIONS, |menu| {
        menu.menu_with_disabled(
            t!("sftp_tab.menu.new_folder"),
            state.action(SftpTabCommand::New {
                remote,
                kind: NewEntryKind::Folder,
            }),
            !state.can_modify,
        )
        .menu_with_disabled(
            t!("sftp_tab.menu.new_file"),
            state.action(SftpTabCommand::New {
                remote,
                kind: NewEntryKind::File,
            }),
            !state.can_modify,
        )
        .separator()
    })
    .when(SHOW_TRANSFER, |menu| {
        menu.menu_with_disabled(
            t!("sftp_tab.menu.upload"),
            state.action(upload),
            upload_disabled,
        )
        .separator()
    })
    .menu(
        t!("sftp_tab.menu.refresh"),
        state.action(SftpTabCommand::Refresh { remote }),
    )
    .menu_with_disabled(
        t!("sftp_tab.menu.up"),
        state.action(SftpTabCommand::Up { remote }),
        !state.can_go_up,
    )
    .menu(
        t!("sftp_tab.menu.copy_path"),
        state.action(SftpTabCommand::CopyPath { remote }),
    )
    .separator()
    .menu_with_check(
        t!("sftp_tab.menu.show_hidden"),
        state.show_hidden,
        Box::new(ToggleHiddenFiles(CardSide::from_remote(remote))),
    )
}
