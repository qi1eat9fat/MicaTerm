//! 应用内的 Agent（Agent CLI 工单 07）：右侧图标栏的「Agent」点开 Agent 面板，里面跑设置里的
//! Agent 命令（默认自动检测 Claude Code、Codex）；远程卡片右键「交给 Agent」把卡片交给面板里的这个。
//! 启动前缺什么（外部 CLI、命令、Skill）先确认再补上。这里只管样子和规则，什么时候开、交给谁在
//! 窗口壳（`workspace/handlers/agent.rs`）。

mod history;
mod hook;
mod panel;
mod prep;
mod program;
mod prompt;
mod screen;
mod state;

pub use history::{Conversation, claude_config_dir, claude_conversations, conversation_time};
pub use hook::{HookSource, hook_event};
pub use panel::{AGENT_PANEL_WIDTH, AgentPanel};
pub use prep::{
    ListConversations, OnReady, Prep, PrepStep, StartChoice, StartOptions, open_prep_dialog,
    prep_steps,
};
pub use program::{AgentProgram, agent_program, agent_program_on};
pub use prompt::{Handoff, handoff_prompt};
pub use screen::{ScreenState, read_screen};
pub use state::{AgentState, AgentTracker, HookEvent, state_label};
