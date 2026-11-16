//! 应用内启动的 Agent 跑哪个程序（Agent CLI 工单 07）：设置 › Agent 的「Agent 命令」，
//! 留空时在 `PATH` 里先找 `claude`，再找 `codex`。

use std::ffi::OsStr;
use std::path::Path;

use gpui_kit::SharedString;

use micaterm_terminal::LocalShell;

use crate::cli::AgentKind;

/// 自动检测时按顺序找的程序。
const DETECTED: [(&str, AgentKind); 2] = [
    ("claude", AgentKind::ClaudeCode),
    ("codex", AgentKind::Codex),
];

/// 要启动的 Agent。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentProgram {
    /// 交给本地终端的命令行：设置里写的，或检测到的程序名（Windows 上由终端按 `PATHEXT` 找
    /// `claude.cmd`、`claude.exe`）。
    pub command: String,
    /// 显示的名字：Claude Code、Codex，认不出的程序是它的文件名。
    pub name: SharedString,
    /// 认得出的 Agent（它的 Skill 装在哪里）；认不出的程序是 `None`。
    pub kind: Option<AgentKind>,
}

/// 设置里的命令（`None` 或空白是自动检测）对应的 Agent；自动检测没找到时 `None`。
/// 用户自己写的命令不检查找不找得到：找不到时终端里会说。阻塞：会看文件系统。
pub fn agent_program(setting: Option<&str>) -> Option<AgentProgram> {
    agent_program_on(setting, None)
}

/// 同 [`agent_program`]，自动检测时在 `path` 里找（`None` 是这个进程的 `PATH`）。
pub fn agent_program_on(setting: Option<&str>, path: Option<&OsStr>) -> Option<AgentProgram> {
    let own = std::env::var_os("PATH").unwrap_or_default();
    let extensions = std::env::var_os("PATHEXT");
    agent_program_in(setting, path.unwrap_or(&own), extensions.as_deref())
}

fn agent_program_in(
    setting: Option<&str>,
    path: &OsStr,
    extensions: Option<&OsStr>,
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
        });
    }
    DETECTED
        .iter()
        .find(|(name, _)| on_path(name, path, extensions))
        .map(|(name, kind)| AgentProgram {
            command: name.to_string(),
            name: kind.label(),
            kind: Some(*kind),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_in_the_settings_is_taken_as_written() {
        let empty = OsStr::new("");
        let claude = agent_program_in(Some(" claude --model opus "), empty, None).unwrap();
        assert_eq!(claude.command, "claude --model opus");
        assert_eq!(claude.name.as_ref(), "Claude Code");
        assert_eq!(claude.kind, Some(AgentKind::ClaudeCode));

        let quoted =
            agent_program_in(Some(r#""C:\Tools\codex.cmd" --full-auto"#), empty, None).unwrap();
        assert_eq!(quoted.kind, Some(AgentKind::Codex));

        let other = agent_program_in(Some("aider --yes"), empty, None).unwrap();
        assert_eq!(other.name.as_ref(), "aider");
        assert_eq!(other.kind, None);
    }

    #[test]
    fn left_empty_it_looks_for_claude_then_codex_on_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().as_os_str();
        assert_eq!(agent_program_in(None, path, None), None);
        assert_eq!(agent_program_in(Some("  "), path, None), None);

        let file = |name: &str| std::fs::write(dir.path().join(name), "").unwrap();
        file(if cfg!(windows) { "codex.cmd" } else { "codex" });
        let found = agent_program_in(None, path, None).unwrap();
        assert_eq!(
            (found.command.as_str(), found.kind),
            ("codex", Some(AgentKind::Codex))
        );

        file(if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        });
        let found = agent_program_in(None, path, None).unwrap();
        assert_eq!(found.command, "claude");
        assert_eq!(found.name.as_ref(), "Claude Code");
    }
}
