//! 右侧的 Agent 面板（Agent CLI 工单 07，26 号「Agent 面板」屏）：和凭据、片段同一种停靠面板，宽 440；
//! 标题行「Agent」+ 淡字程序名，右边「重新开始」；交给它卡片而它已在跑时，标题下一行提示说明填在
//! 输入框里；下面是 Agent 的终端（本地终端，程序是设置里的 Agent 命令）。整个窗口一份，收起时
//! Agent 照常在跑。

use gpui_kit::component::Icon;
use gpui_kit::*;

use micaterm_terminal::{TerminalLifecycle, TerminalView};

use crate::app::{CatalogIcon, RestartAgent};
use crate::i18n::t;
use crate::theme::{Tokens, size};
use crate::ui::icon_button::IconButton;

/// 面板宽度：终端要宽一些（凭据、片段是 280）。
pub const AGENT_PANEL_WIDTH: Pixels = px(440.);

/// 面板里跑着的 Agent。
pub struct AgentPanel {
    pub terminal: Entity<TerminalView>,
    /// Agent 的名字（Claude Code）。
    pub name: SharedString,
    /// 它所在卡片的 CLI 卡片 ID（`MICATERM_SELF_CARD`）。
    pub self_id: SharedString,
    /// 交给它的卡片说明填在了输入框里时，标题下面那一行。
    pub note: Option<SharedString>,
}

impl AgentPanel {
    /// Agent 还在跑（没退出、没启动失败）。
    pub fn is_running(&self, cx: &App) -> bool {
        matches!(
            self.terminal.read(cx).lifecycle(cx),
            TerminalLifecycle::Starting | TerminalLifecycle::Running
        )
    }

    pub fn render(&self, target: &FocusHandle, tokens: &Tokens) -> AnyElement {
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(32.))
            .pl(px(16.))
            .pr(px(8.))
            .child(
                div()
                    .flex_none()
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
            .child(div().flex_1())
            .child(
                IconButton::new(
                    "agent-panel-restart",
                    CatalogIcon::RefreshCw,
                    t!("agent.panel.restart"),
                )
                .size(px(24.), px(14.))
                .dispatch(Box::new(RestartAgent), target, tokens),
            );
        let note = self.note.clone().map(|note| {
            div()
                .id("agent-panel-note")
                .test_support()
                .aria_label(note.clone())
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .h(px(28.))
                .px(px(16.))
                .border_b(size::LINE)
                .border_color(tokens.line)
                .text_color(tokens.text2)
                .child(
                    div()
                        .flex_none()
                        .text_color(tokens.icon)
                        .child(Icon::new(CatalogIcon::Check).size(px(12.))),
                )
                .child(div().min_w_0().truncate().child(note))
        });
        div()
            .id("agent-panel")
            .test_support()
            .flex()
            .flex_col()
            .flex_none()
            .w(AGENT_PANEL_WIDTH)
            .h_full()
            .border_l(size::LINE)
            .border_color(tokens.line)
            .bg(tokens.content)
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
