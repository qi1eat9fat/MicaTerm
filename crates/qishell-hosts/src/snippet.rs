// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-hosts/src/snippet.rs
// 裁剪：去掉片段分类（`SnippetCategory`）和作用域（`SnippetScope`），所有主机共用；去掉同步用的
// `sync_id`、`edited_ms`。

//! Command snippets: the commands 命令片段 keeps, shared by every host.
//! They live with the hosts because they are stored in the same database;
//! the snippet panel that shows them depends on this one.

use std::cmp::Ordering;

use gpui_shared_string::SharedString;

/// Stable identity of a snippet. Never reused within a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SnippetId(pub u64);

/// Why a snippet draft cannot be saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnippetDraftError {
    Name,
    Command,
}

/// What the snippet form edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnippetDraft {
    pub name: SharedString,
    /// What it types into the terminal, one line or several.
    pub command: String,
    /// Whether a click on it runs it, rather than only putting it on the
    /// terminal's line (「点击时自动执行」).
    pub run_on_click: bool,
}

impl SnippetDraft {
    pub fn new(name: impl Into<SharedString>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            run_on_click: false,
        }
    }

    pub fn with_run_on_click(mut self, run_on_click: bool) -> Self {
        self.run_on_click = run_on_click;
        self
    }

    /// The draft as the form's fields give it, tidied, or why it cannot be
    /// saved. Blank lines and spaces around the command go; inside it,
    /// every line stays as written.
    pub fn validated(name: &str, command: &str) -> Result<Self, SnippetDraftError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(SnippetDraftError::Name);
        }
        let command = command.trim_end().trim_start_matches(['\n', '\r']);
        if command.trim().is_empty() {
            return Err(SnippetDraftError::Command);
        }
        Ok(Self::new(name.to_owned(), command.to_owned()))
    }
}

/// A saved command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub id: SnippetId,
    pub name: SharedString,
    pub command: String,
    pub run_on_click: bool,
}

impl Snippet {
    pub fn new(id: SnippetId, draft: SnippetDraft) -> Self {
        Self {
            id,
            name: draft.name,
            command: draft.command,
            run_on_click: draft.run_on_click,
        }
    }

    pub fn draft(&self) -> SnippetDraft {
        SnippetDraft {
            name: self.name.clone(),
            command: self.command.clone(),
            run_on_click: self.run_on_click,
        }
    }

    /// Whether the snippet holds every word of `query` in its name or its
    /// command, regardless of case.
    pub fn matches(&self, query: &str) -> bool {
        let name = self.name.to_lowercase();
        let command = self.command.to_lowercase();
        query.split_whitespace().all(|word| {
            let word = word.to_lowercase();
            name.contains(&word) || command.contains(&word)
        })
    }
}

/// The order snippets are listed in: by name, regardless of case, then by
/// when they were made (`(name, id)`).
pub fn by_name(a: (&str, u64), b: (&str, u64)) -> Ordering {
    a.0.to_lowercase()
        .cmp(&b.0.to_lowercase())
        .then(a.0.cmp(b.0))
        .then(a.1.cmp(&b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snippet_needs_a_name_and_a_command() {
        assert_eq!(
            SnippetDraft::validated("  ", "ls"),
            Err(SnippetDraftError::Name)
        );
        assert_eq!(
            SnippetDraft::validated("列表", " \n \n"),
            Err(SnippetDraftError::Command)
        );
        let draft = SnippetDraft::validated(
            " 清理日志 ",
            "\n\n  find /var/log -mtime +7\n  -delete \n\n",
        )
        .unwrap();
        assert_eq!(draft.name, "清理日志");
        assert_eq!(draft.command, "  find /var/log -mtime +7\n  -delete");
        assert!(!draft.run_on_click);
    }

    #[test]
    fn the_search_looks_in_the_name_and_the_command() {
        let snippet = Snippet::new(
            SnippetId(1),
            SnippetDraft::new("查看容器", "docker ps -a\ndocker images"),
        );
        assert!(snippet.matches(""));
        assert!(snippet.matches("容器"));
        assert!(snippet.matches("DOCKER images"));
        assert!(snippet.matches("容器 ps"));
        assert!(!snippet.matches("kubectl"));
    }

    #[test]
    fn names_sort_regardless_of_case() {
        let mut names = vec![("nginx", 1), ("Docker", 2), ("apt", 3), ("docker", 4)];
        names.sort_by(|a, b| by_name(*a, *b));
        assert_eq!(
            names,
            [("apt", 3), ("Docker", 2), ("docker", 4), ("nginx", 1)]
        );
    }
}
