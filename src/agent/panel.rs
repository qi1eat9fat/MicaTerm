//! 右侧的 Agent 面板（Agent CLI 工单 07，26 号「Agent 面板」屏）：和凭据、片段同一种停靠面板，宽 440；
//! 标题行「Agent」+ 淡字程序名和工作文件夹名，右边「重新开始」（先弹启动框选文件夹和对话）和「关闭」
//! （收起面板，Agent 照常在跑）；交给它卡片而它已在跑时，标题下一行提示说明填在
//! 输入框里；下面是 Agent 的终端（本地终端，程序是设置里的 Agent 命令）。整个窗口一份，收起时
//! Agent 照常在跑。

use std::path::PathBuf;

use gpui_kit::component::Icon;
use gpui_kit::*;

use micaterm_terminal::{TerminalLifecycle, TerminalView};

use super::state::{AgentState, AgentTracker, state_label};
use crate::app::{CatalogIcon, RestartAgent, ShowAgent};
use crate::cli::AgentKind;
use crate::i18n::t;
use crate::theme::space;
use crate::theme::{INTERFACE_FONT_SIZE, Tokens, TypeScale, WEIGHT_SEMIBOLD, size};
use crate::ui::dock::dock_panel;
use crate::ui::icon_button::IconButton;

/// 面板第一次展开的宽度（与停靠面板统一）。之后是用户拖到的宽度。
pub const AGENT_PANEL_WIDTH: Pixels = size::DOCK_PANEL_WIDTH;

/// 面板里跑着的 Agent。
pub struct AgentPanel {
    pub terminal: Entity<TerminalView>,
    /// Agent 的名字（Claude Code）。
    pub name: SharedString,
    /// 它所在卡片的 CLI 卡片 ID（`MICATERM_SELF_CARD`）。
    pub self_id: SharedString,
    /// 交给它的卡片说明填在了输入框里时，标题下面那一行。
    pub note: Option<SharedString>,
    /// 认得出的 Agent（屏幕规则按它挑；认不出的程序不判状态）。
    pub kind: Option<AgentKind>,
    /// 它在干什么（工单 09）。
    pub tracker: AgentTracker,
    /// 回合结束以后面板看见过没有：看见过图标栏就不再亮点。
    pub turn_seen: bool,
    /// 它在哪个文件夹里启动的（启动框里选的）；`None` 是连主目录都找不到时（标题行不写）。
    pub directory: Option<PathBuf>,
}

impl AgentPanel {
    /// Agent 还在跑（没退出、没启动失败）。
    pub fn is_running(&self, cx: &App) -> bool {
        matches!(
            self.terminal.read(cx).lifecycle(cx),
            TerminalLifecycle::Starting | TerminalLifecycle::Running
        )
    }

    /// 标题行程序名后面的状态（26 号 `.ag-st`）：点 + 字，等你确认是橙色字；空闲不写。
    fn state_chip(&self, tokens: &Tokens) -> Option<impl IntoElement> {
        let state = self.tracker.state();
        let (dot, text) = match state {
            AgentState::Idle => return None,
            AgentState::Running => (tokens.ok, tokens.st_dim),
            AgentState::Waiting => (tokens.warn, tokens.warn),
            AgentState::TurnEnded => (tokens.st_dim, tokens.st_dim),
        };
        let label = state_label(state);
        Some(
            div()
                .id("agent-panel-state")
                .test_support()
                .aria_label(label.clone())
                .flex()
                .flex_none()
                .items_center()
                .gap(space::SPACE_5)
                .ml(space::SPACE_8)
                .text_color(text)
                .child(crate::ui::dot(dot, size::DOT))
                .child(label),
        )
    }

    /// 程序名后面淡字的工作文件夹名（`· ssh`），无障碍名是完整路径。
    fn folder_label(&self, tokens: &Tokens) -> Option<impl IntoElement> {
        let directory = self.directory.as_ref()?;
        let full: SharedString = directory.to_string_lossy().into_owned().into();
        // 盘符根目录（`E:\`）没有文件名，写完整路径。
        let name: SharedString = directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned().into())
            .unwrap_or_else(|| full.clone());
        Some(
            div()
                .id("agent-panel-folder")
                .test_support()
                .aria_label(full)
                .min_w_0()
                .truncate()
                .text_color(tokens.st_dim)
                .child(format!("· {name}")),
        )
    }

    pub fn render(&self, target: &FocusHandle, tokens: &Tokens) -> AnyElement {
        let scale = TypeScale::from_base(INTERFACE_FONT_SIZE);
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(space::SPACE_5)
            .h(size::DOCK_HEADER)
            .pl(space::SPACE_10)
            // 右边只留 1：20 宽按钮的中心和顶栏 22 宽窗口按钮的圆点中心对齐。
            .pr(px(1.))
            .border_b(size::LINE)
            .border_color(tokens.line)
            .text_size(scale.sm)
            .child(
                div()
                    .flex_none()
                    .text_color(tokens.icon)
                    .child(Icon::new(CatalogIcon::Sparkles).size(size::ICON_SM)),
            )
            .child(
                div()
                    .flex_none()
                    .font_weight(WEIGHT_SEMIBOLD)
                    .text_color(tokens.text)
                    .child(t!("agent.panel.title")),
            )
            .child(
                div()
                    .id("agent-panel-name")
                    .test_support()
                    .aria_label(self.name.clone())
                    .min_w_0()
                    .truncate()
                    .text_color(tokens.st_dim)
                    .child(self.name.clone()),
            )
            .children(self.folder_label(tokens))
            .children(self.state_chip(tokens))
            .child(div().flex_1())
            // 两个按钮贴在一起（间距 2，和凭据、片段面板的标题行一致），好和顶栏窗口按钮对齐。
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(space::SPACE_2)
                    .child(
                        IconButton::new(
                            "agent-panel-restart",
                            CatalogIcon::RefreshCw,
                            t!("agent.panel.restart"),
                        )
                        .sm()
                        // 刷新箭头画出来比关闭的 × 大一圈，单独小一号。
                        .icon_size(px(10.))
                        .dispatch(Box::new(RestartAgent), target, tokens),
                    )
                    .child(
                        IconButton::new(
                            "agent-panel-close",
                            CatalogIcon::Close,
                            t!("agent.panel.close"),
                        )
                        .sm()
                        .dispatch(Box::new(ShowAgent), target, tokens),
                    ),
            );
        let note = self.note.clone().map(|note| {
            div()
                .id("agent-panel-note")
                .test_support()
                .aria_label(note.clone())
                .flex()
                .flex_none()
                .items_center()
                .gap(space::SPACE_5)
                .h(size::DOCK_HEADER)
                .pl(space::SPACE_10)
                .pr(space::SPACE_16)
                .border_b(size::LINE)
                .border_color(tokens.line)
                .text_size(scale.sm)
                .text_color(tokens.text2)
                .child(
                    div()
                        .flex_none()
                        .text_color(tokens.icon)
                        .child(Icon::new(CatalogIcon::Check).size(size::ICON_SM)),
                )
                .child(div().min_w_0().truncate().child(note))
        });
        // 外框和凭据、片段面板共用：宽度由外面的可拖分隔决定（默认 `AGENT_PANEL_WIDTH`），这里填满。
        dock_panel("agent-panel", tokens)
            .test_support()
            .h_full()
            .child(header)
            .children(note)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(self.terminal.clone()),
            )
            .into_any_element()
    }
}
