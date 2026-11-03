//! 标签列表的纯逻辑：按顺序放标签，记着哪个在前面。不依赖 gpui。
//! 02 只用到新建和关闭；工单 04 在这里加上一个 / 下一个 / 第 n 个、关闭其他、关闭右侧。

/// 一个标签的 id：从 1 起递增，不复用。元素 id 写成 `("tab", id)`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

/// 按顺序排着的标签，每个带一份 `T`（终端标签是它的卡片），加上在前的那个。
#[derive(Debug)]
pub struct TabList<T> {
    tabs: Vec<(TabId, T)>,
    active: Option<TabId>,
    next_id: u64,
}

impl<T> Default for TabList<T> {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            next_id: 1,
        }
    }
}

impl<T> TabList<T> {
    /// 在最后加一个标签并让它在前，返回它的 id。
    pub fn push(&mut self, content: T) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        self.tabs.push((id, content));
        self.active = Some(id);
        id
    }

    /// 关掉 `id`，交回它的内容。关的是在前的那个时，左边的那个到前面来（没有左边就右边）。
    pub fn remove(&mut self, id: TabId) -> Option<T> {
        let index = self.index_of(id)?;
        let (_, content) = self.tabs.remove(index);
        if self.active == Some(id) {
            self.active = self.tabs.get(index.saturating_sub(1)).map(|(id, _)| *id);
        }
        Some(content)
    }

    /// 让 `id` 到前面来；没有这个标签时什么都不做，返回 `false`。
    pub fn activate(&mut self, id: TabId) -> bool {
        let found = self.index_of(id).is_some();
        if found {
            self.active = Some(id);
        }
        found
    }

    pub fn active(&self) -> Option<TabId> {
        self.active
    }

    pub fn active_content(&self) -> Option<&T> {
        self.active.and_then(|id| self.get(id))
    }

    pub fn get(&self, id: TabId) -> Option<&T> {
        self.tabs
            .iter()
            .find(|(tab, _)| *tab == id)
            .map(|(_, content)| content)
    }

    /// 从左到右的标签。
    pub fn iter(&self) -> impl Iterator<Item = (TabId, &T)> {
        self.tabs.iter().map(|(id, content)| (*id, content))
    }

    pub fn ids(&self) -> Vec<TabId> {
        self.tabs.iter().map(|(id, _)| *id).collect()
    }

    /// 内容满足 `find` 的第一个标签。
    pub fn find(&self, mut find: impl FnMut(&T) -> bool) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|(_, content)| find(content))
            .map(|(id, _)| *id)
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|(tab, _)| *tab == id)
    }
}

#[cfg(test)]
mod tests {
    use super::{TabId, TabList};

    fn list(names: &[&'static str]) -> TabList<&'static str> {
        let mut tabs = TabList::default();
        for name in names {
            tabs.push(*name);
        }
        tabs
    }

    #[test]
    fn a_new_tab_goes_last_and_comes_to_the_front_with_the_next_id() {
        let tabs = list(&["a", "b", "c"]);
        assert_eq!(tabs.ids(), [TabId(1), TabId(2), TabId(3)]);
        assert_eq!(tabs.active(), Some(TabId(3)));
        assert_eq!(tabs.active_content(), Some(&"c"));
    }

    #[test]
    fn closing_the_front_tab_brings_its_left_neighbour_forward() {
        let mut tabs = list(&["a", "b", "c"]);
        tabs.activate(TabId(2));
        assert_eq!(tabs.remove(TabId(2)), Some("b"));
        assert_eq!(tabs.active(), Some(TabId(1)));
        // 最左边的关了，右边的上来。
        assert_eq!(tabs.remove(TabId(1)), Some("a"));
        assert_eq!(tabs.active(), Some(TabId(3)));
        assert_eq!(tabs.remove(TabId(3)), Some("c"));
        assert_eq!(tabs.active(), None);
        assert!(tabs.is_empty());
    }

    #[test]
    fn closing_a_tab_behind_leaves_the_front_one() {
        let mut tabs = list(&["a", "b", "c"]);
        tabs.remove(TabId(1));
        assert_eq!(tabs.active(), Some(TabId(3)));
        assert_eq!(tabs.remove(TabId(9)), None);
    }

    #[test]
    fn ids_are_not_reused() {
        let mut tabs = list(&["a"]);
        tabs.remove(TabId(1));
        assert_eq!(tabs.push("b"), TabId(2));
    }
}
