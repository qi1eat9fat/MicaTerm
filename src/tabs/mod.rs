//! 标签：标签列表的纯逻辑（`model`）、终端标签（`TerminalTab`）和标签轨道（`tab_rail`）。
//!
//! 02 的终端标签里只有一张卡片；工单 05 把 `TerminalTab` 换成一棵卡片分屏树，
//! 工单 04 补全标签的切换、关闭和标签轨道的样式。

mod model;
mod tab_rail;

pub use model::{TabId, TabList};
pub use tab_rail::{TabSnapshot, render_tabs};

use gpui_kit::{
    AnyElement, App, Entity, IntoElement as _, ParentElement as _, Styled as _, Subscription, div,
};

use crate::card::{CardId, TerminalCard};

/// 一个终端标签。
pub struct TerminalTab {
    /// 02：一个标签一张卡片。工单 05 换成分屏树。
    card: Entity<TerminalCard>,
    /// 窗口壳对卡片的订阅（卡片的名称、目录变了，标签重画），跟着标签走。
    _subscriptions: Vec<Subscription>,
}

impl TerminalTab {
    pub fn new(card: Entity<TerminalCard>, subscriptions: Vec<Subscription>) -> Self {
        Self {
            card,
            _subscriptions: subscriptions,
        }
    }

    /// 标签里的全部卡片。
    pub fn cards(&self) -> impl Iterator<Item = &Entity<TerminalCard>> {
        std::iter::once(&self.card)
    }

    /// 标签跟着它的焦点卡片：名称、目录、图块都取自它。
    pub fn focused_card(&self) -> &Entity<TerminalCard> {
        &self.card
    }

    pub fn contains(&self, card: CardId, cx: &App) -> bool {
        self.cards().any(|entity| entity.read(cx).id() == card)
    }

    /// 标签区里画的内容。
    pub fn content(&self) -> AnyElement {
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(self.card.clone())
            .into_any_element()
    }
}
