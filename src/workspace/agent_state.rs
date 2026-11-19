//! Agent 的状态（Agent CLI 工单 09，26 号「Agent 状态」屏）：右侧面板里的 Agent，和前台是 claude、codex
//! 的本地卡片。状态 hook（`micaterm hook`，经外部 CLI 的服务排着，`workspace/cli.rs` 取来）为主，
//! 屏幕规则补上：每半秒看一眼底部几行，屏幕停了一秒才判（`crate::agent::read_screen`）。
//!
//! 状态变了：
//! - 卡片：等你确认时用户看不到它就亮橙点、发通知（窗口不在前走系统通知，点了切到那张卡片）；回合结束
//!   看不到时亮橙点（和「在看不见的地方结束」同一条规则）。
//! - 面板：收起着或窗口不在前时，等你确认发通知（点了展开面板）；图标栏的点在渲染时按状态画
//!   （等你确认、没看过的回合结束橙点，运行中绿点）。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;

use super::Workspace;
use super::right_sidebar::{AgentRail, RailDot, RightPanel};
use crate::agent::{AgentState, HookEvent, ScreenState, read_screen};
use crate::card::CardId;
use crate::i18n::t;

/// 多久看一眼屏幕。
const LOOK_EVERY: Duration = Duration::from_millis(500);
/// 屏幕停了多久才判：还在刷新的屏幕判不准。
const STILL_FOR: Duration = Duration::from_secs(1);
/// 看底部多少行。
const SCREEN_LINES: usize = 30;
/// 面板里那个 Agent 的系统通知的 tag（点了展开面板）。
pub(super) const AGENT_PANEL_TAG: &str = "agent-panel";

/// 谁的状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum AgentSlot {
    Panel,
    Card(CardId),
}

/// 每个 Agent 最后看到的屏幕：内容、什么时候变成这样的、判过没有。
#[derive(Default)]
pub(super) struct AgentScreens(HashMap<AgentSlot, (String, Instant, bool)>);

impl AgentScreens {
    /// 屏幕停够了、还没判过时交回它，判一次。
    fn settle(&mut self, slot: AgentSlot, text: String, now: Instant) -> Option<String> {
        match self.0.get_mut(&slot) {
            Some((last, since, judged)) if *last == text => {
                if !*judged && now.duration_since(*since) >= STILL_FOR {
                    *judged = true;
                    return Some(text);
                }
                None
            }
            _ => {
                self.0.insert(slot, (text, now, false));
                None
            }
        }
    }

    pub(super) fn forget(&mut self, slot: AgentSlot) {
        self.0.remove(&slot);
    }
}

impl Workspace {
    /// 开始定时看 Agent 的屏幕。窗口壳建好时调用一次。
    pub(super) fn watch_agents(&self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(LOOK_EVERY).await;
                if this
                    .update_in(cx, |this, window, cx| this.look_at_agents(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn look_at_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        // 面板里的 Agent。
        let panel = self.agent.as_ref().and_then(|agent| {
            let kind = agent.kind?;
            Some((
                kind,
                agent.terminal.read(cx).read_tail(SCREEN_LINES, cx).text,
            ))
        });
        if let Some((kind, text)) = panel {
            let state = read_screen(kind, &text);
            let judged = if state == Some(ScreenState::Working) {
                // 干活时底栏的秒数一直变，屏幕停不下来：认出现在在干活就马上判，不等它停。
                self.agent_screens.forget(AgentSlot::Panel);
                state
            } else {
                // 其余等屏幕停一秒再判，判的就是同一段文字。
                self.agent_screens
                    .settle(AgentSlot::Panel, text, now)
                    .and(state)
            };
            if let Some(state) = judged
                && let Some(agent) = &mut self.agent
                && agent.tracker.on_screen(Some(state))
            {
                self.agent_changed(AgentSlot::Panel, window, cx);
            }
        }
        // 前台是 claude、codex 的本地卡片；不再是了的回到空闲。
        let cards: Vec<_> = self
            .terminal_tabs()
            .flat_map(|tab| tab.cards())
            .cloned()
            .collect();
        for entity in cards {
            let id = entity.read(cx).id();
            let slot = AgentSlot::Card(id);
            match entity.read(cx).agent_kind() {
                Some(kind) => {
                    let text = entity
                        .read(cx)
                        .terminal()
                        .read(cx)
                        .read_tail(SCREEN_LINES, cx)
                        .text;
                    let state = read_screen(kind, &text);
                    let judged = if state == Some(ScreenState::Working) {
                        self.agent_screens.forget(slot);
                        state
                    } else {
                        self.agent_screens.settle(slot, text, now).and(state)
                    };
                    let Some(state) = judged else {
                        continue;
                    };
                    let changed = entity.update(cx, |card, cx| {
                        card.update_agent(cx, |agent| agent.on_screen(Some(state)))
                    });
                    if changed {
                        self.agent_changed(slot, window, cx);
                    }
                }
                None => {
                    self.agent_screens.forget(slot);
                    if entity.read(cx).agent_tracker().state() != AgentState::Idle {
                        entity.update(cx, |card, cx| card.update_agent(cx, |agent| agent.reset()));
                    }
                }
            }
        }
    }

    /// 状态 hook 报来一件事：`card` 是报它的终端的 `MICATERM_SELF_CARD`。
    pub(super) fn apply_hook(
        &mut self,
        card: &str,
        event: HookEvent,
        message: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(agent) = &mut self.agent
            && agent.self_id.as_ref() == card
        {
            if agent.tracker.on_hook(event, message) {
                self.agent_changed(AgentSlot::Panel, window, cx);
            }
            return;
        }
        let found = self
            .terminal_tabs()
            .flat_map(|tab| tab.cards())
            .find(|entity| {
                entity
                    .read(cx)
                    .self_id()
                    .is_some_and(|id| id.as_ref() == card)
            })
            .cloned();
        if let Some(entity) = found {
            let id = entity.read(cx).id();
            let changed = entity.update(cx, |card, cx| {
                card.update_agent(cx, |agent| agent.on_hook(event, message))
            });
            if changed {
                self.agent_changed(AgentSlot::Card(id), window, cx);
            }
        }
    }

    /// 状态变了：亮点、发通知。
    fn agent_changed(&mut self, slot: AgentSlot, window: &mut Window, cx: &mut Context<Self>) {
        let window_active = window.is_window_active();
        match slot {
            AgentSlot::Panel => {
                let visible = window_active && self.right_panel == Some(RightPanel::Agent);
                let Some(agent) = &mut self.agent else {
                    return;
                };
                agent.turn_seen = visible;
                if agent.tracker.state() == AgentState::Waiting && !visible {
                    let title = t!("agent.state.notice_title", agent = agent.name.clone());
                    let body = agent
                        .tracker
                        .message()
                        .map(SharedString::from)
                        .unwrap_or_else(|| t!("agent.state.notice_body"));
                    self.agent_notice(AGENT_PANEL_TAG.into(), title, body, slot, window, cx);
                }
            }
            AgentSlot::Card(id) => {
                let Some(entity) = self.card(id, cx) else {
                    return;
                };
                let card = entity.read(cx);
                let state = card.agent_tracker().state();
                if self.card_seen(id, cx)
                    || !matches!(state, AgentState::Waiting | AgentState::TurnEnded)
                {
                    cx.notify();
                    return;
                }
                let title = t!(
                    "agent.state.card_notice_title",
                    card = card.name(),
                    agent = card
                        .agent_kind()
                        .map(|kind| kind.label())
                        .unwrap_or_default()
                );
                let body = card
                    .agent_tracker()
                    .message()
                    .map(SharedString::from)
                    .unwrap_or_else(|| t!("agent.state.notice_body"));
                entity.update(cx, |card, cx| card.set_attention(true, cx));
                if state == AgentState::Waiting {
                    self.agent_notice(
                        super::notices::notice_tag(id),
                        title,
                        body,
                        slot,
                        window,
                        cx,
                    );
                }
            }
        }
        cx.notify();
    }

    /// 窗口不在前走系统通知，在前时在窗口里提示；点了把那个 Agent 带到前面。
    fn agent_notice(
        &mut self,
        tag: String,
        title: SharedString,
        body: SharedString,
        slot: AgentSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !window.is_window_active() {
            cx.show_system_notification(SystemNotification {
                tag: tag.into(),
                title,
                body,
                actions: Vec::new(),
            });
            return;
        }
        let workspace = cx.weak_entity();
        let notification =
            Notification::warning(body)
                .title(title)
                .on_click(move |_, window, cx| {
                    _ = workspace
                        .update(cx, |workspace, cx| workspace.show_agent(slot, window, cx));
                });
        window.push_notification(notification, cx);
    }

    /// 把这个 Agent 带到前面：面板的展开面板，卡片的切过去。
    pub(super) fn show_agent(
        &mut self,
        slot: AgentSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match slot {
            AgentSlot::Panel => self.show_agent_panel(window, cx),
            AgentSlot::Card(card) => self.show_card(card, window, cx),
        }
    }

    /// 面板看得见：回合结束算看过了，图标栏的点熄掉。窗口回到前面、展开面板时调用。
    pub(super) fn see_agent_panel(&mut self) {
        if self.window_active
            && self.right_panel == Some(RightPanel::Agent)
            && let Some(agent) = &mut self.agent
        {
            agent.turn_seen = true;
        }
    }

    /// 图标栏 Agent 图标上的点和悬停提示（面板展开时没有点）。
    pub(super) fn agent_rail(&self) -> AgentRail {
        let state = self
            .agent
            .as_ref()
            .filter(|_| self.right_panel != Some(RightPanel::Agent))
            .map(|agent| (agent.tracker.state(), agent.turn_seen));
        let dot = match state {
            Some((AgentState::Waiting, _)) | Some((AgentState::TurnEnded, false)) => {
                Some(RailDot::Attention)
            }
            Some((AgentState::Running, _)) => Some(RailDot::Busy),
            _ => None,
        };
        let tooltip = match (dot, state) {
            (Some(_), Some((state, _))) => {
                t!("agent.rail.state", state = crate::agent::state_label(state))
            }
            _ => t!("workspace.right_sidebar.agent"),
        };
        AgentRail { dot, tooltip }
    }
}
