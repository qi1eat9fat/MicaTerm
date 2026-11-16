//! 本地卡片的身份环境变量（Agent CLI 工单 07，设计参照 Pebrel 的 `agent_env.rs`，代码自己写）：
//! 卡片里的程序——尤其是 Claude Code、Codex 这样的 Agent——不翻进程、不读源码，就知道自己在
//! MicaTerm 的哪张卡片里、`micaterm` 命令在哪。
//!
//! | 变量 | 说什么 |
//! | --- | --- |
//! | `TERM_PROGRAM` / `TERM_PROGRAM_VERSION` | 在 MicaTerm 里，哪个版本 |
//! | `MICATERM_SELF_CARD` | 自己这张卡片的 CLI 卡片 ID |
//! | `MICATERM_CLI` | `micaterm` 命令的完整路径（便携版不一定在 `PATH` 上） |
//! | `PATH` | 能直接敲 `micaterm` 时，它所在的目录放到最前面 |
//!
//! 只给本地卡片：远程主机上没有这个程序，送过去的路径永远执行不了。

use std::path::{Path, PathBuf};

use super::IntegrationPaths;

/// `TERM_PROGRAM` 的值：程序判断「我在哪个终端里」的通用变量。
pub const TERM_PROGRAM: &str = "micaterm";
pub const TERM_PROGRAM_ENV: &str = "TERM_PROGRAM";
pub const TERM_PROGRAM_VERSION_ENV: &str = "TERM_PROGRAM_VERSION";
/// 自己这张卡片的 CLI 卡片 ID。
pub const SELF_CARD_ENV: &str = "MICATERM_SELF_CARD";
/// `micaterm` 命令的完整路径。
pub const CLI_ENV: &str = "MICATERM_CLI";
/// 只在 Agent 卡片里：交给它的远程卡片的 CLI 卡片 ID。
pub const CARD_ENV: &str = "MICATERM_CARD";
/// 只在 Agent 卡片里：那张卡片的主机 ID。
pub const HOST_ENV: &str = "MICATERM_HOST";

/// 本地卡片要带的环境变量，和要放到 `PATH` 最前面的目录。`card` 是卡片自己的 CLI 卡片 ID；
/// `paths` 是 `None`（测试、装不了命令的系统）时只有前三个。会看文件系统（命令装没装）。
pub fn identity(
    card: &str,
    paths: Option<&IntegrationPaths>,
) -> (Vec<(String, String)>, Option<PathBuf>) {
    let mut env = vec![
        (TERM_PROGRAM_ENV.to_string(), TERM_PROGRAM.to_string()),
        (
            TERM_PROGRAM_VERSION_ENV.to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        ),
        (SELF_CARD_ENV.to_string(), card.to_string()),
    ];
    let Some(paths) = paths else {
        return (env, None);
    };
    let callable = callable(paths);
    let cli = callable.unwrap_or(&paths.exe);
    env.push((CLI_ENV.to_string(), cli.display().to_string()));
    (env, callable.and_then(Path::parent).map(Path::to_path_buf))
}

/// 本地卡片里能不能直接敲 `micaterm`（[`identity`] 会把它所在的目录放到 `PATH` 最前面）。
pub fn cli_callable(paths: &IntegrationPaths) -> bool {
    callable(paths).is_some()
}

/// 能按 `micaterm` 这个名字直接敲的那个文件：装好的命令（Windows 上 `bin` 里那份、macOS 和
/// Linux 上的链接），没装时 macOS、Linux 上程序自己（它带参数启动就是命令行）。Windows 上没装时
/// 没有：程序旁边的叫 `micaterm-cli.exe`，`micaterm.exe` 是窗口程序。
fn callable(paths: &IntegrationPaths) -> Option<&Path> {
    if paths.bin_link.is_file() {
        return Some(&paths.bin_link);
    }
    let named = paths
        .exe
        .file_stem()
        .is_some_and(|stem| stem == super::install::COMMAND_NAME);
    (named && paths.exe.is_file()).then_some(paths.exe.as_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
        env.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn paths(root: &Path, bin_link: &str, exe: &str) -> IntegrationPaths {
        IntegrationPaths {
            home: root.join("home"),
            bin_link: root.join(bin_link),
            exe: root.join(exe),
            user_path: None,
        }
    }

    #[test]
    fn every_local_card_says_where_it_is_even_without_the_command() {
        let (env, front) = identity("Card0123456789ab", None);
        assert_eq!(value(&env, "TERM_PROGRAM"), Some("micaterm"));
        assert_eq!(
            value(&env, "TERM_PROGRAM_VERSION"),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(value(&env, "MICATERM_SELF_CARD"), Some("Card0123456789ab"));
        assert_eq!(value(&env, "MICATERM_CLI"), None);
        assert_eq!(front, None);
    }

    #[test]
    fn the_installed_command_goes_on_the_path_and_otherwise_only_its_path_is_given() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let command = if cfg!(windows) {
            "micaterm.exe"
        } else {
            "micaterm"
        };
        std::fs::create_dir_all(root.join("app/bin")).unwrap();
        std::fs::write(root.join("app/micaterm-cli.exe"), "cli").unwrap();

        // Windows 上没装：只给 `micaterm-cli.exe` 的路径，不动 PATH。
        let windows = paths(root, &format!("app/bin/{command}"), "app/micaterm-cli.exe");
        let (env, front) = identity("C", Some(&windows));
        assert_eq!(
            value(&env, "MICATERM_CLI"),
            Some(windows.exe.display().to_string().as_str())
        );
        assert_eq!(front, None);

        // 装了：给装好的那份，它的目录放到 PATH 最前面。
        std::fs::write(root.join("app/bin").join(command), "cli").unwrap();
        let (env, front) = identity("C", Some(&windows));
        assert_eq!(
            value(&env, "MICATERM_CLI"),
            Some(windows.bin_link.display().to_string().as_str())
        );
        assert_eq!(front.as_deref(), windows.bin_link.parent());

        // macOS、Linux 上没装链接：程序自己就叫 micaterm，能直接敲。
        std::fs::write(root.join("app").join(command), "app").unwrap();
        let unix = paths(root, "nowhere/micaterm", &format!("app/{command}"));
        let (env, front) = identity("C", Some(&unix));
        assert_eq!(
            value(&env, "MICATERM_CLI"),
            Some(unix.exe.display().to_string().as_str())
        );
        assert_eq!(front.as_deref(), unix.exe.parent());
    }
}
