//! 标签列表的纯逻辑：按顺序放标签，记着哪个在前面。不依赖 gpui。
//! 新建、关闭（02），上一个 / 下一个 / 第 n 个、关闭其他、关闭右侧（04），拖动排序（29）。

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

    /// 改 `id` 的内容（工单 05：分屏、关卡片、移动焦点都改终端标签里的分屏树）。
    pub fn get_mut(&mut self, id: TabId) -> Option<&mut T> {
        self.tabs
            .iter_mut()
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

    /// 把在前的标签往右（`step` 为正）或往左挪 `step` 个，过了两头绕回来；返回新的在前标签。
    pub fn step(&mut self, step: isize) -> Option<TabId> {
        let count = self.tabs.len() as isize;
        let current = self.active.and_then(|id| self.index_of(id))? as isize;
        let next = (current + step).rem_euclid(count) as usize;
        let id = self.tabs[next].0;
        self.active = Some(id);
        Some(id)
    }

    /// 第 `position` 个标签，从 1 数起；9 表示最后一个（同浏览器）。超出的位置没有标签。
    pub fn nth(&self, position: usize) -> Option<TabId> {
        let index = if position >= 9 {
            self.tabs.len().checked_sub(1)?
        } else {
            position.checked_sub(1)?
        };
        self.tabs.get(index).map(|(id, _)| *id)
    }

    /// `id` 以外的标签，从左到右；没有 `id` 这个标签时为空。
    pub fn others(&self, id: TabId) -> Vec<TabId> {
        if self.index_of(id).is_none() {
            return Vec::new();
        }
        self.tabs
            .iter()
            .map(|(tab, _)| *tab)
            .filter(|tab| *tab != id)
            .collect()
    }

    /// `id` 右边的标签，从左到右。
    pub fn right_of(&self, id: TabId) -> Vec<TabId> {
        match self.index_of(id) {
            Some(index) => self.tabs[index + 1..].iter().map(|(tab, _)| *tab).collect(),
            None => Vec::new(),
        }
    }

    /// 把 `id` 挪到第 `index` 个（从 0 数起，超出的放到最后），在前的标签不变（工单 29 拖动排序）。
    /// 顺序变了返回 `true`；没有这个标签、或它本来就在那里时返回 `false`。
    pub fn move_to(&mut self, id: TabId, index: usize) -> bool {
        let Some(from) = self.index_of(id) else {
            return false;
        };
        let to = index.min(self.tabs.len() - 1);
        if from == to {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        true
    }

    /// `id` 在第几个（从 0 数起）。
    pub fn position(&self, id: TabId) -> Option<usize> {
        self.index_of(id)
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

    #[test]
    fn stepping_goes_round_the_ends() {
        let mut tabs = list(&["a", "b", "c"]);
        assert_eq!(tabs.step(1), Some(TabId(1)));
        assert_eq!(tabs.active(), Some(TabId(1)));
        assert_eq!(tabs.step(-1), Some(TabId(3)));
        assert_eq!(tabs.step(-1), Some(TabId(2)));
        assert_eq!(tabs.step(1), Some(TabId(3)));
    }

    #[test]
    fn stepping_without_tabs_does_nothing() {
        let mut tabs = list(&[]);
        assert_eq!(tabs.step(1), None);
        assert_eq!(tabs.active(), None);
    }

    /// 从 1 数起；9 是最后一个，超出的位置没有标签。
    #[test]
    fn the_nth_tab_counts_from_one_and_nine_is_the_last() {
        let tabs = list(&["a", "b", "c"]);
        assert_eq!(tabs.nth(1), Some(TabId(1)));
        assert_eq!(tabs.nth(3), Some(TabId(3)));
        assert_eq!(tabs.nth(4), None);
        assert_eq!(tabs.nth(0), None);
        assert_eq!(tabs.nth(9), Some(TabId(3)));
        let many = list(&["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"]);
        assert_eq!(many.nth(8), Some(TabId(8)));
        assert_eq!(many.nth(9), Some(TabId(11)));
        assert_eq!(list(&[]).nth(9), None);
    }

    #[test]
    fn the_others_and_the_ones_to_the_right() {
        let tabs = list(&["a", "b", "c", "d"]);
        assert_eq!(tabs.others(TabId(2)), [TabId(1), TabId(3), TabId(4)]);
        assert_eq!(tabs.right_of(TabId(2)), [TabId(3), TabId(4)]);
        assert!(tabs.right_of(TabId(4)).is_empty());
        assert!(tabs.right_of(TabId(9)).is_empty());
        assert!(tabs.others(TabId(9)).is_empty());
    }

    /// 拖动排序（工单 29）：挪到第 `index` 个（从 0 数起），超出的放到最后；在前的标签不变。
    #[test]
    fn moving_a_tab_puts_it_at_the_index_and_keeps_the_front_one() {
        let mut tabs = list(&["a", "b", "c", "d"]);
        tabs.activate(TabId(2));

        assert!(tabs.move_to(TabId(4), 0));
        assert_eq!(tabs.ids(), [TabId(4), TabId(1), TabId(2), TabId(3)]);
        assert!(tabs.move_to(TabId(4), 2));
        assert_eq!(tabs.ids(), [TabId(1), TabId(2), TabId(4), TabId(3)]);
        assert!(tabs.move_to(TabId(1), 9));
        assert_eq!(tabs.ids(), [TabId(2), TabId(4), TabId(3), TabId(1)]);
        assert_eq!(tabs.active(), Some(TabId(2)));
        assert_eq!(tabs.get(TabId(1)), Some(&"a"));
    }

    #[test]
    fn moving_a_tab_to_where_it_is_or_a_tab_that_is_gone_changes_nothing() {
        let mut tabs = list(&["a", "b", "c"]);
        assert!(!tabs.move_to(TabId(2), 1));
        assert!(!tabs.move_to(TabId(3), 7));
        assert!(!tabs.move_to(TabId(9), 0));
        assert_eq!(tabs.ids(), [TabId(1), TabId(2), TabId(3)]);
    }

    #[test]
    fn the_position_of_a_tab() {
        let tabs = list(&["a", "b"]);
        assert_eq!(tabs.position(TabId(2)), Some(1));
        assert_eq!(tabs.position(TabId(9)), None);
    }
}
