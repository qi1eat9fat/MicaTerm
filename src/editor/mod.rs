//! 编辑器标签（工单 23 填内容）：编辑一个远程文件或本地文件，不能分屏。
//!
//! 工单 20 先把标签的骨架立好：标签列表、标签轨道、关闭流程和「文件」入口都已经认它。
//! 23 在这里加编辑器本身；`unsaved` 是关闭确认要问的。

use gpui_kit::*;

use crate::card::StatusDot;
use crate::programs::Program;
use crate::sftp_tab::SftpTab;

/// 编辑器标签打开的文件在哪里。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorLocation {
    Local(std::path::PathBuf),
    Remote(String),
}

/// 编辑器标签。远程文件经打开它的 SFTP 标签读写（`sftp`），本地文件不属于任何 SFTP 标签。
pub struct EditorTab {
    location: EditorLocation,
    /// 文件名：标签上的名称。
    name: SharedString,
    /// 标签上的灰字：主机名（本地文件写「本机」）。
    place: SharedString,
    sftp: Option<WeakEntity<SftpTab>>,
    unsaved: bool,
    focus_handle: FocusHandle,
}

impl EditorTab {
    /// 本地文件的编辑器标签。
    pub fn local(
        path: std::path::PathBuf,
        name: SharedString,
        place: SharedString,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            location: EditorLocation::Local(path),
            name,
            place,
            sftp: None,
            unsaved: false,
            focus_handle: cx.focus_handle(),
        }
    }

    /// 远程文件的编辑器标签，由 SFTP 标签 `sftp` 打开，经它的连接读写。
    pub fn remote(
        path: String,
        name: SharedString,
        place: SharedString,
        sftp: WeakEntity<SftpTab>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            location: EditorLocation::Remote(path),
            name,
            place,
            sftp: Some(sftp),
            unsaved: false,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn location(&self) -> &EditorLocation {
        &self.location
    }

    /// 打开它的 SFTP 标签：本地文件是 `None`。
    pub fn sftp(&self) -> Option<&WeakEntity<SftpTab>> {
        self.sftp.as_ref()
    }

    /// 有没有未保存的修改：关闭标签时要问。23 接上编辑器后由它维护。
    pub fn has_unsaved_changes(&self) -> bool {
        self.unsaved
    }

    /// 文件名。
    pub fn file_name(&self) -> SharedString {
        self.name.clone()
    }

    pub(crate) fn rail_name(&self) -> SharedString {
        self.name.clone()
    }

    pub(crate) fn rail_directory(&self) -> SharedString {
        self.place.clone()
    }

    pub(crate) fn rail_tile(&self) -> Program {
        crate::programs::editor_program(&self.name)
    }

    pub(crate) fn rail_dot(&self) -> StatusDot {
        if self.unsaved {
            StatusDot::Attention
        } else {
            StatusDot::None
        }
    }

    pub(crate) fn focus_target(&self) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("editor-tab")
            .test_support()
            .track_focus(&self.focus_handle)
            .flex_1()
            .size_full()
    }
}
