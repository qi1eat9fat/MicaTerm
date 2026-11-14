// 来自 since2006/shell-rs @ b2a307a: src/explorer/model.rs
// 裁剪：去掉 `FileSizeFormat`（大小固定简短格式，原型）；`ExplorerId` 改名 `SftpTabId`；
// 修改时间不带秒（原型）；加上 `reported_directory`（工单 20）。
//! 文件列表一行的快照和显示格式。

use gpui_kit::{App, Global, SharedString};
use serde::{Deserialize, Serialize};

use super::CardSide;
use crate::backend::sftp::{DirectoryEntry, EntryKind, RemotePath};
use crate::i18n::t;

/// 一个 SFTP 标签的身份。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SftpTabId(pub u64);

/// 「新建」建什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewEntryKind {
    Folder,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Dir,
    File,
    Symlink,
}

impl FileKind {
    pub fn label(self) -> SharedString {
        match self {
            FileKind::Dir => t!("sftp_tab.kind.folder"),
            FileKind::File => t!("sftp_tab.kind.file"),
            FileKind::Symlink => t!("sftp_tab.kind.link"),
        }
    }
}

impl From<EntryKind> for FileKind {
    /// 设备、套接字、管道都当文件列出来，打不开它们。
    fn from(kind: EntryKind) -> Self {
        match kind {
            EntryKind::Directory => FileKind::Dir,
            EntryKind::Symlink => FileKind::Symlink,
            EntryKind::File | EntryKind::Other => FileKind::File,
        }
    }
}

/// 目录列表里的一行。值保持原样，表格负责格式化。
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileEntry {
    pub name: SharedString,
    pub kind: FileKind,
    /// 符号链接指向什么；别的行和断了的链接是 `None`。
    pub target: Option<FileKind>,
    pub size: u64,
    /// 从 Unix 纪元起的秒数。
    pub modified: Option<i64>,
    /// 原始的 mode，带类型位。
    pub permissions: Option<u32>,
    pub owner: Option<SharedString>,
    pub group: Option<SharedString>,
}

impl FileEntry {
    pub fn new(name: &str, kind: FileKind) -> Self {
        Self {
            name: name.to_string().into(),
            kind,
            target: None,
            size: 0,
            modified: None,
            permissions: None,
            owner: None,
            group: None,
        }
    }

    pub fn dir(name: &str) -> Self {
        Self::new(name, FileKind::Dir)
    }

    #[cfg(test)]
    pub fn file(name: &str, size: u64) -> Self {
        Self {
            size,
            ..Self::new(name, FileKind::File)
        }
    }

    /// 回上级目录的 `..` 这一行。
    pub fn parent() -> Self {
        Self::dir("..")
    }

    pub fn from_directory_entry(entry: &DirectoryEntry) -> Self {
        let metadata = entry.metadata();
        let kind = FileKind::from(metadata.kind());
        Self {
            name: entry.name().to_string().into(),
            kind,
            target: (kind == FileKind::Symlink)
                .then(|| entry.target_kind().map(FileKind::from))
                .flatten(),
            size: metadata.size(),
            modified: metadata.modified().map(i64::from),
            permissions: metadata.permissions(),
            owner: entry.owner().map(|owner| owner.to_string().into()),
            group: entry.group().map(|group| group.to_string().into()),
        }
    }

    /// 目录和指向目录的链接：双击进去，排在文件前面。
    pub fn is_dir(&self) -> bool {
        self.kind == FileKind::Dir || self.target == Some(FileKind::Dir)
    }

    pub fn is_link(&self) -> bool {
        self.kind == FileKind::Symlink
    }

    pub fn is_parent(&self) -> bool {
        self.name.as_ref() == ".."
    }

    /// 点开头的文件：隐藏文件不显示时列表里没有它。`..` 不算。
    pub fn is_hidden(&self) -> bool {
        !self.is_parent() && self.name.starts_with('.')
    }

    /// 「类型」列：种类，文件是扩展名。
    pub fn type_label(&self) -> String {
        if self.is_parent() {
            return t!("sftp_tab.kind.parent").into();
        }
        match self.kind {
            FileKind::File => match self.name.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
                    t!("sftp_tab.kind.extension", ext = ext.to_uppercase()).into()
                }
                _ => self.kind.label().to_string(),
            },
            kind => kind.label().to_string(),
        }
    }
}

/// `directory` 里叫 `name` 的项目在哪，按它那一侧的路径规则。
pub fn child_path(directory: &str, name: &str, side: CardSide) -> String {
    if side.is_remote() {
        format!("{}/{name}", directory.trim_end_matches('/'))
    } else {
        std::path::Path::new(directory)
            .join(name)
            .to_string_lossy()
            .into_owned()
    }
}

/// 输入的目录按卡片解析：`~` 是主目录，相对路径从 `current` 算起，去掉末尾的分隔符。
pub fn expand_path(text: &str, current: &str, home: &str, side: CardSide) -> String {
    let joined = if text == "~" {
        home.to_string()
    } else if let Some(rest) = text.strip_prefix("~/") {
        format!("{}/{rest}", home.trim_end_matches('/'))
    } else if side.is_remote() {
        if text.starts_with('/') {
            text.to_string()
        } else {
            format!("{}/{text}", current.trim_end_matches('/'))
        }
    } else {
        std::path::Path::new(current)
            .join(text)
            .to_string_lossy()
            .into_owned()
    };
    if side.is_remote() {
        match joined.trim_end_matches('/') {
            "" => "/".into(),
            trimmed => trimmed.into(),
        }
    } else {
        std::path::Path::new(&joined)
            .components()
            .collect::<std::path::PathBuf>()
            .to_string_lossy()
            .into_owned()
    }
}

/// 卡片报告的目录，换成 SFTP 能用的路径：OSC 9;9 报的 Windows 路径 `C:\Users\qi`，
/// 在 OpenSSH 的 SFTP 里写成 `/C:/Users/qi`。别的原样。
pub fn reported_directory(directory: &str) -> String {
    let bytes = directory.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        format!("/{}", directory.replace('\\', "/"))
    } else {
        directory.to_string()
    }
}

/// 从根到 `path` 的每一级目录，`(名字, 路径)`。根的名字就是根自己（`/`、`C:\`）。
pub fn path_ancestors(path: &str, side: CardSide) -> Vec<(String, String)> {
    if path.is_empty() {
        return Vec::new();
    }
    let mut chain = Vec::new();
    if side.is_remote() {
        let Ok(mut current) = RemotePath::new(path) else {
            return chain;
        };
        loop {
            let text = current.as_str().trim_end_matches('/');
            let title = text.rsplit('/').next().filter(|name| !name.is_empty());
            chain.push((
                title.unwrap_or("/").to_string(),
                if text.is_empty() {
                    "/".into()
                } else {
                    text.into()
                },
            ));
            if current.is_root() || text.is_empty() {
                break;
            }
            current = current.parent();
        }
    } else {
        for ancestor in std::path::Path::new(path).ancestors() {
            let text = ancestor.to_string_lossy().into_owned();
            if text.is_empty() {
                continue;
            }
            let title = ancestor
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| text.clone());
            chain.push((title, text));
        }
    }
    chain.reverse();
    chain
}

/// 显示隐藏文件：列表显示不显示点开头的文件，本地和远程各一个开关（设置 › SFTP）。
/// 是全局量，由设置设：SFTP 标签不依赖设置。默认都关。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShowHiddenFiles {
    pub local: bool,
    pub remote: bool,
}

impl Global for ShowHiddenFiles {}

impl ShowHiddenFiles {
    /// `side` 现在显示不显示。
    pub fn get(side: CardSide, cx: &App) -> bool {
        cx.try_global::<Self>()
            .copied()
            .unwrap_or_default()
            .on(side)
    }

    pub fn on(self, side: CardSide) -> bool {
        match side {
            CardSide::Local => self.local,
            CardSide::Remote => self.remote,
        }
    }

    pub fn toggle(&mut self, side: CardSide) {
        match side {
            CardSide::Local => self.local = !self.local,
            CardSide::Remote => self.remote = !self.remote,
        }
    }
}

/// 简短的大小，用让数字小于 1024 的最小单位：`12 B`、`1.5 KB`、`348 KB`、`1.2 MB`。
/// 100 以下带一位小数，以上取整。
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 1;
    loop {
        // 按打印出来的样子算：四舍五入到 1024 就进一个单位，到 100 就去掉小数。
        let shown = if value < 99.95 {
            (value * 10.0).round() / 10.0
        } else {
            value.round()
        };
        if shown >= 1024.0 && unit < UNITS.len() - 1 {
            value /= 1024.0;
            unit += 1;
            continue;
        }
        return if shown < 100.0 {
            format!("{shown:.1} {}", UNITS[unit])
        } else {
            format!("{shown:.0} {}", UNITS[unit])
        };
    }
}

/// 「修改时间」列，本地时间，补零定宽（26 号）：`2026/04/22 09:05`。
pub fn format_changed(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y/%m/%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// 「权限」列：`rwxr-xr-x`，setuid、setgid、sticky 像 `ls -l` 那样写成 `s`/`S`、`t`/`T`。
pub fn format_rights(mode: u32) -> String {
    let special = [(0o4000, 's', 'S'), (0o2000, 's', 'S'), (0o1000, 't', 'T')];
    let mut rights = String::with_capacity(9);
    for (class, (special_bit, with_x, without_x)) in special.into_iter().enumerate() {
        let shift = 6 - class * 3;
        let bits = (mode >> shift) & 0o7;
        rights.push(if bits & 0o4 != 0 { 'r' } else { '-' });
        rights.push(if bits & 0o2 != 0 { 'w' } else { '-' });
        let execute = bits & 0o1 != 0;
        rights.push(match (mode & special_bit != 0, execute) {
            (true, true) => with_x,
            (true, false) => without_x,
            (false, true) => 'x',
            (false, false) => '-',
        });
    }
    rights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(12), "12 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(348 * 1024), "348 KB");
        assert_eq!(format_size(1_258_291), "1.2 MB");
        assert_eq!(format_size(4_105_175_040), "3.8 GB");
        assert_eq!(format_size(3 * 1024u64.pow(4)), "3.0 TB");
    }

    #[test]
    fn a_size_that_rounds_up_is_shown_as_it_rounds() {
        // 99.96 KB 不写成「100.0 KB」。
        assert_eq!(format_size(102_359), "100 KB");
        assert_eq!(format_size(102_297), "99.9 KB");
        // 1023.6 KB 不写成「1024 KB」。
        assert_eq!(format_size(1_048_166), "1.0 MB");
        assert_eq!(format_size(1_047_961), "1023 KB");
    }

    #[test]
    fn rights_follow_ls() {
        assert_eq!(format_rights(0o100_755), "rwxr-xr-x");
        assert_eq!(format_rights(0o600), "rw-------");
        assert_eq!(format_rights(0o41_777), "rwxrwxrwt");
        assert_eq!(format_rights(0o1_776), "rwxrwxrwT");
        assert_eq!(format_rights(0o4_755), "rwsr-xr-x");
        assert_eq!(format_rights(0o2_745), "rwxr-Sr-x");
    }

    #[test]
    fn changed_uses_a_padded_date_without_seconds() {
        let formatted = format_changed(1_700_000_000);
        let (date, time) = formatted.split_once(' ').unwrap();
        let date: Vec<_> = date.split('/').collect();
        assert_eq!(date[0].len(), 4, "{formatted}");
        assert!(date[1..].iter().all(|part| part.len() == 2));
        assert_eq!(date.len(), 3);
        assert_eq!(time.split(':').count(), 2);
    }

    #[test]
    fn type_label_uses_extension_for_files() {
        assert_eq!(FileEntry::file("a.log", 1).type_label(), "LOG 文件");
        assert_eq!(FileEntry::file(".zshrc", 1).type_label(), "文件");
        assert_eq!(FileEntry::dir("etc").type_label(), "文件夹");
        assert_eq!(FileEntry::parent().type_label(), "上级目录");
    }

    #[test]
    fn ancestors_run_from_the_root_down() {
        assert_eq!(
            path_ancestors("/home/tester/目录", CardSide::Remote),
            [
                ("/".to_string(), "/".to_string()),
                ("home".into(), "/home".into()),
                ("tester".into(), "/home/tester".into()),
                ("目录".into(), "/home/tester/目录".into()),
            ]
        );
        assert_eq!(
            path_ancestors("/", CardSide::Remote),
            [("/".to_string(), "/".to_string())]
        );
        assert!(path_ancestors("", CardSide::Remote).is_empty());
    }

    #[test]
    fn typed_paths_resolve_against_the_card() {
        let expand = |text| expand_path(text, "/srv/app", "/home/me", CardSide::Remote);
        assert_eq!(expand("~"), "/home/me");
        assert_eq!(expand("~/logs/"), "/home/me/logs");
        assert_eq!(expand("releases"), "/srv/app/releases");
        assert_eq!(expand("/var/log/"), "/var/log");
        assert_eq!(expand("/"), "/");
    }

    #[cfg(windows)]
    #[test]
    fn a_typed_drive_changes_drive() {
        assert_eq!(
            expand_path(r"D:\", r"C:\Users\Qi", r"C:\Users\Qi", CardSide::Local),
            r"D:\"
        );
        assert_eq!(
            expand_path("Downloads", r"C:\Users\Qi", r"C:\Users\Qi", CardSide::Local),
            r"C:\Users\Qi\Downloads"
        );
    }

    #[test]
    fn a_reported_windows_directory_is_written_the_way_sftp_reads_it() {
        assert_eq!(reported_directory(r"C:\Users\qi"), "/C:/Users/qi");
        assert_eq!(reported_directory("/etc/nginx"), "/etc/nginx");
        assert_eq!(reported_directory("~/logs"), "~/logs");
    }

    #[test]
    fn links_to_directories_count_as_directories() {
        let link = FileEntry {
            target: Some(FileKind::Dir),
            ..FileEntry::new("bin", FileKind::Symlink)
        };
        assert!(link.is_dir() && link.is_link());
        assert!(!FileEntry::new("broken", FileKind::Symlink).is_dir());
    }
}
