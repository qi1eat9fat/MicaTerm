//! 数据目录：QiShell 存放全部配置的文件夹，拷走它就带走全部配置（ADR 0003）。
//!
//! 默认是可执行文件旁的 `data/`；macOS 上放在 `.app` 包的旁边（写进包内会破坏签名）。
//! 环境变量 `QISHELL_DATA_DIR` 可以把它改到别处。开发版的数据自然落在 `target/debug/data/`。
// 参考 since2006/shell-rs @ b2a307a: src/app/paths.rs 重写（规则按 ADR 0003）

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// 改数据目录的环境变量。
pub const DATA_DIR_ENV: &str = "QISHELL_DATA_DIR";

/// 可执行文件旁的数据目录名。
pub const DATA_DIR_NAME: &str = "data";

/// 设置文件名。
pub const SETTINGS_FILE: &str = "settings.json";

/// 主机、凭据、命令片段的数据库文件名。
pub const DATABASE_FILE: &str = "qishell.db";

/// 本次运行的数据目录：`QISHELL_DATA_DIR`，否则是可执行文件旁的 `data/`。
pub fn data_dir() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    // 系统怎么拼就怎么用：8.3 短名、符号链接都换成真实路径。
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);
    Ok(data_dir_of(&exe, std::env::var_os(DATA_DIR_ENV)))
}

/// 程序在 `exe`、环境变量 `QISHELL_DATA_DIR` 的值是 `env` 时的数据目录。纯函数，便于测试。
///
/// - `env` 非空：就用它；
/// - macOS 的 `X.app/Contents/MacOS/qishell`：`.app` 旁边的 `data/`；
/// - 其他情况：`exe` 所在目录下的 `data/`。
pub fn data_dir_of(exe: &Path, env: Option<OsString>) -> PathBuf {
    if let Some(dir) = env.filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    let folder = exe.parent().unwrap_or(Path::new("."));
    let beside = app_bundle_of(folder)
        .and_then(Path::parent)
        .unwrap_or(folder);
    beside.join(DATA_DIR_NAME)
}

/// `folder` 是 `X.app/Contents/MacOS` 时返回 `X.app`。
fn app_bundle_of(folder: &Path) -> Option<&Path> {
    let contents = folder
        .file_name()
        .is_some_and(|name| name == "MacOS")
        .then(|| folder.parent())
        .flatten()?;
    let bundle = contents
        .file_name()
        .is_some_and(|name| name == "Contents")
        .then(|| contents.parent())
        .flatten()?;
    bundle
        .extension()
        .is_some_and(|extension| extension == "app")
        .then_some(bundle)
}

/// `settings.json` 的路径。数据目录还不存在时先建好；建不了就返回错误，由调用方报告。
pub fn settings_path() -> io::Result<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(SETTINGS_FILE))
}

/// 主机数据库的路径。数据目录还不存在时先建好；建不了就返回错误，由调用方报告。
pub fn database_path() -> io::Result<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(DATABASE_FILE))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::data_dir_of;

    #[test]
    fn the_data_folder_is_beside_the_program() {
        assert_eq!(
            data_dir_of(Path::new("D:/Tools/QiShell/qishell.exe"), None),
            PathBuf::from("D:/Tools/QiShell/data")
        );
        assert_eq!(
            data_dir_of(Path::new("/opt/qishell/qishell"), None),
            PathBuf::from("/opt/qishell/data")
        );
    }

    #[test]
    fn on_macos_the_data_folder_is_beside_the_app_bundle() {
        assert_eq!(
            data_dir_of(
                Path::new("/Applications/Tools/QiShell.app/Contents/MacOS/qishell"),
                None
            ),
            PathBuf::from("/Applications/Tools/data")
        );
        // 只是名字像的普通目录不算包。
        assert_eq!(
            data_dir_of(Path::new("/home/me/Contents/MacOS/qishell"), None),
            PathBuf::from("/home/me/Contents/MacOS/data")
        );
    }

    #[test]
    fn the_environment_variable_moves_the_data_folder() {
        assert_eq!(
            data_dir_of(
                Path::new("D:/Tools/QiShell/qishell.exe"),
                Some(OsString::from("E:/portable/qishell-data"))
            ),
            PathBuf::from("E:/portable/qishell-data")
        );
        // 设成空字符串等于没设。
        assert_eq!(
            data_dir_of(
                Path::new("D:/Tools/QiShell/qishell.exe"),
                Some(OsString::new())
            ),
            PathBuf::from("D:/Tools/QiShell/data")
        );
    }
}
