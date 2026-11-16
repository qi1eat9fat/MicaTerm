//! Agent 的状态 hook（Agent CLI 工单 09，设置 › Agent 的「Agent 状态」一组）：往 Agent 自己的配置文件里
//! 加几项调用 `micaterm hook` 的 hook，或者删掉。
//!
//! - Claude Code：`~/.claude/settings.json` 的 `hooks` 里，`UserPromptSubmit`、`Notification`、`Stop` 各加一项。
//!   按 JSON 改，键的顺序不动。
//! - Codex：`~/.codex/config.toml` 顶层的一行 `notify = [...]`。按行改，别的行、注释和格式都不动；
//!   已经有别人的 `notify` 时不碰它，说为什么。
//!
//! 改之前把原文件备份成同目录的 `<文件名>.micaterm-backup`。只增删自己那几项（命令里有 `micaterm`
//! 和 ` hook claude` / ` hook codex` 的）；读不懂的文件不写，说为什么。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::install::{AgentKind, IntegrationPaths};
use crate::i18n::t;

/// Claude Code 装的三个事件。
const CLAUDE_EVENTS: [&str; 3] = ["UserPromptSubmit", "Notification", "Stop"];
/// 备份文件名的后缀。
const BACKUP_SUFFIX: &str = ".micaterm-backup";

/// 装没装。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookStatus {
    Installed,
    Missing,
    /// 读不懂配置文件，或者里面有别人的同名设置：不会去改。
    Blocked(String),
}

/// 能装状态 hook 的 Agent，按设置页的顺序。
pub const HOOK_AGENTS: [AgentKind; 2] = [AgentKind::ClaudeCode, AgentKind::Codex];

/// `agent` 的配置文件。
pub fn hook_file(paths: &IntegrationPaths, agent: AgentKind) -> Option<PathBuf> {
    match agent {
        AgentKind::ClaudeCode => Some(paths.home.join(".claude").join("settings.json")),
        AgentKind::Codex => Some(paths.home.join(".codex").join("config.toml")),
        AgentKind::Generic | AgentKind::WorkBuddy => None,
    }
}

pub fn hook_status(paths: &IntegrationPaths, agent: AgentKind) -> HookStatus {
    let Some(file) = hook_file(paths, agent) else {
        return HookStatus::Missing;
    };
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return HookStatus::Missing,
        Err(error) => return HookStatus::Blocked(error.to_string()),
    };
    match agent {
        AgentKind::ClaudeCode => match parse_claude(&text) {
            Ok(root) => {
                let hooks = root.get("hooks").and_then(Value::as_object);
                let all = CLAUDE_EVENTS.iter().all(|event| {
                    hooks
                        .and_then(|hooks| hooks.get(*event))
                        .and_then(Value::as_array)
                        .is_some_and(|groups| groups.iter().any(group_is_ours))
                });
                if all {
                    HookStatus::Installed
                } else {
                    HookStatus::Missing
                }
            }
            Err(error) => HookStatus::Blocked(error.to_string()),
        },
        AgentKind::Codex => match codex_notify(&text) {
            Some((_, line)) if is_ours(line, "codex") => HookStatus::Installed,
            Some((_, line)) => HookStatus::Blocked(foreign_notify(line)),
            None => HookStatus::Missing,
        },
        AgentKind::Generic | AgentKind::WorkBuddy => HookStatus::Missing,
    }
}

/// 装上（已经装着就换成现在的命令路径）。
pub fn install_hooks(paths: &IntegrationPaths, agent: AgentKind) -> io::Result<()> {
    let Some(file) = hook_file(paths, agent) else {
        return Ok(());
    };
    let command = command_path(paths);
    let text = read_or_empty(&file)?;
    let new = match agent {
        AgentKind::ClaudeCode => {
            let mut root = parse_claude(&text)?;
            strip_claude(&mut root);
            let line = format!("\"{}\" hook claude", shell_path(&command));
            let hooks = root
                .as_object_mut()
                .expect("parse_claude only returns objects")
                .entry("hooks")
                .or_insert_with(|| Value::Object(Map::new()));
            let hooks = hooks
                .as_object_mut()
                .ok_or_else(|| invalid(&t!("settings.cli.hooks_not_object")))?;
            for event in CLAUDE_EVENTS {
                let groups = hooks
                    .entry(event)
                    .or_insert_with(|| Value::Array(Vec::new()));
                groups
                    .as_array_mut()
                    .ok_or_else(|| invalid(&t!("settings.cli.hooks_not_array", event = event)))?
                    .push(json!({ "hooks": [{ "type": "command", "command": line }] }));
            }
            pretty(&root)?
        }
        AgentKind::Codex => {
            let ours = format!(
                "notify = [{}, \"hook\", \"codex\"]",
                serde_json::to_string(&command.display().to_string()).map_err(io::Error::other)?
            );
            let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
            match codex_notify(&text) {
                Some((index, line)) if is_ours(line, "codex") => lines[index] = ours,
                Some((_, line)) => return Err(invalid(&foreign_notify(line))),
                None => {
                    // 顶层的键要写在第一个 [表] 前面。
                    let at = lines
                        .iter()
                        .position(|line| line.trim_start().starts_with('['))
                        .unwrap_or(lines.len());
                    lines.insert(at, ours);
                }
            }
            let mut new = lines.join("\n");
            new.push('\n');
            new
        }
        AgentKind::Generic | AgentKind::WorkBuddy => return Ok(()),
    };
    write_with_backup(&file, &text, &new)
}

/// 删掉自己那几项；没有配置文件、本来就没装时什么也不做。
pub fn remove_hooks(paths: &IntegrationPaths, agent: AgentKind) -> io::Result<()> {
    let Some(file) = hook_file(paths, agent) else {
        return Ok(());
    };
    if !file.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(&file)?;
    let new = match agent {
        AgentKind::ClaudeCode => {
            let mut root = parse_claude(&text)?;
            if !strip_claude(&mut root) {
                return Ok(());
            }
            pretty(&root)?
        }
        AgentKind::Codex => match codex_notify(&text) {
            Some((index, line)) if is_ours(line, "codex") => {
                let mut lines: Vec<&str> = text.lines().collect();
                lines.remove(index);
                let mut new = lines.join("\n");
                new.push('\n');
                new
            }
            _ => return Ok(()),
        },
        AgentKind::Generic | AgentKind::WorkBuddy => return Ok(()),
    };
    write_with_backup(&file, &text, &new)
}

/// hook 里写的命令：装好的 `micaterm`，没装时程序旁边那个（不靠 PATH，Agent 的 PATH 不一定有它）。
fn command_path(paths: &IntegrationPaths) -> PathBuf {
    if paths.bin_link.is_file() {
        paths.bin_link.clone()
    } else {
        paths.exe.clone()
    }
}

/// 写进 shell 命令行的路径：Windows 上用 `/`，Git Bash、cmd 都认。
fn shell_path(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

fn is_ours(command: &str, source: &str) -> bool {
    command.contains("micaterm") && command.contains(&format!(" hook {source}"))
        || command.contains("micaterm") && command.contains(&format!("\"hook\", \"{source}\""))
}

fn group_is_ours(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| hooks.iter().any(hook_is_ours))
}

fn hook_is_ours(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| is_ours(command, "claude"))
}

/// 读 settings.json：空文件当 `{}`；不是 JSON 对象就说不懂。
fn parse_claude(text: &str) -> io::Result<Value> {
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let root: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|error| invalid(&t!("settings.cli.hooks_bad_json", error = error)))?;
    if !root.is_object() {
        return Err(invalid(&t!("settings.cli.hooks_not_json_object")));
    }
    if root.get("hooks").is_some_and(|hooks| !hooks.is_object()) {
        return Err(invalid(&t!("settings.cli.hooks_not_object")));
    }
    Ok(root)
}

/// 去掉自己装的 hook：组里只剩空的就去掉组，事件下没有组了就去掉事件，`hooks` 空了就去掉它。
/// 改过返回 `true`。
fn strip_claude(root: &mut Value) -> bool {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(inner) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = inner.len();
                inner.retain(|hook| !hook_is_ours(hook));
                changed |= inner.len() != before;
            }
        }
        let before = groups.len();
        groups.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|inner| !inner.is_empty())
        });
        changed |= groups.len() != before;
    }
    if changed {
        hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
        if hooks.is_empty() {
            root.as_object_mut()
                .expect("parse_claude only returns objects")
                .remove("hooks");
        }
    }
    changed
}

/// config.toml 顶层（第一个 [表] 之前）的 `notify = ...` 行：第几行、那一行。
fn codex_notify(text: &str) -> Option<(usize, &str)> {
    text.lines()
        .take_while(|line| !line.trim_start().starts_with('['))
        .enumerate()
        .find(|(_, line)| {
            line.trim_start()
                .strip_prefix("notify")
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        })
}

fn foreign_notify(line: &str) -> String {
    t!("settings.cli.hooks_foreign_notify", line = line.trim()).to_string()
}

fn read_or_empty(file: &Path) -> io::Result<String> {
    match fs::read_to_string(file) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

fn pretty(root: &Value) -> io::Result<String> {
    let mut text = serde_json::to_string_pretty(root).map_err(io::Error::other)?;
    text.push('\n');
    Ok(text)
}

/// 原文件在就先备份，再写新的；目录不在就建。
fn write_with_backup(file: &Path, old: &str, new: &str) -> io::Result<()> {
    if file.exists() {
        let mut backup = file.as_os_str().to_owned();
        backup.push(BACKUP_SUFFIX);
        fs::write(PathBuf::from(backup), old)?;
    } else if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(file, new)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &Path) -> IntegrationPaths {
        let exe = root.join("app").join("micaterm-cli.exe");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "cli").unwrap();
        IntegrationPaths {
            home: root.join("home"),
            bin_link: root.join("app").join("bin").join("micaterm.exe"),
            exe,
            user_path: None,
        }
    }

    fn claude_file(paths: &IntegrationPaths) -> PathBuf {
        hook_file(paths, AgentKind::ClaudeCode).unwrap()
    }

    /// 装上保留别的内容和顺序、生成备份；重装不重复；移除只删自己的。
    #[test]
    fn claude_hooks_keep_the_rest_of_the_settings() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let file = claude_file(&paths);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let original = r#"{
  "model": "opus",
  "hooks": {
    "Stop": [
      { "hooks": [ { "type": "command", "command": "say done" } ] }
    ]
  },
  "permissions": { "allow": ["Bash(ls)"] }
}
"#;
        fs::write(&file, original).unwrap();
        assert_eq!(
            hook_status(&paths, AgentKind::ClaudeCode),
            HookStatus::Missing
        );

        install_hooks(&paths, AgentKind::ClaudeCode).unwrap();
        install_hooks(&paths, AgentKind::ClaudeCode).unwrap();
        assert_eq!(
            hook_status(&paths, AgentKind::ClaudeCode),
            HookStatus::Installed
        );
        let backup = fs::read_to_string(format!("{}{BACKUP_SUFFIX}", file.display())).unwrap();
        assert!(
            serde_json::from_str::<Value>(&backup).is_ok(),
            "备份是改之前的那份（第二次装时是第一次装完的）"
        );
        let text = fs::read_to_string(&file).unwrap();
        let root: Value = serde_json::from_str(&text).unwrap();
        let keys: Vec<&str> = root
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["model", "hooks", "permissions"], "顺序不动");
        let stop = root["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "别人的那项留着，自己的只有一项");
        assert_eq!(stop[0]["hooks"][0]["command"], "say done");
        assert!(
            stop[1]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .ends_with(" hook claude")
        );
        assert_eq!(
            root["hooks"]["UserPromptSubmit"].as_array().unwrap().len(),
            1
        );

        remove_hooks(&paths, AgentKind::ClaudeCode).unwrap();
        let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            root,
            serde_json::from_str::<Value>(original).unwrap(),
            "移除后和原来一样"
        );
        assert_eq!(
            hook_status(&paths, AgentKind::ClaudeCode),
            HookStatus::Missing
        );
    }

    #[test]
    fn a_missing_settings_file_is_created_and_a_broken_one_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let file = claude_file(&paths);
        install_hooks(&paths, AgentKind::ClaudeCode).unwrap();
        assert!(file.is_file());
        assert_eq!(
            hook_status(&paths, AgentKind::ClaudeCode),
            HookStatus::Installed
        );

        fs::write(&file, "{ broken").unwrap();
        assert!(matches!(
            hook_status(&paths, AgentKind::ClaudeCode),
            HookStatus::Blocked(_)
        ));
        assert!(install_hooks(&paths, AgentKind::ClaudeCode).is_err());
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "{ broken",
            "读不懂的不写"
        );
    }

    #[test]
    fn codex_notify_is_one_line_before_the_tables_and_others_are_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let file = hook_file(&paths, AgentKind::Codex).unwrap();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let original =
            "# my config\nmodel = \"gpt-5\"\n\n[profiles.fast]\nmodel = \"gpt-5-mini\"\n";
        fs::write(&file, original).unwrap();

        install_hooks(&paths, AgentKind::Codex).unwrap();
        install_hooks(&paths, AgentKind::Codex).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        let notify: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("notify"))
            .collect();
        assert_eq!(notify.len(), 1, "{text}");
        assert!(notify[0].ends_with("\"hook\", \"codex\"]"), "{text}");
        let header = text
            .lines()
            .position(|line| line.starts_with("[profiles"))
            .unwrap();
        let at = text
            .lines()
            .position(|line| line.starts_with("notify"))
            .unwrap();
        assert!(at < header, "在第一个表前面");
        assert!(text.starts_with("# my config\nmodel = \"gpt-5\"\n"));
        assert_eq!(hook_status(&paths, AgentKind::Codex), HookStatus::Installed);

        remove_hooks(&paths, AgentKind::Codex).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), original);

        // 别人的 notify：不碰，说为什么。
        fs::write(&file, "notify = [\"terminal-notifier\"]\n").unwrap();
        assert!(matches!(
            hook_status(&paths, AgentKind::Codex),
            HookStatus::Blocked(_)
        ));
        assert!(install_hooks(&paths, AgentKind::Codex).is_err());
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "notify = [\"terminal-notifier\"]\n"
        );
    }
}
