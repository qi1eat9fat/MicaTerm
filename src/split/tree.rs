//! 卡片分屏树的纯逻辑：二叉树，叶子是卡片，分支是一条分隔线两边的两棵子树。不依赖 gpui。
//! 参考 gorex 的模型重写（`state.go` 的 `Node`、`split`）。
//!
//! 树是标签布局的唯一事实来源：比例、焦点、放大都记在这里，渲染只是照着画。

use crate::card::CardId;

/// 一条分隔线（树的一个分支）的 id：每棵树从 1 起递增，不复用。元素 id 写成 `("divider", id)`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u64);

/// 分支里两边怎么排。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// 左右并排（向右分屏）。
    Horizontal,
    /// 上下叠放（向下分屏）。
    Vertical,
}

/// 焦点往哪边移、分隔线往哪边推。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// 往这个方向推的是哪个方向上的分隔线：左右推竖线（左右并排的分支），上下推横线。
    pub fn axis(self) -> Axis {
        match self {
            Direction::Left | Direction::Right => Axis::Horizontal,
            Direction::Up | Direction::Down => Axis::Vertical,
        }
    }
}

/// Alt+Shift+方向键一次移动分隔线多少比例。
pub const RATIO_STEP: f32 = 0.05;

/// 新分支的比例：两边各一半。双击间隙也恢复成它。
pub const DEFAULT_RATIO: f32 = 0.5;
/// 比例的下限和上限：哪一边都不会被挤没。
pub const MIN_RATIO: f32 = 0.08;
pub const MAX_RATIO: f32 = 0.92;

/// 比例限制在 `MIN_RATIO`–`MAX_RATIO`。
pub fn clamp_ratio(ratio: f32) -> f32 {
    if ratio.is_nan() {
        return DEFAULT_RATIO;
    }
    ratio.clamp(MIN_RATIO, MAX_RATIO)
}

/// 关一张卡片的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closed {
    /// 树里没有这张卡片。
    Missing,
    /// 它是最后一张：树不动，由标签关掉整个标签。
    LastCard,
    /// 摘掉了，它的兄弟占据了父节点的位置。
    Merged,
}

/// 树的一个节点。
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Card(CardId),
    Split {
        id: NodeId,
        axis: Axis,
        /// `first`（左边或上边）占的份额。
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

impl Node {
    fn collect_cards(&self, cards: &mut Vec<CardId>) {
        match self {
            Node::Card(card) => cards.push(*card),
            Node::Split { first, second, .. } => {
                first.collect_cards(cards);
                second.collect_cards(cards);
            }
        }
    }

    /// 去掉 `card` 的叶子：它的兄弟占据父节点的位置。`self` 本身就是这张卡片时不动，返回 `false`。
    fn remove(&mut self, card: CardId) -> bool {
        let Node::Split { first, second, .. } = self else {
            return false;
        };
        let survivor = if **first == Node::Card(card) {
            second
        } else if **second == Node::Card(card) {
            first
        } else {
            return first.remove(card) || second.remove(card);
        };
        let survivor = std::mem::replace(survivor.as_mut(), Node::Card(card));
        *self = survivor;
        true
    }

    /// id 为 `node` 的分支的比例。
    fn ratio_mut(&mut self, node: NodeId) -> Option<&mut f32> {
        match self {
            Node::Card(_) => None,
            Node::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                if *id == node {
                    Some(ratio)
                } else {
                    first
                        .ratio_mut(node)
                        .or_else(move || second.ratio_mut(node))
                }
            }
        }
    }

    fn ratio_of(&self, node: NodeId) -> Option<f32> {
        match self {
            Node::Card(_) => None,
            Node::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                if *id == node {
                    Some(*ratio)
                } else {
                    first.ratio_of(node).or_else(|| second.ratio_of(node))
                }
            }
        }
    }

    /// 从这里到 `card` 那片叶子，一路经过的分支（从上往下）。
    fn path_to(&self, card: CardId, path: &mut Vec<(NodeId, Axis)>) -> bool {
        match self {
            Node::Card(id) => *id == card,
            Node::Split {
                id,
                axis,
                first,
                second,
                ..
            } => {
                path.push((*id, *axis));
                if first.path_to(card, path) || second.path_to(card, path) {
                    return true;
                }
                path.pop();
                false
            }
        }
    }

    /// 每张卡片在 `area` 里按比例分到的那一块。
    fn layout(&self, area: Rect, cards: &mut Vec<(CardId, Rect)>) {
        match self {
            Node::Card(card) => cards.push((*card, area)),
            Node::Split {
                axis,
                ratio,
                first,
                second,
                ..
            } => {
                let (a, b) = area.split(*axis, *ratio);
                first.layout(a, cards);
                second.layout(b, cards);
            }
        }
    }

    /// 找到 `card` 的叶子。
    fn leaf_mut(&mut self, card: CardId) -> Option<&mut Node> {
        match self {
            Node::Card(id) if *id == card => Some(self),
            Node::Card(_) => None,
            Node::Split { first, second, .. } => {
                first.leaf_mut(card).or_else(move || second.leaf_mut(card))
            }
        }
    }
}

/// 标签里的一块地方，整个标签是 0..1 见方：只用来比较卡片的相对位置。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Rect {
    const WHOLE: Rect = Rect {
        x: 0.,
        y: 0.,
        width: 1.,
        height: 1.,
    };

    fn split(self, axis: Axis, ratio: f32) -> (Rect, Rect) {
        match axis {
            Axis::Horizontal => {
                let first = self.width * ratio;
                (
                    Rect {
                        width: first,
                        ..self
                    },
                    Rect {
                        x: self.x + first,
                        width: self.width - first,
                        ..self
                    },
                )
            }
            Axis::Vertical => {
                let first = self.height * ratio;
                (
                    Rect {
                        height: first,
                        ..self
                    },
                    Rect {
                        y: self.y + first,
                        height: self.height - first,
                        ..self
                    },
                )
            }
        }
    }

    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }

    fn center(self) -> (f32, f32) {
        (self.x + self.width / 2., self.y + self.height / 2.)
    }
}

/// 一个终端标签里的卡片分屏树，加上焦点卡片。
#[derive(Clone, Debug)]
pub struct SplitTree {
    root: Node,
    focused: CardId,
    /// 按最近一次聚焦排的卡片，最近的在最后：关掉焦点卡片时焦点交还给前一张。
    recent: Vec<CardId>,
    /// 放大的那张：只画它，树不动。
    zoomed: Option<CardId>,
    next_node: u64,
}

impl SplitTree {
    /// 只有一张卡片的树，焦点在它上面。
    pub fn new(card: CardId) -> Self {
        Self {
            root: Node::Card(card),
            focused: card,
            recent: vec![card],
            zoomed: None,
            next_node: 1,
        }
    }

    /// 树根：渲染从这里往下画。
    pub fn root(&self) -> &Node {
        &self.root
    }

    /// 全部卡片，从左到右、从上到下。
    pub fn cards(&self) -> Vec<CardId> {
        let mut cards = Vec::new();
        self.root.collect_cards(&mut cards);
        cards
    }

    /// 焦点卡片：标签跟着它。
    pub fn focused(&self) -> CardId {
        self.focused
    }

    /// 把焦点卡片分成两半：`card` 放在它的右边（`Horizontal`）或下边（`Vertical`），
    /// 比例 0.5，焦点给 `card`。返回新分隔线的 id。
    pub fn split(&mut self, axis: Axis, card: CardId) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node += 1;
        let old = self.focused;
        let leaf = self
            .root
            .leaf_mut(old)
            .expect("the focused card is in the tree");
        *leaf = Node::Split {
            id,
            axis,
            ratio: DEFAULT_RATIO,
            first: Box::new(Node::Card(old)),
            second: Box::new(Node::Card(card)),
        };
        self.set_focused(card);
        self.zoomed = None;
        id
    }

    /// 放大的卡片：标签里只画它。
    pub fn zoomed(&self) -> Option<CardId> {
        self.zoomed
    }

    /// 放大焦点卡片，或者还原。只有一张卡片时不放大，返回 `false`。
    pub fn toggle_zoom(&mut self) -> bool {
        if self.zoomed.is_some() {
            self.zoomed = None;
        } else if matches!(self.root, Node::Split { .. }) {
            self.zoomed = Some(self.focused);
        } else {
            return false;
        }
        true
    }

    /// 树里有没有 `card`。
    pub fn contains(&self, card: CardId) -> bool {
        self.cards().contains(&card)
    }

    /// 把焦点给 `card`（点了它、Alt+方向键移过去）。树里没有它时不动，返回 `false`。
    pub fn focus(&mut self, card: CardId) -> bool {
        if !self.contains(card) {
            return false;
        }
        self.set_focused(card);
        true
    }

    fn set_focused(&mut self, card: CardId) {
        self.focused = card;
        self.recent.retain(|recent| *recent != card);
        self.recent.push(card);
    }

    /// 摘掉 `card`：它的兄弟占据父节点的位置。关的是焦点卡片时，焦点交还给之前最近聚焦过的那张。
    pub fn close(&mut self, card: CardId) -> Closed {
        if self.root == Node::Card(card) {
            return Closed::LastCard;
        }
        if !self.root.remove(card) {
            return Closed::Missing;
        }
        self.recent.retain(|recent| *recent != card);
        if self.zoomed == Some(card) {
            self.zoomed = None;
        }
        if self.focused == card {
            let next = self
                .recent
                .last()
                .copied()
                .unwrap_or_else(|| self.cards()[0]);
            self.set_focused(next);
        }
        Closed::Merged
    }

    /// 分隔线 `node` 现在的比例：它左边或上边那一侧占的份额。
    pub fn ratio(&self, node: NodeId) -> Option<f32> {
        self.root.ratio_of(node)
    }

    /// 把分隔线 `node` 的比例设成 `ratio`（拖动间隙），限制在 0.08–0.92。没有这条分隔线时返回 `false`。
    pub fn set_ratio(&mut self, node: NodeId, ratio: f32) -> bool {
        match self.root.ratio_mut(node) {
            Some(slot) => {
                *slot = clamp_ratio(ratio);
                true
            }
            None => false,
        }
    }

    /// 双击间隙：分隔线 `node` 恢复成一半。
    pub fn reset(&mut self, node: NodeId) -> bool {
        self.set_ratio(node, DEFAULT_RATIO)
    }

    /// Alt+方向键：焦点移到 `direction` 那边紧挨着的卡片，几张都挨着时挑和焦点卡片中心最对齐的。
    /// 那边没有卡片、或者正放大着，焦点不动，返回 `None`。
    pub fn move_focus(&mut self, direction: Direction) -> Option<CardId> {
        if self.zoomed.is_some() {
            return None;
        }
        let mut cards = Vec::new();
        self.root.layout(Rect::WHOLE, &mut cards);
        let (_, from) = *cards.iter().find(|(card, _)| *card == self.focused)?;
        let (center_x, center_y) = from.center();
        let mut best: Option<(f32, CardId)> = None;
        for (card, rect) in cards.iter().filter(|(card, _)| *card != self.focused) {
            // 离焦点卡片那条边多远，两张在另一个方向上重叠多少。
            let (distance, overlap) = match direction {
                Direction::Right => (rect.x - from.right(), overlap_y(rect, &from)),
                Direction::Left => (from.x - rect.right(), overlap_y(rect, &from)),
                Direction::Down => (rect.y - from.bottom(), overlap_x(rect, &from)),
                Direction::Up => (from.y - rect.bottom(), overlap_x(rect, &from)),
            };
            if distance < -EPSILON || overlap <= EPSILON {
                continue;
            }
            // 最近的优先，其次是和焦点卡片中心最对齐的（照 gorex 的打分）。
            let (x, y) = rect.center();
            let offset = match direction.axis() {
                Axis::Horizontal => y - center_y,
                Axis::Vertical => x - center_x,
            };
            let score = distance * 4. + offset.abs();
            if best.is_none_or(|(best, _)| score < best - EPSILON) {
                best = Some((score, *card));
            }
        }
        let (_, card) = best?;
        self.set_focused(card);
        Some(card)
    }

    /// Alt+Shift+方向键：焦点卡片往上找第一条同方向的分隔线，往 `direction` 推 0.05（限制在 0.08–0.92）。
    /// 返回推的是哪一条；没有同方向的分隔线时返回 `None`。
    pub fn move_divider(&mut self, direction: Direction) -> Option<NodeId> {
        let mut path = Vec::new();
        self.root.path_to(self.focused, &mut path);
        let (node, _) = path
            .into_iter()
            .rev()
            .find(|(_, axis)| *axis == direction.axis())?;
        let step = match direction {
            Direction::Right | Direction::Down => RATIO_STEP,
            Direction::Left | Direction::Up => -RATIO_STEP,
        };
        let ratio = self.ratio(node)?;
        self.set_ratio(node, ratio + step);
        Some(node)
    }
}

/// 比较位置时容许的误差：并排的两张卡片边挨着边，浮点数加减以后不一定正好相等。
const EPSILON: f32 = 1e-4;

fn overlap_x(a: &Rect, b: &Rect) -> f32 {
    a.right().min(b.right()) - a.x.max(b.x)
}

fn overlap_y(a: &Rect, b: &Rect) -> f32 {
    a.bottom().min(b.bottom()) - a.y.max(b.y)
}

#[cfg(test)]
mod tests {
    use super::{Axis, Closed, DEFAULT_RATIO, Direction, Node, NodeId, SplitTree};
    use crate::card::CardId;

    const A: CardId = CardId(1);
    const B: CardId = CardId(2);
    const C: CardId = CardId(3);

    fn card(id: CardId) -> Box<Node> {
        Box::new(Node::Card(id))
    }

    #[test]
    fn a_new_tree_is_one_focused_card() {
        let tree = SplitTree::new(A);

        assert_eq!(tree.cards(), [A]);
        assert_eq!(tree.focused(), A);
    }

    #[test]
    fn splitting_right_puts_the_new_card_right_of_the_focused_one_and_focuses_it() {
        let mut tree = SplitTree::new(A);

        let divider = tree.split(Axis::Horizontal, B);

        assert_eq!(divider, NodeId(1));
        assert_eq!(tree.cards(), [A, B]);
        assert_eq!(tree.focused(), B);
        assert_eq!(
            tree.root(),
            &Node::Split {
                id: NodeId(1),
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: card(A),
                second: card(B),
            }
        );
    }

    /// 向下分屏只分焦点卡片那一格，旁边的卡片不动。
    #[test]
    fn splitting_down_splits_only_the_focused_card() {
        let mut tree = SplitTree::new(A);
        tree.split(Axis::Horizontal, B);

        let divider = tree.split(Axis::Vertical, C);

        assert_eq!(divider, NodeId(2));
        assert_eq!(tree.cards(), [A, B, C]);
        assert_eq!(tree.focused(), C);
        assert_eq!(
            tree.root(),
            &Node::Split {
                id: NodeId(1),
                axis: Axis::Horizontal,
                ratio: DEFAULT_RATIO,
                first: card(A),
                second: Box::new(Node::Split {
                    id: NodeId(2),
                    axis: Axis::Vertical,
                    ratio: DEFAULT_RATIO,
                    first: card(B),
                    second: card(C),
                }),
            }
        );
    }

    /// 关掉一张卡片，它的兄弟占据父节点的位置，兄弟子树里的比例不变。
    #[test]
    fn closing_a_card_lets_its_sibling_take_the_place_of_their_split() {
        let mut tree = SplitTree::new(A);
        tree.split(Axis::Horizontal, B);
        tree.split(Axis::Vertical, C);
        tree.set_ratio(NodeId(2), 0.3);

        assert_eq!(tree.close(A), Closed::Merged);

        assert_eq!(tree.cards(), [B, C]);
        assert_eq!(
            tree.root(),
            &Node::Split {
                id: NodeId(2),
                axis: Axis::Vertical,
                ratio: 0.3,
                first: card(B),
                second: card(C),
            }
        );
    }

    /// 关掉焦点卡片，焦点交还给之前最近聚焦过的那张。
    #[test]
    fn closing_the_focused_card_gives_the_focus_back_to_the_one_before() {
        let mut tree = SplitTree::new(A);
        tree.split(Axis::Horizontal, B);
        tree.split(Axis::Vertical, C);
        tree.focus(A);
        tree.focus(C);

        tree.close(C);

        assert_eq!(tree.focused(), A);
        assert_eq!(tree.cards(), [A, B]);
    }

    /// 关掉的不是焦点卡片，焦点不动。
    #[test]
    fn closing_another_card_keeps_the_focus() {
        let mut tree = SplitTree::new(A);
        tree.split(Axis::Horizontal, B);

        tree.close(A);

        assert_eq!(tree.focused(), B);
        assert_eq!(tree.root(), &Node::Card(B));
    }

    /// 最后一张卡片不从树里摘：由标签关掉整个标签。
    #[test]
    fn closing_the_last_card_says_so_and_leaves_the_tree_alone() {
        let mut tree = SplitTree::new(A);

        assert_eq!(tree.close(A), Closed::LastCard);
        assert_eq!(tree.close(B), Closed::Missing);

        assert_eq!(tree.cards(), [A]);
    }

    /// 拖动间隙设的比例限制在 0.08–0.92。
    #[test]
    fn a_ratio_is_kept_between_eight_and_ninety_two_percent() {
        let mut tree = SplitTree::new(A);
        let divider = tree.split(Axis::Horizontal, B);

        tree.set_ratio(divider, 0.02);
        assert_eq!(tree.ratio(divider), Some(0.08));
        tree.set_ratio(divider, 0.99);
        assert_eq!(tree.ratio(divider), Some(0.92));
        tree.set_ratio(divider, 0.3);
        assert_eq!(tree.ratio(divider), Some(0.3));

        assert!(!tree.set_ratio(NodeId(9), 0.3));
        assert_eq!(tree.ratio(NodeId(9)), None);
    }

    /// 双击间隙恢复成一半。
    #[test]
    fn resetting_a_divider_gives_both_sides_half() {
        let mut tree = SplitTree::new(A);
        let divider = tree.split(Axis::Vertical, B);
        tree.set_ratio(divider, 0.8);

        assert!(tree.reset(divider));

        assert_eq!(tree.ratio(divider), Some(0.5));
        assert!(!tree.reset(NodeId(9)));
    }

    /// 放大和还原只换显示的那张，不改树的结构。
    #[test]
    fn zooming_toggles_the_focused_card_without_changing_the_tree() {
        let mut tree = SplitTree::new(A);
        tree.split(Axis::Horizontal, B);
        let before = tree.root().clone();

        assert!(tree.toggle_zoom());
        assert_eq!(tree.zoomed(), Some(B));
        assert_eq!(tree.root(), &before);

        assert!(tree.toggle_zoom());
        assert_eq!(tree.zoomed(), None);
        assert_eq!(tree.root(), &before);
    }

    /// 只有一张卡片时没什么可放大的。
    #[test]
    fn a_lone_card_does_not_zoom() {
        let mut tree = SplitTree::new(A);

        assert!(!tree.toggle_zoom());

        assert_eq!(tree.zoomed(), None);
    }

    /// 分屏或关掉放大的那张，都回到平铺。
    #[test]
    fn splitting_or_closing_the_zoomed_card_ends_the_zoom() {
        let mut tree = SplitTree::new(A);
        tree.split(Axis::Horizontal, B);
        tree.toggle_zoom();

        tree.split(Axis::Vertical, C);
        assert_eq!(tree.zoomed(), None);

        tree.toggle_zoom();
        assert_eq!(tree.zoomed(), Some(C));
        tree.close(C);
        assert_eq!(tree.zoomed(), None);
    }

    /// 关掉别的卡片，放大的那张还放大着。
    #[test]
    fn closing_another_card_keeps_the_zoom() {
        let mut tree = SplitTree::new(A);
        tree.split(Axis::Horizontal, B);
        tree.split(Axis::Vertical, C);
        tree.toggle_zoom();

        tree.close(A);

        assert_eq!(tree.zoomed(), Some(C));
    }

    /// A 在左，右边 B 在上、C 在下。
    fn a_beside_b_over_c() -> SplitTree {
        let mut tree = SplitTree::new(A);
        tree.split(Axis::Horizontal, B);
        tree.split(Axis::Vertical, C);
        tree
    }

    #[test]
    fn the_focus_moves_to_the_card_next_to_it() {
        let mut tree = a_beside_b_over_c();

        assert_eq!(tree.move_focus(Direction::Up), Some(B));
        assert_eq!(tree.focused(), B);
        assert_eq!(tree.move_focus(Direction::Left), Some(A));
        assert_eq!(tree.focused(), A);
    }

    /// 那一边有几张卡片时，挑和焦点卡片中心最对齐的那张。
    #[test]
    fn the_focus_moves_to_the_card_most_in_line() {
        let mut tree = a_beside_b_over_c();
        tree.set_ratio(NodeId(2), 0.3);
        tree.focus(A);

        assert_eq!(tree.move_focus(Direction::Right), Some(C));
    }

    /// 那一边没有卡片，或者正放大着，焦点不动。
    #[test]
    fn the_focus_stays_where_there_is_no_card_or_while_zoomed() {
        let mut tree = a_beside_b_over_c();
        tree.focus(B);

        assert_eq!(tree.move_focus(Direction::Up), None);
        assert_eq!(tree.move_focus(Direction::Right), None);
        assert_eq!(tree.focused(), B);

        tree.toggle_zoom();
        assert_eq!(tree.move_focus(Direction::Down), None);
        assert_eq!(tree.focused(), B);
    }

    /// Alt+Shift+方向键移动焦点卡片最近的那条同方向的分隔线，一次 0.05。
    #[test]
    fn moving_a_divider_moves_the_nearest_one_on_that_axis() {
        let mut tree = a_beside_b_over_c();

        assert_eq!(tree.move_divider(Direction::Up), Some(NodeId(2)));
        assert_close(tree.ratio(NodeId(2)), 0.45);
        assert_eq!(tree.move_divider(Direction::Right), Some(NodeId(1)));
        assert_close(tree.ratio(NodeId(1)), 0.55);
        assert_eq!(tree.move_divider(Direction::Left), Some(NodeId(1)));
        assert_close(tree.ratio(NodeId(1)), 0.5);
    }

    #[test]
    fn a_moved_divider_stops_at_the_limit() {
        let mut tree = a_beside_b_over_c();

        for _ in 0..20 {
            tree.move_divider(Direction::Down);
        }

        assert_eq!(tree.ratio(NodeId(2)), Some(0.92));
    }

    /// 焦点卡片上方没有横着的分隔线：什么都不动。
    #[test]
    fn no_divider_moves_when_there_is_none_on_that_axis() {
        let mut tree = a_beside_b_over_c();
        tree.focus(A);

        assert_eq!(tree.move_divider(Direction::Down), None);

        assert_eq!(tree.ratio(NodeId(2)), Some(0.5));
        assert_eq!(SplitTree::new(A).move_divider(Direction::Left), None);
    }

    fn assert_close(actual: Option<f32>, expected: f32) {
        let actual = actual.expect("the divider is in the tree");
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }
}
