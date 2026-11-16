//! 启动框里列的历史对话（2026-11-16 用户定：对话可以挑）：Claude Code 把每个文件夹的对话记在
//! `<配置目录>/projects/<文件夹编码>/<会话 ID>.jsonl`，一行一条 JSON。这里只读标题和时间：
//! 标题先用会话标题（`customTitle`，Claude 起的或用户改的），再用摘要（`summary`），再用最后一句
//! 提问（`lastPrompt`），都没有时用第一句提问；什么都没有的（空会话）不列。
//! 文件可能有几 MB，标题这几种记录在文件尾，只读尾部；第一句提问只读头部。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Datelike, Local};
use serde_json::Value;

/// 最多列几个（按最近用过排）。
const MAX_LISTED: usize = 50;
/// 找标题时读文件尾部多少字节。
const TAIL_BYTES: u64 = 256 * 1024;
/// 找第一句提问时读文件头部多少字节。
const HEAD_BYTES: u64 = 256 * 1024;
/// 标题最多几个字。
const TITLE_CHARS: usize = 80;

/// 一个能接着的对话。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    /// 会话 ID（文件名），`claude --resume <ID>` 用它。
    pub id: String,
    pub title: String,
    /// 最后一次写记录的时间。
    pub modified: SystemTime,
}

/// Claude Code 放配置和对话记录的目录：`CLAUDE_CONFIG_DIR`，没设是 `~/.claude`。
pub fn claude_config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
}

/// Claude Code 在 `folder` 里的对话，最近用过的在前，最多 [`MAX_LISTED`] 个。
/// 没聊过、目录不在时是空的。阻塞：会读文件，放到后台。
pub fn claude_conversations(config: &Path, folder: &Path) -> Vec<Conversation> {
    let project = config.join("projects").join(claude_project_name(folder));
    let Ok(entries) = std::fs::read_dir(project) else {
        return Vec::new();
    };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| {
                (
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    entry.path(),
                )
            })
        })
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    files
        .into_iter()
        .filter_map(|(modified, path)| {
            let id = path.file_stem()?.to_string_lossy().into_owned();
            let title = conversation_title(&path)?;
            Some(Conversation {
                id,
                title,
                modified,
            })
        })
        .take(MAX_LISTED)
        .collect()
}

/// Claude Code 给文件夹起的记录目录名：字母和数字以外的字符都换成 `-`
/// （`E:\vibe\ssh` → `E--vibe-ssh`）。
fn claude_project_name(folder: &Path) -> String {
    folder
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// 一个对话的标题；读不出来（空会话、读不了）时 `None`。
fn conversation_title(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut tail).ok()?;
    let tail = String::from_utf8_lossy(&tail);
    // 从文件中间开始读时第一行多半不完整，丢掉。
    let lines: Vec<&str> = tail.lines().skip(usize::from(start > 0)).collect();
    if let Some(title) = title_in_tail(lines.into_iter()) {
        return Some(title);
    }
    let mut file = File::open(path).ok()?;
    let mut head = Vec::new();
    (&mut file).take(HEAD_BYTES).read_to_end(&mut head).ok()?;
    first_prompt(String::from_utf8_lossy(&head).lines())
}

/// 尾部里最后写的会话标题，没有就摘要，再没有就最后一句提问。
fn title_in_tail<'a>(lines: impl DoubleEndedIterator<Item = &'a str>) -> Option<String> {
    let (mut custom, mut summary, mut last_prompt) = (None, None, None);
    for line in lines.rev() {
        if custom.is_some() {
            break;
        }
        let wanted = (custom.is_none() && line.contains("\"customTitle\""))
            || (summary.is_none() && line.contains("\"summary\""))
            || (last_prompt.is_none() && line.contains("\"lastPrompt\""));
        if !wanted {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let text = |key: &str| record.get(key).and_then(Value::as_str).and_then(clean);
        match record.get("type").and_then(Value::as_str) {
            Some("custom-title") if custom.is_none() => custom = text("customTitle"),
            Some("summary") if summary.is_none() => summary = text("summary"),
            Some("last-prompt") if last_prompt.is_none() => last_prompt = text("lastPrompt"),
            _ => {}
        }
    }
    custom.or(summary).or(last_prompt)
}

/// 头部里用户的第一句话（不算系统塞进来的、以 `<` 开头的命令记录）。
fn first_prompt<'a>(lines: impl Iterator<Item = &'a str>) -> Option<String> {
    lines
        .filter(|line| line.contains("\"user\""))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record.get("type").and_then(Value::as_str) == Some("user"))
        .filter(|record| record.get("isMeta").and_then(Value::as_bool) != Some(true))
        .find_map(|record| {
            let content = record.get("message")?.get("content")?;
            let text = match content {
                Value::String(text) => Some(text.as_str()),
                Value::Array(blocks) => blocks.iter().find_map(|block| {
                    (block.get("type").and_then(Value::as_str) == Some("text"))
                        .then(|| block.get("text").and_then(Value::as_str))
                        .flatten()
                }),
                _ => None,
            }?;
            (!text.trim_start().starts_with('<'))
                .then(|| clean(text))
                .flatten()
        })
}

/// 标题只要一行：空白并成一个空格，太长截断。
fn clean(text: &str) -> Option<String> {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    let mut chars = line.chars();
    let short: String = chars.by_ref().take(TITLE_CHARS).collect();
    Some(if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    })
}

/// 列表右边的时间：今天的写时分，今年的写月日，更早的写年月日。
pub fn conversation_time(modified: SystemTime, now: DateTime<Local>) -> String {
    let time: DateTime<Local> = modified.into();
    if time.date_naive() == now.date_naive() {
        time.format("%H:%M").to_string()
    } else if time.year() == now.year() {
        time.format("%m-%d").to_string()
    } else {
        time.format("%Y-%m-%d").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn write(dir: &Path, id: &str, lines: &[&str], modified: SystemTime) {
        let file = dir.join(format!("{id}.jsonl"));
        std::fs::write(&file, lines.join("\n")).unwrap();
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }

    #[test]
    fn a_folder_is_named_the_way_claude_code_names_it() {
        assert_eq!(
            claude_project_name(Path::new(r"E:\vibe\ssh")),
            "E--vibe-ssh"
        );
        assert_eq!(
            claude_project_name(Path::new("/home/qi/my.app")),
            "-home-qi-my-app"
        );
    }

    #[test]
    fn conversations_are_listed_newest_first_with_the_best_title_they_have() {
        let config = tempfile::tempdir().unwrap();
        let folder = Path::new(r"E:\vibe\ssh");
        assert!(claude_conversations(config.path(), folder).is_empty());

        let dir = config.path().join("projects").join("E--vibe-ssh");
        std::fs::create_dir_all(dir.join("a-subfolder")).unwrap();
        let at =
            |minutes: u64| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(minutes * 60);
        let user = |text: &str| {
            format!(r#"{{"type":"user","message":{{"role":"user","content":"{text}"}}}}"#)
        };
        let first = user("<command-name>/clear</command-name>");
        // JSON 里的换行是 `\n` 转义。
        let second = user(r"  修一下\n 登录页  ");
        write(&dir, "first-prompt", &[&first, &second], at(10));
        write(
            &dir,
            "titled",
            &[
                &user("早先的问题"),
                r#"{"type":"last-prompt","lastPrompt":"最后一句"}"#,
                r#"{"type":"custom-title","customTitle":"旧标题"}"#,
                r#"{"type":"custom-title","customTitle":"新标题"}"#,
                r#"{"type":"assistant","message":{}}"#,
            ],
            at(30),
        );
        write(
            &dir,
            "last-prompt",
            &[r#"{"type":"last-prompt","lastPrompt":"只有最后一句"}"#],
            at(20),
        );
        write(&dir, "empty", &[r#"{"type":"system"}"#], at(40));

        let listed = claude_conversations(config.path(), folder);
        let seen: Vec<(&str, &str)> = listed
            .iter()
            .map(|it| (it.id.as_str(), it.title.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                ("titled", "新标题"),
                ("last-prompt", "只有最后一句"),
                ("first-prompt", "修一下 登录页"),
            ],
            "空会话不列；最近的在前"
        );
        assert_eq!(listed[0].modified, at(30));
    }

    #[test]
    fn a_long_title_is_cut_to_one_short_line() {
        let long = "字".repeat(100);
        let title = clean(&long).unwrap();
        assert_eq!(title.chars().count(), TITLE_CHARS + 1);
        assert!(title.ends_with('…'));
        assert_eq!(clean(" \n "), None);
    }

    #[test]
    fn the_time_is_as_short_as_it_can_be() {
        let now = Local.with_ymd_and_hms(2026, 11, 16, 18, 0, 0).unwrap();
        let at = |y, m, d, h, min| -> SystemTime {
            Local.with_ymd_and_hms(y, m, d, h, min, 0).unwrap().into()
        };
        assert_eq!(conversation_time(at(2026, 11, 16, 9, 5), now), "09:05");
        assert_eq!(conversation_time(at(2026, 11, 15, 23, 0), now), "11-15");
        assert_eq!(conversation_time(at(2025, 3, 1, 8, 0), now), "2025-03-01");
    }
}
