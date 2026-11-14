//! 状态栏和标签上显示的目录：主目录下的写成 `~` 开头（原型「~\vibe\MicaTerm」）。

use std::path::{Path, PathBuf};

/// 本机的主目录：Windows 上是 `USERPROFILE`，别处是 `HOME`。
pub fn home_dir() -> Option<PathBuf> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// 显示用的目录：在 `home` 下的换成 `~` 开头，其余原样。
pub fn display_directory(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home
        && let Ok(rest) = path.strip_prefix(home)
    {
        let rest = rest.as_os_str();
        if rest.is_empty() {
            return "~".to_string();
        }
        return format!("~{}{}", std::path::MAIN_SEPARATOR, rest.to_string_lossy());
    }
    path.display().to_string()
}

/// shell 报告的目录（OSC 7 之类）变成本机路径。Windows 上 OSC 7 的路径写成 `/C:/Users/x`，
/// 去掉开头的 `/`，斜杠换成反斜杠。
pub fn reported_path(path: &str) -> PathBuf {
    if cfg!(windows) {
        let bytes = path.as_bytes();
        let drive = bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b':';
        let path = if drive { &path[1..] } else { path };
        PathBuf::from(path.replace('/', "\\"))
    } else {
        PathBuf::from(path)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{MAIN_SEPARATOR, Path, PathBuf};

    use super::{display_directory, reported_path};

    #[test]
    fn the_home_folder_and_what_is_under_it_start_with_a_tilde() {
        let home: PathBuf = ["/", "home", "qi"].iter().collect();
        let under = home.join("vibe").join("MicaTerm");
        assert_eq!(display_directory(&home, Some(&home)), "~");
        assert_eq!(
            display_directory(&under, Some(&home)),
            format!("~{MAIN_SEPARATOR}vibe{MAIN_SEPARATOR}MicaTerm")
        );
    }

    #[test]
    fn a_folder_elsewhere_is_shown_as_it_is() {
        let home = Path::new("/home/qi");
        assert_eq!(display_directory(Path::new("/etc"), Some(home)), "/etc");
        // 只是名字开头一样，不在主目录下。
        assert_eq!(
            display_directory(Path::new("/home/qing"), Some(home)),
            "/home/qing"
        );
        assert_eq!(display_directory(Path::new("/etc"), None), "/etc");
    }

    #[cfg(windows)]
    #[test]
    fn a_reported_windows_path_loses_its_leading_slash() {
        assert_eq!(
            reported_path("/C:/Users/Qi/vibe"),
            PathBuf::from(r"C:\Users\Qi\vibe")
        );
        assert_eq!(reported_path(r"D:\work"), PathBuf::from(r"D:\work"));
        assert_eq!(
            display_directory(
                &reported_path("/C:/Users/Qi/vibe"),
                Some(Path::new(r"C:\Users\Qi"))
            ),
            r"~\vibe"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn a_reported_unix_path_is_kept() {
        assert_eq!(reported_path("/srv/app"), PathBuf::from("/srv/app"));
    }
}
