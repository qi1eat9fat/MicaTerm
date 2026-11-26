//! 应用内启动的 Agent 跑哪个程序（Agent CLI 工单 07）：设置 › Agent 的「Agent 命令」，
//! 留空时先找 `claude`，再找 `codex`，最后是 Antigravity 的 CLI（`agy`）。每个先在 `PATH` 里找；
//! 找不到时再看不在 `PATH` 里的常见位置：官方安装脚本的 `~/.local/bin`、Claude 桌面版自带的那份，
//! 和 Antigravity CLI 安装脚本放的位置。

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui_kit::SharedString;

use micaterm_terminal::LocalShell;

use crate::cli::AgentKind;

/// 自动检测时按顺序找的程序。
const DETECTED: [(&str, AgentKind); 3] = [
    ("claude", AgentKind::ClaudeCode),
    ("codex", AgentKind::Codex),
    ("agy", AgentKind::Antigravity),
];

/// 要启动的 Agent。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentProgram {
    /// 交给本地终端的命令行：设置里写的，或检测到的程序名（Windows 上由终端按 `PATHEXT` 找
    /// `claude.cmd`、`claude.exe`）；不在 `PATH` 里时是加了引号的完整路径。
    pub command: String,
    /// 显示的名字：Claude Code、Codex，认不出的程序是它的文件名。
    pub name: SharedString,
    /// 认得出的 Agent（它的 Skill 装在哪里）；认不出的程序是 `None`。
    pub kind: Option<AgentKind>,
    /// 自动检测到的是 Claude 桌面版自带的 Claude Code。
    pub bundled: bool,
}

impl AgentProgram {
    /// 能不能接着以前的对话（启动框列不列历史对话）：只有 Claude Code 认得 `--resume`。
    pub fn can_resume(&self) -> bool {
        self.kind == Some(AgentKind::ClaudeCode)
    }

    /// 接着对话 `id` 要加的参数（放在命令行后面、交卡片的说明前面）；不能接着时 `None`。
    pub fn resume_args(&self, id: &str) -> Option<Vec<String>> {
        self.can_resume()
            .then(|| vec!["--resume".to_string(), id.to_string()])
    }
}

/// 不在 `PATH` 里、自动检测也认的一份安装。
#[derive(Clone, Debug, PartialEq, Eq)]
struct KnownInstall {
    kind: AgentKind,
    file: PathBuf,
    bundled: bool,
}

/// 设置里的命令（`None` 或空白是自动检测）对应的 Agent；自动检测没找到时 `None`。
/// 用户自己写的命令不检查找不找得到：找不到时终端里会说。阻塞：会看文件系统。
pub fn agent_program(setting: Option<&str>) -> Option<AgentProgram> {
    agent_program_on(setting, None)
}

/// 同 [`agent_program`]，自动检测时在 `path` 里找（`None` 是这个进程的 `PATH`）。
/// 给了 `path`（测试）时只看它，不看本机的其他安装位置，结果不随跑测试的机器变。
pub fn agent_program_on(setting: Option<&str>, path: Option<&OsStr>) -> Option<AgentProgram> {
    let own = std::env::var_os("PATH").unwrap_or_default();
    let extensions = std::env::var_os("PATHEXT");
    let known = match path {
        Some(_) => Vec::new(),
        None => known_installs(),
    };
    agent_program_in(setting, path.unwrap_or(&own), extensions.as_deref(), &known)
}

fn agent_program_in(
    setting: Option<&str>,
    path: &OsStr,
    extensions: Option<&OsStr>,
    known: &[KnownInstall],
) -> Option<AgentProgram> {
    if let Some(command) = setting.map(str::trim).filter(|command| !command.is_empty()) {
        let program = LocalShell::new(command)
            .command()
            .and_then(first_word)
            .unwrap_or_default();
        let stem = Path::new(&program)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let kind = DETECTED
            .iter()
            .find(|(name, _)| *name == stem)
            .map(|(_, kind)| *kind);
        return Some(AgentProgram {
            command: command.to_string(),
            name: kind.map(AgentKind::label).unwrap_or_else(|| stem.into()),
            kind,
            bundled: false,
        });
    }
    DETECTED.iter().find_map(|(name, kind)| {
        if on_path(name, path, extensions) {
            return Some(AgentProgram {
                command: name.to_string(),
                name: kind.label(),
                kind: Some(*kind),
                bundled: false,
            });
        }
        known
            .iter()
            .find(|install| install.kind == *kind)
            .map(|install| AgentProgram {
                command: format!("\"{}\"", install.file.display()),
                name: kind.label(),
                kind: Some(*kind),
                bundled: install.bundled,
            })
    })
}

/// 命令行的第一个词（程序），去掉包着它的引号。
fn first_word(command: &str) -> Option<String> {
    let command = command.trim_start();
    let word = match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => command.split_whitespace().next()?,
    };
    (!word.is_empty()).then(|| word.to_string())
}

/// `PATH` 里有没有叫 `name` 的程序；Windows 上按 `PATHEXT` 的扩展名找。
fn on_path(name: &str, path: &OsStr, extensions: Option<&OsStr>) -> bool {
    let extensions: Vec<String> = if cfg!(windows) {
        extensions
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    std::env::split_paths(path).any(|dir| {
        dir.join(name).is_file()
            || extensions
                .iter()
                .any(|ext| dir.join(format!("{name}{}", ext.to_lowercase())).is_file())
    })
}

/// Claude Code 可执行文件的文件名。
const CLAUDE_EXE: &str = if cfg!(windows) {
    "claude.exe"
} else {
    "claude"
};

/// 本机不在 `PATH` 里的 Claude Code，按优先顺序：官方安装脚本装的 `~/.local/bin/claude`
/// （GUI 程序的 `PATH` 常常没有它），再是 Claude 桌面版自带的。阻塞：会看文件系统。
fn known_installs() -> Vec<KnownInstall> {
    let mut found = Vec::new();
    if let Some(home) = dirs::home_dir() {
        let file = home.join(".local").join("bin").join(CLAUDE_EXE);
        if file.is_file() {
            found.push(KnownInstall {
                kind: AgentKind::ClaudeCode,
                file,
                bundled: false,
            });
        }
        // Antigravity 的 CLI（`agy`）：安装脚本放的位置（Windows 在 %LOCALAPPDATA%\agy\bin）。
        if let Some(file) = crate::cli::antigravity_cli_path(&home) {
            found.push(KnownInstall {
                kind: AgentKind::Antigravity,
                file,
                bundled: false,
            });
        }
    }
    if let Some(file) = newest_bundled(desktop_data_dirs()) {
        found.push(KnownInstall {
            kind: AgentKind::ClaudeCode,
            file,
            bundled: true,
        });
    }
    found
}

/// Claude 桌面版可能放数据的目录（里面有 `claude-code/<版本>/…/claude(.exe)`）：
/// - `%APPDATA%\Claude`（Windows 安装包版）、`~/Library/Application Support/Claude`（macOS）；
/// - `%LOCALAPPDATA%\Claude-3p`（新版桌面版的 Code 标签页，自带的 Claude Code 在这里）；
/// - `%LOCALAPPDATA%\Claude-Data\claude_<包 ID>`（Windows 应用商店 / MSIX 版）；
/// - `%LOCALAPPDATA%\Packages\Claude_<包 ID>\LocalCache\Roaming\Claude`（MSIX 把
///   `%APPDATA%` 重定向到这里的情况）。
fn desktop_data_dirs() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(config) = dirs::config_dir() {
        found.extend(children_starting_with(&config, "claude"));
    }
    if cfg!(windows)
        && let Some(local) = dirs::data_local_dir()
    {
        found.extend(local_data_dirs(&local));
    }
    found
}

/// `%LOCALAPPDATA%` 下桌面版可能用来放数据的目录。桌面版的目录名会随版本变
/// （`Claude-3p`、`Claude`、`Claude-Data` 之类），所以按 `Claude` 前缀找，
/// 而不是写死某个名字。
fn local_data_dirs(local: &Path) -> Vec<PathBuf> {
    let mut found = children_starting_with(local, "claude");
    found.extend(children_starting_with(
        &local.join("Claude-Data"),
        "claude_",
    ));
    found.extend(
        children_starting_with(&local.join("Packages"), "claude_")
            .into_iter()
            .map(|package| package.join("LocalCache").join("Roaming").join("Claude")),
    );
    found
}

/// `dir` 下名字以 `prefix` 开头的子目录（不分大小写）；`dir` 不在时是空的。
fn children_starting_with(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let prefix = prefix.to_lowercase();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .starts_with(&prefix)
        })
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

/// 这些桌面版数据目录里自带的 Claude Code：`claude-code/<版本>/claude(.exe)` 或
/// `claude-code/<版本>/<哈希>/claude(.exe)`。有好几份（升级后旧的还在）时取最后改的那份。
fn newest_bundled(data_dirs: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut consider = |file: PathBuf| {
        if let Ok(meta) = std::fs::metadata(&file)
            && meta.is_file()
        {
            found.push((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), file));
        }
    };
    for data in data_dirs {
        let Ok(versions) = std::fs::read_dir(data.join("claude-code")) else {
            continue;
        };
        for version in versions.flatten().map(|entry| entry.path()) {
            consider(version.join(CLAUDE_EXE));
            let Ok(inner) = std::fs::read_dir(&version) else {
                continue;
            };
            for dir in inner.flatten().map(|entry| entry.path()) {
                consider(dir.join(CLAUDE_EXE));
            }
        }
    }
    found
        .into_iter()
        .max_by_key(|(time, _)| *time)
        .map(|(_, file)| file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_in_the_settings_is_taken_as_written() {
        let empty = OsStr::new("");
        let claude = agent_program_in(Some(" claude --model opus "), empty, None, &[]).unwrap();
        assert_eq!(claude.command, "claude --model opus");
        assert_eq!(claude.name.as_ref(), "Claude Code");
        assert_eq!(claude.kind, Some(AgentKind::ClaudeCode));

        let quoted = agent_program_in(
            Some(r#""C:\Tools\codex.cmd" --full-auto"#),
            empty,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(quoted.kind, Some(AgentKind::Codex));

        let other = agent_program_in(Some("aider --yes"), empty, None, &[]).unwrap();
        assert_eq!(other.name.as_ref(), "aider");
        assert_eq!(other.kind, None);
    }

    #[test]
    fn left_empty_it_looks_for_claude_then_codex_on_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().as_os_str();
        assert_eq!(agent_program_in(None, path, None, &[]), None);
        assert_eq!(agent_program_in(Some("  "), path, None, &[]), None);

        let file = |name: &str| std::fs::write(dir.path().join(name), "").unwrap();
        file(if cfg!(windows) { "codex.cmd" } else { "codex" });
        let found = agent_program_in(None, path, None, &[]).unwrap();
        assert_eq!(
            (found.command.as_str(), found.kind),
            ("codex", Some(AgentKind::Codex))
        );

        file(if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        });
        let found = agent_program_in(None, path, None, &[]).unwrap();
        assert_eq!(found.command, "claude");
        assert_eq!(found.name.as_ref(), "Claude Code");
    }

    /// Antigravity 的 CLI（`agy`）排在 claude、codex 后面；不在 `PATH` 里时用安装脚本放的那份。
    #[test]
    fn the_antigravity_cli_is_detected_last() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().as_os_str();
        let file = |name: &str| std::fs::write(dir.path().join(name), "").unwrap();
        file(if cfg!(windows) { "agy.cmd" } else { "agy" });
        let found = agent_program_in(None, path, None, &[]).unwrap();
        assert_eq!(
            (found.command.as_str(), found.kind),
            ("agy", Some(AgentKind::Antigravity))
        );
        assert_eq!(found.name.as_ref(), "Antigravity");

        // `PATH` 里的 claude 排在它前面。
        file(if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        });
        let found = agent_program_in(None, path, None, &[]).unwrap();
        assert_eq!(found.kind, Some(AgentKind::ClaudeCode));

        // 不在 `PATH` 里：用安装脚本放的那份。
        let empty = OsStr::new("");
        let install = PathBuf::from(r"C:\Users\me\AppData\Local\agy\bin\agy.exe");
        let known = [KnownInstall {
            kind: AgentKind::Antigravity,
            file: install.clone(),
            bundled: false,
        }];
        let found = agent_program_in(None, empty, None, &known).unwrap();
        assert_eq!(found.command, format!("\"{}\"", install.display()));
        assert_eq!(found.kind, Some(AgentKind::Antigravity));

        // 设置里写了命令时也认得出它。
        let written = agent_program_in(Some("agy --help"), empty, None, &[]).unwrap();
        assert_eq!(written.kind, Some(AgentKind::Antigravity));
        assert_eq!(written.name.as_ref(), "Antigravity");
    }

    #[test]
    fn a_claude_off_the_path_comes_before_codex_on_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().as_os_str();
        std::fs::write(
            dir.path()
                .join(if cfg!(windows) { "codex.cmd" } else { "codex" }),
            "",
        )
        .unwrap();
        let bundled = PathBuf::from(r"C:\Claude\claude-code\2.1.325\abc\claude.exe");
        let known = [KnownInstall {
            kind: AgentKind::ClaudeCode,
            file: bundled.clone(),
            bundled: true,
        }];
        let found = agent_program_in(None, path, None, &known).unwrap();
        assert_eq!(found.command, format!("\"{}\"", bundled.display()));
        assert_eq!(found.kind, Some(AgentKind::ClaudeCode));
        assert!(found.bundled);

        // 设置里写了命令时不管检测到什么。
        let written = agent_program_in(Some("codex"), path, None, &known).unwrap();
        assert_eq!(written.kind, Some(AgentKind::Codex));
        assert!(!written.bundled);
    }

    #[test]
    fn only_claude_code_resumes_a_conversation() {
        let empty = OsStr::new("");
        let claude = agent_program_in(Some("claude"), empty, None, &[]).unwrap();
        assert!(claude.can_resume());
        assert_eq!(
            claude.resume_args("abc").unwrap(),
            ["--resume".to_string(), "abc".to_string()]
        );
        let codex = agent_program_in(Some("codex"), empty, None, &[]).unwrap();
        assert!(!codex.can_resume());
        assert_eq!(codex.resume_args("abc"), None);
    }

    #[test]
    fn the_newest_bundled_claude_code_is_found_with_or_without_a_hash_folder() {
        let data = tempfile::tempdir().unwrap();
        let root = data.path().join("claude-code");
        assert_eq!(newest_bundled([data.path().to_path_buf()]), None);

        let old = root.join("2.1.300").join(CLAUDE_EXE);
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "").unwrap();
        assert_eq!(
            newest_bundled([data.path().to_path_buf()]),
            Some(old.clone())
        );

        let new = root.join("2.1.325").join("806e0a45e6f5").join(CLAUDE_EXE);
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::write(&new, "").unwrap();
        let later = SystemTime::now() + std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&new)
            .unwrap()
            .set_modified(later)
            .unwrap();
        // 没有 claude 的目录（`.payload` 之类）不算。
        std::fs::create_dir_all(root.join("2.1.400").join("empty")).unwrap();
        assert_eq!(newest_bundled([data.path().to_path_buf()]), Some(new));
    }

    #[test]
    fn a_bundled_claude_code_under_localappdata_is_found() {
        let local = tempfile::tempdir().unwrap();
        // 新版桌面版（Code 标签页）把自带的那份放在 %LOCALAPPDATA%\Claude-3p。
        let file = local
            .path()
            .join("Claude-3p")
            .join("claude-code")
            .join("2.1.325")
            .join("806e0a45e6f5")
            .join(CLAUDE_EXE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        // 同一个 `%LOCALAPPDATA%` 下别的 Claude 目录没有 claude-code，跳过就好。
        std::fs::create_dir_all(local.path().join("Claude").join("logs")).unwrap();
        std::fs::create_dir_all(local.path().join("claude-cli-nodejs").join("Cache")).unwrap();

        assert!(
            local_data_dirs(local.path()).contains(&local.path().join("Claude-3p")),
            "{:?}",
            local_data_dirs(local.path())
        );
        assert_eq!(newest_bundled(local_data_dirs(local.path())), Some(file));

        // 应用商店版的目录也还在找。
        std::fs::create_dir_all(local.path().join("Claude-Data").join("claude_store")).unwrap();
        assert!(
            local_data_dirs(local.path())
                .contains(&local.path().join("Claude-Data").join("claude_store"))
        );
    }
}
