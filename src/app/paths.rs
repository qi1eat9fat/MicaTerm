//! 数据目录：MicaTerm 存放全部配置的文件夹，拷走它就带走全部配置（ADR 0003）。
//!
//! 默认是可执行文件旁的 `data/`；macOS 上放在 `.app` 包的旁边（写进包内会破坏签名）。
//! 环境变量 `MICATERM_DATA_DIR` 可以把它改到别处。开发版的数据自然落在 `target/debug/data/`。
// 参考 since2006/shell-rs @ b2a307a: src/app/paths.rs 重写（规则按 ADR 0003）

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// 改数据目录的环境变量。
pub const DATA_DIR_ENV: &str = "MICATERM_DATA_DIR";

/// 可执行文件旁的数据目录名。
pub const DATA_DIR_NAME: &str = "data";

/// Windows 上装进 PATH 的 `micaterm` 命令所在的文件夹：程序目录下的 `bin`（Agent CLI 工单 05）。
pub const CLI_BIN_DIR: &str = "bin";

/// 程序文件名：`bin` 里的命令副本据此认出上一级就是 MicaTerm 的程序目录。
pub const PROGRAM_FILE: &str = if cfg!(windows) {
    "micaterm.exe"
} else {
    "micaterm"
};

/// 设置文件名。
pub const SETTINGS_FILE: &str = "settings.json";

/// 主机、凭据、命令片段的数据库文件名。
/// 发布后不能改：改了就找不到已有的主机数据。
pub const DATABASE_FILE: &str = "micaterm.db";

/// 本次运行的数据目录：`MICATERM_DATA_DIR`，否则是可执行文件旁的 `data/`。
pub fn data_dir() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    // 系统怎么拼就怎么用：8.3 短名、符号链接都换成真实路径。
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);
    Ok(data_dir_of(&exe, std::env::var_os(DATA_DIR_ENV)))
}

/// 程序在 `exe`、环境变量 `MICATERM_DATA_DIR` 的值是 `env` 时的数据目录。纯函数，便于测试。
///
/// - `env` 非空：就用它；
/// - macOS 的 `X.app/Contents/MacOS/micaterm`：`.app` 旁边的 `data/`；
/// - 装进 PATH 的命令副本 `<程序目录>/bin/micaterm.exe`（上一级有 MicaTerm 程序）：程序目录下的 `data/`，
///   这样命令和它拷自的那个 MicaTerm 用同一个数据目录、连同一根管道；
/// - 其他情况：`exe` 所在目录下的 `data/`。
pub fn data_dir_of(exe: &Path, env: Option<OsString>) -> PathBuf {
    if let Some(dir) = env.filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    let folder = exe.parent().unwrap_or(Path::new("."));
    let beside = app_bundle_of(folder)
        .and_then(Path::parent)
        .or_else(|| program_folder_above(folder))
        .unwrap_or(folder);
    beside.join(DATA_DIR_NAME)
}

/// `folder` 是程序目录下的 `bin`（上一级有 MicaTerm 程序）时返回程序目录。
fn program_folder_above(folder: &Path) -> Option<&Path> {
    let above = folder
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(CLI_BIN_DIR))
        .then(|| folder.parent())
        .flatten()?;
    above.join(PROGRAM_FILE).is_file().then_some(above)
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

/// 保险库文件名。
pub const VAULT_FILE: &str = "vault.json";

/// 保险库 `vault.json` 的路径。数据目录还不存在时先建好；建不了就返回错误，由调用方报告。
pub fn vault_path() -> io::Result<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(VAULT_FILE))
}

/// MicaTerm 自己的主机密钥记录文件名（不碰 `~/.ssh/known_hosts`）。
pub const KNOWN_HOSTS_FILE: &str = "known_hosts";

/// 主机密钥记录 `known_hosts` 的路径（工单 12）。数据目录还不存在时先建好；建不了就返回错误。
pub fn known_hosts_path() -> io::Result<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(KNOWN_HOSTS_FILE))
}

/// 主窗口上次的大小、位置和最大化状态（设置 › 应用 › 窗口，工单 26）。
/// 和设置分开放：窗口一动它就变。
pub const WINDOW_STATE_FILE: &str = "window.json";

/// 数据目录 `data_dir` 里的 `window.json`。数据目录由调用方给：启动时是 [`data_dir`]，
/// 窗口壳用注入的那个（测试给临时目录）。不建目录，写的时候再建。
pub fn window_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(WINDOW_STATE_FILE)
}

/// 上传的续传记录放在哪（工单 22）。
pub fn upload_resume_dir() -> io::Result<PathBuf> {
    Ok(data_dir()?.join("upload-resume"))
}

/// 下载的续传记录放在哪（工单 22）。
pub fn download_resume_dir() -> io::Result<PathBuf> {
    Ok(data_dir()?.join("download-resume"))
}

/// 数据目录在 `dir` 的 MicaTerm 在哪里等 `micaterm` 命令（外部 CLI）：macOS、Linux 上是
/// 这个目录里的套接字 `cli.sock`，Windows 上是命名管道。命令和应用都按自己的数据目录算，
/// 所以都跟着 `MICATERM_DATA_DIR` 走；便携版的两份拷贝各有各的。
// 参考 since2006/shell-rs @ b2a307a: src/app/paths.rs 的 `cli_endpoint`、`pipe_name`
pub fn cli_endpoint(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(pipe_name(dir))
    } else {
        dir.join("cli.sock")
    }
}

/// 命名管道的名字是整台机器共用的，所以拿数据目录的路径算一个散列放进去：每个数据目录一根管道。
/// 路径先转成绝对路径、小写（Windows 的路径不分大小写），8.3 短名由 [`data_dir`] 先换成真实路径。
fn pipe_name(dir: &Path) -> String {
    use sha2::{Digest as _, Sha256};

    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let digest = Sha256::digest(dir.to_string_lossy().to_lowercase().as_bytes());
    let hex: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(r"\\.\pipe\micaterm-cli-{hex}")
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::{data_dir_of, pipe_name};

    #[test]
    fn the_command_copied_into_bin_uses_the_program_s_data_folder() {
        let root = tempfile::tempdir().unwrap();
        let program = root.path().join(super::PROGRAM_FILE);
        let copy = root.path().join("bin").join(super::PROGRAM_FILE);
        // 上一级没有 MicaTerm：只是个叫 bin 的文件夹，数据就在它旁边。
        assert_eq!(
            data_dir_of(&copy, None),
            root.path().join("bin").join("data")
        );
        std::fs::write(&program, "").unwrap();
        assert_eq!(data_dir_of(&copy, None), root.path().join("data"));
        assert_eq!(data_dir_of(&program, None), root.path().join("data"));
    }

    #[test]
    fn each_data_folder_has_a_pipe_of_its_own() {
        let one = pipe_name(Path::new("D:/Tools/MicaTerm/data"));
        assert!(one.starts_with(r"\\.\pipe\micaterm-cli-"), "{one}");
        assert_eq!(one.len(), r"\\.\pipe\micaterm-cli-".len() + 16);
        // 大小写不同还是同一个文件夹。
        assert_eq!(one, pipe_name(Path::new("d:/tools/micaterm/DATA")));
        assert_ne!(one, pipe_name(Path::new("E:/MicaTerm/data")));
    }

    #[test]
    fn the_data_folder_is_beside_the_program() {
        assert_eq!(
            data_dir_of(Path::new("D:/Tools/MicaTerm/micaterm.exe"), None),
            PathBuf::from("D:/Tools/MicaTerm/data")
        );
        assert_eq!(
            data_dir_of(Path::new("/opt/micaterm/micaterm"), None),
            PathBuf::from("/opt/micaterm/data")
        );
    }

    #[test]
    fn on_macos_the_data_folder_is_beside_the_app_bundle() {
        assert_eq!(
            data_dir_of(
                Path::new("/Applications/Tools/MicaTerm.app/Contents/MacOS/micaterm"),
                None
            ),
            PathBuf::from("/Applications/Tools/data")
        );
        // 只是名字像的普通目录不算包。
        assert_eq!(
            data_dir_of(Path::new("/home/me/Contents/MacOS/micaterm"), None),
            PathBuf::from("/home/me/Contents/MacOS/data")
        );
    }

    #[test]
    fn the_environment_variable_moves_the_data_folder() {
        assert_eq!(
            data_dir_of(
                Path::new("D:/Tools/MicaTerm/micaterm.exe"),
                Some(OsString::from("E:/portable/micaterm-data"))
            ),
            PathBuf::from("E:/portable/micaterm-data")
        );
        // 设成空字符串等于没设。
        assert_eq!(
            data_dir_of(
                Path::new("D:/Tools/MicaTerm/micaterm.exe"),
                Some(OsString::new())
            ),
            PathBuf::from("D:/Tools/MicaTerm/data")
        );
    }
}
