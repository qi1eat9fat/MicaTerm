//! 标签：标签列表的纯逻辑（`model`）、终端标签（`TerminalTab`）和标签轨道（`tab_rail`）。
//!
//! 终端标签里是一棵卡片分屏树（工单 05，树和它的渲染在 `crate::split`）。
//! 分屏时先在树里放一张占位的新卡片（`CardPicker`，2026-11-14 分屏重做），选好了再换成真卡片。
//! 工单 04 补全了标签的切换、关闭（`model`）和标签轨道（`TabRail`）。

mod model;
mod tab;
mod tab_rail;

pub use model::{TabDrop, TabId, TabList};
pub use tab::Tab;
pub use tab_rail::{TabLeading, TabRail, TabSnapshot, build_tab_menu};

use std::collections::HashMap;

use gpui_kit::{
    AnyElement, App, Entity, IntoElement as _, ParentElement as _, Styled as _, Subscription,
    Window, div,
};

use micaterm_terminal::TerminalView;

use crate::card::{CardId, CardPicker, StatusDot, TerminalCard};
use crate::split::{Axis, Closed, Direction, DividerMirrors, NodeId, SplitTree};

/// 一个终端标签：一棵卡片分屏树。
pub struct TerminalTab {
    /// 布局的唯一事实来源：卡片怎么分、比例、焦点、放大。
    tree: SplitTree,
    /// 树里每张卡片的实体，和窗口壳对它的订阅（名称、目录变了标签重画，聚焦了记进树），跟着卡片走。
    cards: HashMap<CardId, (Entity<TerminalCard>, Vec<Subscription>)>,
    /// 每条分隔线在 gpui 里的镜像（resizable 组件的状态）。
    dividers: DividerMirrors,
    /// 刚分出来、还在选开什么的新卡片：它在树里占着位置，但不是卡片（不在 `cards` 里）；
    /// 树的焦点留在分屏的那张卡片上，标签名、终端动作都还跟着它。
    picker: Option<Box<PickerSlot>>,
    /// 新卡片选好了、真卡片还没放进来时，它那一格：下一次 `split` 放进这一格（分隔线、比例不动）。
    reserved: Option<CardId>,
}

/// 占位的新卡片和窗口壳对它的订阅。
pub struct PickerSlot {
    pub view: Entity<CardPicker>,
    pub _subscription: Subscription,
}

impl TerminalTab {
    /// 只有一张卡片的标签。`subscriptions` 是窗口壳对这张卡片的订阅。
    pub fn new(card: Entity<TerminalCard>, subscriptions: Vec<Subscription>, cx: &App) -> Self {
        let id = card.read(cx).id();
        Self {
            tree: SplitTree::new(id),
            cards: HashMap::from([(id, (card, subscriptions))]),
            dividers: DividerMirrors::default(),
            picker: None,
            reserved: None,
        }
    }

    /// 标签里的全部卡片，从左到右、从上到下。
    pub fn cards(&self) -> impl Iterator<Item = &Entity<TerminalCard>> {
        self.tree.cards().into_iter().filter_map(|id| self.card(id))
    }

    /// 标签跟着它的焦点卡片：名称、目录、图块都取自它。
    pub fn focused_card(&self) -> &Entity<TerminalCard> {
        self.card(self.tree.focused())
            .expect("the focused card belongs to the tab")
    }

    /// 焦点卡片里的终端：终端动作（复制、粘贴、查找……）的对象。
    pub fn focused_terminal(&self, cx: &App) -> Entity<TerminalView> {
        self.focused_card().read(cx).terminal().clone()
    }

    pub fn contains(&self, card: CardId, _cx: &App) -> bool {
        self.cards.contains_key(&card)
    }

    /// 标签在前时 `card` 画不画出来：放大着时只画放大的那张，其余被藏起来了。
    pub fn shows(&self, card: CardId) -> bool {
        self.cards.contains_key(&card) && self.tree.zoomed().is_none_or(|zoomed| zoomed == card)
    }

    /// 标签上的状态点：有卡片要注意是橙点，否则有卡片在输出是绿点。
    pub fn dot(&self, cx: &App) -> StatusDot {
        StatusDot::of_tab(self.cards().map(|card| card.read(cx).status_dot(cx)))
    }

    /// id 为 `card` 的卡片。
    pub fn card(&self, card: CardId) -> Option<&Entity<TerminalCard>> {
        self.cards.get(&card).map(|(entity, _)| entity)
    }

    /// 分屏树：只读，改它一律经下面的方法（同时更新卡片和分隔线的镜像）。
    pub fn tree(&self) -> &SplitTree {
        &self.tree
    }

    /// 在焦点卡片的右边或下边放一张占位的新卡片 `picker`（分屏重做）。已经有一张了先撤掉它。
    /// 树的焦点留在焦点卡片上；搜索框的焦点由调用方给。
    pub fn open_picker(
        &mut self,
        axis: Axis,
        picker: Entity<CardPicker>,
        subscription: Subscription,
        cx: &mut App,
    ) {
        self.dismiss_picker(cx);
        let source = self.tree.focused();
        let id = picker.read(cx).id();
        self.tree.split(axis, id);
        self.tree.focus(source);
        self.picker = Some(Box::new(PickerSlot {
            view: picker,
            _subscription: subscription,
        }));
        self.dividers.sync(&self.tree, cx);
        self.sync_cards(cx);
    }

    /// 占位的新卡片（开着时）。
    pub fn picker(&self) -> Option<&Entity<CardPicker>> {
        self.picker.as_ref().map(|slot| &slot.view)
    }

    /// 撤掉占位的新卡片 `id`，交回它（它不是现在这张就是 `None`）。焦点卡片不变。
    pub fn take_picker(&mut self, id: CardId, cx: &mut App) -> Option<Box<PickerSlot>> {
        let current = self.picker.as_ref()?.view.read(cx).id();
        if current != id {
            return None;
        }
        let slot = self.picker.take()?;
        self.tree.close(id);
        self.dividers.sync(&self.tree, cx);
        self.sync_cards(cx);
        Some(slot)
    }

    /// 新卡片 `id` 选好了：交回它，它那一格留给接下来 `split` 进来的真卡片。它不是现在这张就是 `None`。
    pub fn choose_picker(&mut self, id: CardId, cx: &mut App) -> Option<Box<PickerSlot>> {
        let current = self.picker.as_ref()?.view.read(cx).id();
        if current != id {
            return None;
        }
        self.reserved = Some(id);
        self.picker.take()
    }

    /// 留着的那一格没等到真卡片（主机刚被删了）：收回它，返回 `true`。
    pub fn release_reserved(&mut self, cx: &mut App) -> bool {
        let Some(id) = self.reserved.take() else {
            return false;
        };
        self.tree.close(id);
        self.dividers.sync(&self.tree, cx);
        self.sync_cards(cx);
        true
    }

    /// 改树的结构、动焦点之前先撤掉占位的新卡片：它不是卡片，不能成为焦点卡片。
    fn dismiss_picker(&mut self, cx: &mut App) {
        if let Some(id) = self.picker.as_ref().map(|slot| slot.view.read(cx).id()) {
            self.take_picker(id, cx);
        }
    }

    /// 把焦点卡片分成两半，`card` 放在右边或下边，焦点给它。新卡片刚选好时（`choose_picker`），
    /// 直接放进它留着的那一格，分隔线和比例不动。
    pub fn split(
        &mut self,
        axis: Axis,
        card: Entity<TerminalCard>,
        subscriptions: Vec<Subscription>,
        cx: &mut App,
    ) {
        let id = card.read(cx).id();
        self.cards.insert(id, (card, subscriptions));
        match self.reserved.take() {
            Some(leaf) => {
                self.tree.replace(leaf, id);
            }
            None => {
                self.dismiss_picker(cx);
                self.tree.split(axis, id);
            }
        }
        self.dividers.sync(&self.tree, cx);
        self.sync_cards(cx);
    }

    /// 从树里摘掉 `card`，交回它的实体（由调用方关掉它的 shell）。
    /// 它是最后一张时树不动，返回 `Closed::LastCard`：由调用方关掉整个标签。
    pub fn close_card(
        &mut self,
        card: CardId,
        cx: &mut App,
    ) -> (Closed, Option<Entity<TerminalCard>>) {
        self.dismiss_picker(cx);
        let closed = self.tree.close(card);
        if closed != Closed::Merged {
            return (closed, None);
        }
        let entity = self.cards.remove(&card).map(|(entity, _)| entity);
        self.dividers.sync(&self.tree, cx);
        self.sync_cards(cx);
        (closed, entity)
    }

    /// 焦点到了 `card`（点了它、或者焦点移过去）。不在这个标签里返回 `false`。
    pub fn focus_card(&mut self, card: CardId, cx: &mut App) -> bool {
        if self.tree.focused() == card {
            return self.contains(card, cx);
        }
        let focused = self.tree.focus(card);
        if focused {
            self.sync_cards(cx);
        }
        focused
    }

    /// 让 `card` 看得见并成为焦点卡片（点了它的通知）：放大的是别的卡片时先还原。
    /// 不在这个标签里返回 `false`。
    pub fn reveal_card(&mut self, card: CardId, cx: &mut App) -> bool {
        self.dismiss_picker(cx);
        let revealed = self.tree.reveal(card);
        if revealed {
            self.sync_cards(cx);
        }
        revealed
    }

    /// 焦点移到那一边紧挨着的卡片，返回它；那边没有卡片或正放大着返回 `None`。
    pub fn move_focus(
        &mut self,
        direction: Direction,
        cx: &mut App,
    ) -> Option<Entity<TerminalCard>> {
        self.dismiss_picker(cx);
        let card = self.tree.move_focus(direction)?;
        self.sync_cards(cx);
        self.card(card).cloned()
    }

    /// 放大焦点卡片，或者还原。只有一张卡片时什么都不做，返回 `false`。
    pub fn toggle_zoom(&mut self, cx: &mut App) -> bool {
        self.dismiss_picker(cx);
        let toggled = self.tree.toggle_zoom();
        if toggled {
            self.sync_cards(cx);
        }
        toggled
    }

    /// 把焦点卡片最近的那条同方向的分隔线往那一边推 0.05。
    pub fn move_divider(&mut self, direction: Direction, window: &mut Window, cx: &mut App) {
        if let Some(node) = self.tree.move_divider(direction) {
            self.show_ratio(node, window, cx);
        }
    }

    /// 分隔线 `node` 恢复成一半。
    pub fn reset_divider(&mut self, node: NodeId, window: &mut Window, cx: &mut App) {
        if self.tree.reset(node) {
            self.show_ratio(node, window, cx);
        }
    }

    /// 拖完间隙：比例写回树（限制在 0.08–0.92），拖到界外的再推回 resizable 组件。
    pub fn resize_divider(&mut self, node: NodeId, ratio: f32, window: &mut Window, cx: &mut App) {
        if self.tree.set_ratio(node, ratio) {
            self.show_ratio(node, window, cx);
        }
    }

    fn show_ratio(&self, node: NodeId, window: &mut Window, cx: &mut App) {
        if let Some(ratio) = self.tree.ratio(node) {
            self.dividers.show_ratio(node, ratio, window, cx);
        }
    }

    /// 告诉每张卡片它是不是焦点卡片、是不是放大着、是不是标签里唯一的一张：
    /// 顶行的还原、关闭按钮，右键菜单的「移到新标签」跟着变。
    fn sync_cards(&self, cx: &mut App) {
        let focused = self.tree.focused();
        let zoomed = self.tree.zoomed();
        let alone = self.cards.len() == 1;
        for (id, (card, _)) in &self.cards {
            let (focused, zoomed) = (*id == focused, zoomed == Some(*id));
            card.update(cx, |card, cx| card.set_place(focused, zoomed, alone, cx));
        }
    }

    /// 标签区里画的内容：照着分屏树嵌套 resizable 组件。
    pub fn content(&self) -> AnyElement {
        let picker = self.picker.as_ref().map(|slot| &slot.view);
        let card = |id: CardId| match self.card(id) {
            Some(card) => card.clone().into_any_element(),
            None => match picker {
                Some(picker) => picker.clone().into_any_element(),
                None => div().into_any_element(),
            },
        };
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(self.dividers.render(&self.tree, &card))
            .into_any_element()
    }
}
