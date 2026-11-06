// 参考 since2006/shell-rs @ b2a307a: src/explorer/pane_menu.rs 改写
// 裁剪：去掉书签、查找文件、「前往」子菜单和大小格式；空白处的菜单按全部界面原型重排。
//! 文件卡片的右键菜单（原型「文件右键菜单」「空白处右键菜单」）。

use gpui_kit::Action;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::prelude::FluentBuilder as _;

use super::{CardSide, NewEntryKind, PreviewKind};
use crate::app::{SftpShortcut, SftpTabCommand, ToggleHiddenFiles};
use crate::i18n::t;

/// 菜单要知道的卡片的事，开菜单时读。
#[derive(Clone, Debug)]
pub(super) struct CardMenuState {
    pub side: CardSide,
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
    /// 这一侧能改权限（Windows 本地不能）。
    pub has_properties: bool,
}

impl CardMenuState {
    /// 菜单项派发和按键同一个 action，菜单才写得出按键（原型的「删除 Delete」）；
    /// 菜单只在前面的 SFTP 标签上开，所以发给在前的标签就对了。
    fn action(&self, command: SftpTabCommand) -> Box<dyn Action> {
        Box::new(SftpShortcut(command))
    }

    /// 有「编辑」吗：只选中一个文件时有，图片除外（当文本打开只会被拒绝）。
    fn shows_edit(&self) -> bool {
        let picture = self
            .edits_file
            .as_deref()
            .and_then(PreviewKind::of)
            .is_some_and(PreviewKind::preview_only);
        self.edits_file.is_some() && !picture
    }

    /// 「预览」对着的文件：只选中一个能预览的文件时有。Markdown 和 SVG 两项都有。
    fn previewed_file(&self) -> Option<String> {
        self.edits_file
            .clone()
            .filter(|path| PreviewKind::of(path).is_some())
    }
}

/// 行上的右键：编辑 / 预览 / 下载… / 复制路径 / 删除 / 重命名… / 属性…。
pub(super) fn item_menu(menu: PopupMenu, state: &CardMenuState) -> PopupMenu {
    let side = state.side;
    let none = state.targets.is_empty() || !state.can_modify;
    let edit = state.shows_edit();
    let preview = state.previewed_file();
    let transfer = if side.is_remote() {
        t!("sftp_tab.menu.download")
    } else {
        t!("sftp_tab.menu.upload")
    };
    // 和 F4 同一个 action，菜单才写得出按键：不带路径，对着只选中的那个文件。
    menu.when(edit, |menu| {
        menu.menu(
            t!("sftp_tab.menu.edit"),
            state.action(SftpTabCommand::Edit { side, path: None }),
        )
    })
    .when_some(preview, |menu, path| {
        menu.menu(
            t!("sftp_tab.menu.preview"),
            state.action(SftpTabCommand::Preview {
                side,
                path: Some(path),
            }),
        )
    })
    .when(state.edits_file.is_some(), |menu| menu.separator())
    .menu_with_disabled(
        transfer,
        state.action(SftpTabCommand::Transfer { side }),
        state.targets.is_empty() || !state.can_transfer,
    )
    .menu_with_disabled(
        t!("sftp_tab.menu.copy_path"),
        state.action(SftpTabCommand::CopySelectedPaths { side }),
        state.targets.is_empty(),
    )
    .separator()
    .menu_with_disabled(
        t!("sftp_tab.menu.delete"),
        state.action(SftpTabCommand::Delete { side }),
        none,
    )
    .menu_with_disabled(
        t!("sftp_tab.menu.rename"),
        state.action(SftpTabCommand::Rename { side }),
        none || state.targets.len() != 1,
    )
    .when(state.has_properties, |menu| {
        menu.menu_with_disabled(
            t!("sftp_tab.menu.properties"),
            state.action(SftpTabCommand::Properties { side }),
            none,
        )
    })
}

/// 空白处的右键：新建文件夹… / 新建文件… / 上传… / 刷新 / 上级目录 / 复制路径 / 显示隐藏文件。
pub(super) fn directory_menu(menu: PopupMenu, state: &CardMenuState) -> PopupMenu {
    let side = state.side;
    // 远程一侧的「上传…」用原生选择框挑；本地一侧传的是选中的，没选中不能点。
    let (upload, upload_disabled) = if side.is_remote() {
        (SftpTabCommand::ChooseFiles, !state.can_transfer)
    } else {
        (
            SftpTabCommand::Transfer { side },
            state.targets.is_empty() || !state.can_transfer,
        )
    };
    menu.menu_with_disabled(
        t!("sftp_tab.menu.new_folder"),
        state.action(SftpTabCommand::New {
            side,
            kind: NewEntryKind::Folder,
        }),
        !state.can_modify,
    )
    .menu_with_disabled(
        t!("sftp_tab.menu.new_file"),
        state.action(SftpTabCommand::New {
            side,
            kind: NewEntryKind::File,
        }),
        !state.can_modify,
    )
    .separator()
    .menu_with_disabled(
        t!("sftp_tab.menu.upload"),
        state.action(upload),
        upload_disabled,
    )
    .separator()
    .menu(
        t!("sftp_tab.menu.refresh"),
        state.action(SftpTabCommand::Refresh { side }),
    )
    .menu_with_disabled(
        t!("sftp_tab.menu.up"),
        state.action(SftpTabCommand::Up { side }),
        !state.can_go_up,
    )
    .menu(
        t!("sftp_tab.menu.copy_path"),
        state.action(SftpTabCommand::CopyPath { side }),
    )
    .separator()
    .menu_with_check(
        t!("sftp_tab.menu.show_hidden"),
        state.show_hidden,
        Box::new(ToggleHiddenFiles(side)),
    )
}

#[cfg(test)]
mod tests {
    use super::{CardMenuState, CardSide};

    fn state_on(file: Option<&str>) -> CardMenuState {
        CardMenuState {
            side: CardSide::Remote,
            targets: file.iter().map(|file| file.to_string()).collect(),
            edits_file: file.map(str::to_string),
            can_go_up: true,
            can_modify: true,
            can_transfer: true,
            show_hidden: false,
            has_properties: true,
        }
    }

    #[test]
    fn a_picture_is_only_previewed() {
        for picture in ["/p/a.png", "/p/a.JPG", "/p/a.gif", "/p/a.tiff"] {
            let state = state_on(Some(picture));
            assert!(!state.shows_edit(), "{picture}");
            assert_eq!(state.previewed_file().as_deref(), Some(picture));
        }
    }

    #[test]
    fn markdown_and_svg_are_edited_and_previewed() {
        for file in ["/p/README.md", "/p/logo.svg"] {
            let state = state_on(Some(file));
            assert!(state.shows_edit(), "{file}");
            assert_eq!(state.previewed_file().as_deref(), Some(file));
        }
    }

    #[test]
    fn other_files_are_only_edited_and_no_file_has_neither() {
        let text = state_on(Some("/p/notes.txt"));
        assert!(text.shows_edit());
        assert_eq!(text.previewed_file(), None);
        let none = state_on(None);
        assert!(!none.shows_edit());
        assert_eq!(none.previewed_file(), None);
    }
}
