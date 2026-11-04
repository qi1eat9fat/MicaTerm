// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-hosts/src/data/snippets.rs
// 裁剪：去掉片段分类和作用域、同步用的 `sync_id`、`edited_ms`。

//! The command snippets' part of `HostData`, written through like
//! everything else.

use super::{Change, HostData};
use crate::{Snippet, SnippetDraft, SnippetId, by_name};

impl HostData {
    /// Every snippet, in the order they were made.
    pub fn snippets(&self) -> &[Snippet] {
        &self.snippets
    }

    /// 命令片段面板的列表顺序：按名称排，不分大小写，同名的按新建先后。
    pub fn snippets_by_name(&self) -> Vec<&Snippet> {
        let mut snippets: Vec<&Snippet> = self.snippets.iter().collect();
        snippets.sort_by(|a, b| by_name((&a.name, a.id.0), (&b.name, b.id.0)));
        snippets
    }

    pub fn snippet(&self, id: SnippetId) -> Option<&Snippet> {
        self.snippets.iter().find(|snippet| snippet.id == id)
    }

    pub fn insert_snippet(&mut self, draft: SnippetDraft) -> SnippetId {
        let id = self.insert_snippet_unnotified(draft);
        if let Some(snippet) = self.snippet(id) {
            self.persist(Change::NewSnippet, |db| db.insert_snippet(snippet));
        }
        self.changed();
        id
    }

    pub fn insert_snippet_unnotified(&mut self, draft: SnippetDraft) -> SnippetId {
        let id = SnippetId(self.next_snippet_id);
        self.next_snippet_id += 1;
        self.snippets.push(Snippet::new(id, draft));
        id
    }

    pub fn update_snippet(&mut self, id: SnippetId, draft: SnippetDraft) -> bool {
        if !self.update_snippet_unnotified(id, draft) {
            return false;
        }
        if let Some(snippet) = self.snippet(id) {
            self.persist(Change::SaveSnippet, |db| db.update_snippet(snippet));
        }
        self.changed();
        true
    }

    pub fn update_snippet_unnotified(&mut self, id: SnippetId, draft: SnippetDraft) -> bool {
        let Some(snippet) = self.snippets.iter_mut().find(|snippet| snippet.id == id) else {
            return false;
        };
        *snippet = Snippet::new(id, draft);
        true
    }

    pub fn remove_snippet(&mut self, id: SnippetId) -> bool {
        if !self.remove_snippet_unnotified(id) {
            return false;
        }
        self.persist(Change::DeleteSnippet, |db| db.remove_snippet(id));
        self.changed();
        true
    }

    pub fn remove_snippet_unnotified(&mut self, id: SnippetId) -> bool {
        let before = self.snippets.len();
        self.snippets.retain(|snippet| snippet.id != id);
        self.snippets.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthKind, HostDatabase, HostDraft, HostEvent};

    fn draft(name: &str) -> SnippetDraft {
        SnippetDraft::new(name, format!("echo {name}"))
    }

    fn names(store: &HostData) -> Vec<String> {
        store
            .snippets()
            .iter()
            .map(|snippet| snippet.name.to_string())
            .collect()
    }

    #[test]
    fn a_snippet_is_edited_in_place_and_removed() {
        let mut store = HostData::empty();
        let ps = store.insert_snippet_unnotified(draft("ps"));
        let df = store.insert_snippet_unnotified(draft("df"));
        assert!(store.update_snippet_unnotified(
            ps,
            SnippetDraft::new("容器", "docker ps").with_run_on_click(true)
        ));
        let edited = store.snippet(ps).unwrap();
        assert_eq!(edited.command, "docker ps");
        assert!(edited.run_on_click);
        assert_eq!(names(&store), ["容器", "df"]);
        assert!(store.remove_snippet_unnotified(df));
        assert!(!store.remove_snippet_unnotified(df));
        assert!(!store.update_snippet_unnotified(df, draft("df")));
        assert_eq!(names(&store), ["容器"]);
    }

    #[test]
    fn snippets_are_listed_by_name() {
        let mut store = HostData::empty();
        for name in ["nginx", "Docker", "apt", "docker"] {
            store.insert_snippet_unnotified(draft(name));
        }
        let listed: Vec<_> = store
            .snippets_by_name()
            .into_iter()
            .map(|snippet| snippet.name.as_ref())
            .collect();
        assert_eq!(listed, ["apt", "Docker", "docker", "nginx"]);
    }

    #[test]
    fn snippets_are_shared_by_every_host_and_outlive_them() {
        let mut store = HostData::empty();
        let host = store.insert_unnotified(HostDraft::new(
            "db-01",
            "10.0.0.2",
            22,
            "root",
            AuthKind::Password,
        ));
        store.insert_snippet_unnotified(draft("df"));
        store.remove_unnotified(host);
        assert_eq!(names(&store), ["df"]);
    }

    #[test]
    fn snippets_are_written_through_and_the_id_sequence_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qishell.db");
        let mut store = HostData::load(HostDatabase::open(&path).unwrap()).unwrap();
        let ps = store.insert_snippet(draft("ps"));
        let df = store.insert_snippet(draft("df"));
        let tail = store.insert_snippet(draft("tail"));
        assert!(store.update_snippet(ps, SnippetDraft::new("容器", "docker ps -a\ndocker images")));
        assert!(store.remove_snippet(df));
        assert!(
            !store
                .take_events()
                .iter()
                .any(|event| matches!(event, HostEvent::Failed(_)))
        );

        let mut reopened = HostData::load(HostDatabase::open(&path).unwrap()).unwrap();
        assert_eq!(names(&reopened), ["容器", "tail"]);
        assert_eq!(
            reopened.snippet(ps).unwrap().command,
            "docker ps -a\ndocker images"
        );
        assert!(reopened.snippet(tail).is_some());
        assert_eq!(
            reopened.insert_snippet_unnotified(draft("df")),
            SnippetId(4)
        );
    }
}
