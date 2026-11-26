//! SFTP file and directory icons: Lucide line icons aligned with Muxy's `Files` and `Source Control`
//! panels (`site/card4.html`, `site/card5.html`), differentiated by entry kind and file category.

use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{FileEntry, FileKind};
use crate::app::CatalogIcon;

/// Semantic icon category for an entry in the SFTP file list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileIconKind {
    /// `..` parent directory entry.
    Parent,
    Folder,
    File,
    /// Source code, script, or structured configuration file.
    Code,
    /// Plain text, documentation, or log file.
    Text,
    /// Symbolic link pointing to a directory.
    FolderLink,
    /// Symbolic link pointing to a file, or broken link.
    FileLink,
}

impl FileIconKind {
    /// Whether this icon represents a folder or folder symlink.
    pub fn is_folder(self) -> bool {
        matches!(self, Self::Folder | Self::FolderLink)
    }
}

/// Classifies a filename into `Code`, `Text`, or generic `File`.
fn classify_file_name(name: &str) -> FileIconKind {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "dockerfile"
            | "makefile"
            | "justfile"
            | "cargo.lock"
            | "cmakelists.txt"
            | ".bashrc"
            | ".zshrc"
            | ".profile"
            | ".bash_profile"
    ) {
        return FileIconKind::Code;
    }
    if lower.starts_with("readme")
        || lower.starts_with("license")
        || lower.starts_with("notice")
        || lower.starts_with("changelog")
    {
        return FileIconKind::Text;
    }
    let Some((_, ext)) = lower.rsplit_once('.') else {
        return FileIconKind::File;
    };
    match ext {
        "rs" | "toml" | "json" | "jsonc" | "yaml" | "yml" | "js" | "mjs" | "cjs" | "ts" | "mts"
        | "cts" | "tsx" | "jsx" | "py" | "go" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat"
        | "cmd" | "c" | "h" | "cpp" | "hpp" | "cc" | "java" | "kt" | "swift" | "rb" | "php"
        | "lua" | "sql" | "html" | "htm" | "css" | "scss" | "vue" | "svelte" | "xml" | "svg"
        | "nix" | "zig" | "lock" => FileIconKind::Code,
        "md" | "markdown" | "txt" | "log" | "conf" | "cfg" | "ini" | "env" | "csv" | "tsv"
        | "gitignore" | "gitattributes" | "editorconfig" | "service" => FileIconKind::Text,
        _ => FileIconKind::File,
    }
}

/// Monochrome Lucide icon for a file or directory entry.
#[derive(IntoElement)]
pub struct FileIcon {
    kind: FileIconKind,
    dimmed: bool,
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

    /// Explicit square size and color.
    pub fn sized(mut self, size: Pixels, color: Hsla) -> Self {
        self.size = Some(size);
        self.color = Some(color);
        self
    }

    /// Resolves the appropriate icon for a `FileEntry`.
    pub fn entry(entry: &FileEntry) -> Self {
        let kind = if entry.is_parent() {
            FileIconKind::Parent
        } else {
            match (entry.kind, entry.target) {
                (FileKind::Dir, _) => FileIconKind::Folder,
                (FileKind::File, _) => classify_file_name(&entry.name),
                (FileKind::Symlink, Some(FileKind::Dir)) => FileIconKind::FolderLink,
                (FileKind::Symlink, _) => FileIconKind::FileLink,
            }
        };
        Self::new(kind)
    }

    pub fn kind(&self) -> FileIconKind {
        self.kind
    }

    /// Dimmed opacity for hidden dotfiles.
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
            FileIconKind::Code => Icon::new(CatalogIcon::FileCode),
            FileIconKind::Text => Icon::new(CatalogIcon::FileText),
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

#[cfg(test)]
mod tests {
    use super::{FileEntry, FileIcon, FileIconKind};

    #[test]
    fn classifies_code_text_and_folders() {
        assert_eq!(FileIcon::entry(&FileEntry::parent()).kind(), FileIconKind::Parent);
        assert_eq!(FileIcon::entry(&FileEntry::dir("crates")).kind(), FileIconKind::Folder);
        assert_eq!(
            FileIcon::entry(&FileEntry::file("deploy.sh", 10)).kind(),
            FileIconKind::Code
        );
        assert_eq!(
            FileIcon::entry(&FileEntry::file("Dockerfile", 10)).kind(),
            FileIconKind::Code
        );
        assert_eq!(
            FileIcon::entry(&FileEntry::file("README.md", 10)).kind(),
            FileIconKind::Text
        );
        assert_eq!(
            FileIcon::entry(&FileEntry::file("archive.tar.gz", 10)).kind(),
            FileIconKind::File
        );
    }
}
