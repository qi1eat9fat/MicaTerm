// 来自 since2006/shell-rs @ b2a307a: src/explorer/file_icons.rs
// 改造：只留单色一种（工单 12 去掉了「图标风格」和 Material Icon Theme 的彩色图标）。
//! SFTP 列表里名称前的图标：和界面其余图标同一套 Lucide 线条图标，按类型分。

use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{FileEntry, FileKind};
use crate::app::CatalogIcon;

/// What a row names, as far as its icon goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileIconKind {
    /// `..`.
    Parent,
    Folder,
    File,
    /// A symbolic link to a folder.
    FolderLink,
    /// A symbolic link to a file, or a broken one.
    FileLink,
}

/// 一个文件或目录的图标；链接用带箭头的那个。
#[derive(IntoElement)]
pub struct FileIcon {
    kind: FileIconKind,
    dimmed: bool,
    /// 边长和颜色；不给就是小号和次要字色。
    size: Option<Pixels>,
    color: Option<Hsla>,
}

impl FileIcon {
    pub fn new(kind: FileIconKind) -> Self {
        Self {
            kind,
            dimmed: false,
            size: None,
            color: None,
        }
    }

    /// `size` 见方、`color` 色（SFTP 列表，26 号 `.sf-r .nm svg`）。
    pub fn sized(mut self, size: Pixels, color: Hsla) -> Self {
        self.size = Some(size);
        self.color = Some(color);
        self
    }

    /// The icon of a row of the lists.
    pub fn entry(entry: &FileEntry) -> Self {
        let kind = if entry.is_parent() {
            FileIconKind::Parent
        } else {
            match (entry.kind, entry.target) {
                (FileKind::Dir, _) => FileIconKind::Folder,
                (FileKind::File, _) => FileIconKind::File,
                (FileKind::Symlink, Some(FileKind::Dir)) => FileIconKind::FolderLink,
                (FileKind::Symlink, _) => FileIconKind::FileLink,
            }
        };
        Self::new(kind)
    }

    /// Lighter, for a hidden file.
    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }
}

impl RenderOnce for FileIcon {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        match self.kind {
            FileIconKind::Parent => Icon::new(IconName::ArrowUp),
            FileIconKind::Folder => Icon::new(IconName::Folder),
            FileIconKind::File => Icon::new(IconName::File),
            FileIconKind::FolderLink => Icon::new(CatalogIcon::FolderSymlink),
            FileIconKind::FileLink => Icon::new(CatalogIcon::FileSymlink),
        }
        .map(|icon| match self.size {
            Some(size) => icon.size(size),
            None => icon.small(),
        })
        .flex_none()
        .text_color(self.color.unwrap_or(cx.theme().muted_foreground))
        .when(self.dimmed, |icon| icon.opacity(0.6))
    }
}
