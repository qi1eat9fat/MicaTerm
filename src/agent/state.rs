//! Agent 的状态（Agent CLI 工单 09）：运行中、等你确认、回合结束、空闲。状态 hook 报的为主，
//! 屏幕规则补上「等你确认」（没装 hook 时也靠它认出运行中、回合结束）。
//! 优先级照 Pebrel：等你确认 > 运行中 > 回合结束 > 空闲。状态真的变了才把序号加一。

use gpui_kit::SharedString;
use serde::{Deserialize, Serialize};

use crate::i18n::t;

use super::screen::ScreenState;

/// 给人看的状态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    #[default]
    Idle,
    Running,
    /// 等你确认（权限请求、提问）。
    Waiting,
    TurnEnded,
}

/// 状态的名字（面板标题行、图标栏提示）。
pub fn state_label(state: AgentState) -> SharedString {
    match state {
        AgentState::Idle => t!("agent.state.idle"),
        AgentState::Running => t!("agent.state.running"),
        AgentState::Waiting => t!("agent.state.waiting"),
        AgentState::TurnEnded => t!("agent.state.turn_ended"),
    }
}

/// 状态 hook 报来的一件事（`micaterm hook` 把 Claude Code、Codex 的事件归成这几种）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// 用户发了一句话：回合开始。
    PromptSubmit,
    /// 要用户确认或回答。
    NeedsAttention,
    /// 一个工具跑完了：回合还在进行。
    ToolComplete,
    /// 回合结束。
    TurnDone,
    /// 会话结束（程序要退出了）。
    SessionEnd,
}

/// 一个 Agent 的状态，和它是怎么来的。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentTracker {
    /// hook 最后报的；没装 hook 时一直是 `None`。
    hook: Option<AgentState>,
    /// 屏幕规则最后认出的；认不出时保留上一次的。
    screen: Option<ScreenState>,
    state: AgentState,
    seq: u64,
    /// hook 带来的那句话（等你确认时通知的正文）。
    message: Option<String>,
}

impl AgentTracker {
    pub fn state(&self) -> AgentState {
        self.state
    }

    /// 状态变过几次（留给以后的 `cards wait`）。
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 等你确认时 hook 带来的那句话。
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// hook 报来一件事；状态变了返回 `true`。
    pub fn on_hook(&mut self, event: HookEvent, message: Option<String>) -> bool {
        let state = match event {
            HookEvent::PromptSubmit | HookEvent::ToolComplete => AgentState::Running,
            HookEvent::NeedsAttention => AgentState::Waiting,
            HookEvent::TurnDone => AgentState::TurnEnded,
            HookEvent::SessionEnd => AgentState::Idle,
        };
        self.hook = Some(state);
        if event == HookEvent::NeedsAttention {
            self.message = message;
        }
        self.settle()
    }

    /// 屏幕规则看了一眼（`None` 是认不出，不改）；状态变了返回 `true`。
    pub fn on_screen(&mut self, screen: Option<ScreenState>) -> bool {
        if screen.is_some() {
            self.screen = screen;
        }
        self.settle()
    }

    /// 程序退出了：回到空闲，忘掉 hook 和屏幕；状态变了返回 `true`。
    pub fn reset(&mut self) -> bool {
        self.hook = None;
        self.screen = None;
        self.message = None;
        self.change(AgentState::Idle)
    }

    fn settle(&mut self) -> bool {
        self.change(merge(self.hook, self.screen, self.state))
    }

    fn change(&mut self, next: AgentState) -> bool {
        if next == self.state {
            return false;
        }
        if next != AgentState::Waiting {
            self.message = None;
        }
        self.state = next;
        self.seq += 1;
        true
    }
}

/// hook 和屏幕合起来是什么状态（`previous` 是现在的）。
fn merge(
    hook: Option<AgentState>,
    screen: Option<ScreenState>,
    previous: AgentState,
) -> AgentState {
    // 选项框在屏幕上：不管 hook 说什么，都在等你。
    if screen == Some(ScreenState::Blocked) {
        return AgentState::Waiting;
    }
    match hook {
        // hook 说在等你，屏幕却已经在干活：用户已经答了（Claude Code 答完不再报 hook，等 Stop）。
        Some(AgentState::Waiting) if screen == Some(ScreenState::Working) => AgentState::Running,
        Some(state) => state,
        // 没装 hook：全看屏幕。干完活回到提示符就是回合结束。
        None => match screen {
            Some(ScreenState::Working) => AgentState::Running,
            Some(ScreenState::Idle) => match previous {
                AgentState::Running | AgentState::Waiting | AgentState::TurnEnded => {
                    AgentState::TurnEnded
                }
                AgentState::Idle => AgentState::Idle,
            },
            Some(ScreenState::Blocked) => AgentState::Waiting,
            None => previous,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentState, AgentTracker, HookEvent};
    use crate::agent::screen::ScreenState;

    #[test]
    fn hook_events_map_to_states() {
        let mut agent = AgentTracker::default();
        assert!(agent.on_hook(HookEvent::PromptSubmit, None));
        assert_eq!(agent.state(), AgentState::Running);
        assert!(agent.on_hook(
            HookEvent::NeedsAttention,
            Some("Claude needs your permission".into())
        ));
        assert_eq!(agent.state(), AgentState::Waiting);
        assert_eq!(agent.message(), Some("Claude needs your permission"));
        assert!(agent.on_hook(HookEvent::TurnDone, None));
        assert_eq!(agent.state(), AgentState::TurnEnded);
        assert_eq!(agent.message(), None, "只在等你确认时留着");
        assert!(agent.on_hook(HookEvent::SessionEnd, None));
        assert_eq!(agent.state(), AgentState::Idle);
    }

    /// 序号只在状态真的变了时加一：同样的事报两遍、认不出的屏幕都不加。
    #[test]
    fn the_sequence_moves_only_on_a_change() {
        let mut agent = AgentTracker::default();
        assert_eq!(agent.seq(), 0);
        agent.on_hook(HookEvent::PromptSubmit, None);
        assert_eq!(agent.seq(), 1);
        assert!(!agent.on_hook(HookEvent::ToolComplete, None), "还是运行中");
        assert!(!agent.on_screen(None));
        assert!(!agent.on_screen(Some(ScreenState::Working)));
        assert_eq!(agent.seq(), 1);
        agent.on_hook(HookEvent::TurnDone, None);
        assert_eq!(agent.seq(), 2);
        assert!(agent.reset(), "回合结束 → 空闲");
        assert_eq!(agent.seq(), 3);
        assert!(!agent.reset(), "已经是空闲");
        assert_eq!(agent.seq(), 3);
    }

    /// 等你确认 > 运行中 > 回合结束 > 空闲：选项框在屏幕上时 hook 说什么都算等你确认；
    /// hook 说在等你、屏幕已经在干活，就是答完了。
    #[test]
    fn the_screen_and_the_hook_are_merged_by_priority() {
        let mut agent = AgentTracker::default();
        agent.on_hook(HookEvent::PromptSubmit, None);
        agent.on_screen(Some(ScreenState::Blocked));
        assert_eq!(agent.state(), AgentState::Waiting, "屏幕上的选项框优先");
        agent.on_screen(Some(ScreenState::Working));
        assert_eq!(agent.state(), AgentState::Running);

        agent.on_hook(HookEvent::NeedsAttention, None);
        assert_eq!(
            agent.state(),
            AgentState::Running,
            "屏幕还在干活：已经答过了"
        );
        agent.on_screen(Some(ScreenState::Idle));
        assert_eq!(agent.state(), AgentState::Waiting);
        agent.on_hook(HookEvent::TurnDone, None);
        assert_eq!(agent.state(), AgentState::TurnEnded);
    }

    /// 没装 hook：全看屏幕，干完活回到提示符算回合结束；一开始就在提示符是空闲。
    #[test]
    fn without_hooks_the_screen_tells_the_turn() {
        let mut agent = AgentTracker::default();
        agent.on_screen(Some(ScreenState::Idle));
        assert_eq!(agent.state(), AgentState::Idle);
        agent.on_screen(Some(ScreenState::Working));
        assert_eq!(agent.state(), AgentState::Running);
        agent.on_screen(Some(ScreenState::Blocked));
        assert_eq!(agent.state(), AgentState::Waiting);
        agent.on_screen(Some(ScreenState::Working));
        agent.on_screen(Some(ScreenState::Idle));
        assert_eq!(agent.state(), AgentState::TurnEnded);
        agent.reset();
        assert_eq!(agent.state(), AgentState::Idle, "程序退出回到空闲");
    }
}
