//! 右侧面板的入口：照空壳，四个图标并排放在标签栏右侧（文件、片段、凭据、Agent），
//! 不再用自己的图标栏。
//!
//! 「文件」打开或切换到 SFTP 标签（工单 20），不展开面板；在 SFTP 标签里显示成按下的样子（当前所在），
//! 没有远程文件可开时变灰，悬停说为什么。
//! 片段、凭据点一下在右边展开宽 400 的面板，再点一下收起，展开时图标显示成按下的样子。
//! Agent 展开宽 400 的 Agent 面板，收起时 Agent 照常在跑。
//!
//! 面板左边那条细线可以左右拖（`with_panel`）：三种面板各记各的宽度，收起再展开还是拖到的宽度；
//! 窗口变宽变窄时面板宽度不变，内容区让出或吃下差额。

use std::ops::Range;

use gpui_kit::base::{ResizableState, h_resizable, resizable_panel};
use gpui_kit::*;

use crate::agent::AGENT_PANEL_WIDTH;
use crate::app::{CatalogIcon, OpenSftp, ShowAgent, ShowCredentials, ShowSnippets};
use crate::i18n::t;
use crate::theme::space;
use crate::theme::{Tokens, size};
use crate::ui::icon_button::IconButton;
use crate::ui::resize::invisible_handle;

/// 拖面板时内容区（终端、SFTP……）至少留这么宽。
const MIN_CONTENT_WIDTH: Pixels = px(240.);

/// 右侧栏展开的面板，同一时间最多一个。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightPanel {
    /// 命令片段（工单 19）。
    Snippets,
    /// 凭据（工单 14）。
    Credentials,
    /// 应用内的 Agent（Agent CLI 工单 07）。
    Agent,
}

impl RightPanel {
    /// 第一次展开时的宽度；之后是用户拖到的宽度。
    fn default_width(self) -> Pixels {
        match self {
            RightPanel::Snippets | RightPanel::Credentials => size::DOCK_PANEL_WIDTH,
            RightPanel::Agent => AGENT_PANEL_WIDTH,
        }
    }

    /// 拖的时候面板宽度能到的范围（内容区另有 `MIN_CONTENT_WIDTH` 的下限）。
    fn width_range(self) -> Range<Pixels> {
        match self {
            RightPanel::Snippets | RightPanel::Credentials => px(200.)..px(640.),
            RightPanel::Agent => px(320.)..px(1200.),
        }
    }

    fn index(self) -> usize {
        match self {
            RightPanel::Snippets => 0,
            RightPanel::Credentials => 1,
            RightPanel::Agent => 2,
        }
    }
}

/// 三种面板各自拖到的宽度（resizable 组件的状态）。存在实体里：面板收起时元素状态就没了。
pub struct RightPanelWidths {
    states: [Entity<ResizableState>; 3],
}

impl RightPanelWidths {
    pub fn new(cx: &mut App) -> Self {
        Self {
            states: std::array::from_fn(|_| cx.new(|_| ResizableState::default())),
        }
    }

    /// 内容区和展开的面板之间隔一条能左右拖的分隔线（线是面板自己的左边线，柄里不画东西）。
    pub fn with_panel(
        &self,
        panel: RightPanel,
        content: impl IntoElement,
        panel_element: AnyElement,
    ) -> AnyElement {
        let ix = panel.index();
        h_resizable(("right-panel-split", ix))
            .with_state(&self.states[ix])
            .with_handle_appearance(invisible_handle("right-panel-handle"))
            .child(
                resizable_panel()
                    .size_range(MIN_CONTENT_WIDTH..Pixels::MAX)
                    .child(content),
            )
            .child(
                resizable_panel()
                    .size(panel.default_width())
                    .size_range(panel.width_range())
                    .fixed()
                    .child(panel_element),
            )
            .into_any_element()
    }
}

/// 图标栏 Agent 图标右上角的点（工单 09，26 号 `.rail .rdot`）：和标签状态点同一套颜色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailDot {
    /// 橙点：等你确认，或没看过的回合结束。
    Attention,
    /// 绿点：运行中。
    Busy,
}

/// 图标栏 Agent 图标现在什么样：点，和悬停提示（「Agent · 等你确认」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRail {
    pub dot: Option<RailDot>,
    pub tooltip: SharedString,
}

/// 右侧栏「文件」现在是什么样。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilesButton {
    /// 点了打开或切到 SFTP 标签。
    Open,
    /// 在前的就是 SFTP 标签：显示成按下的样子（26 号 `.rail .ib.on`）。
    Current,
    /// 没有远程文件可开：变灰，悬停写原因。
    Unavailable(SharedString),
}

pub(super) fn render_panel_tools(
    open: Option<RightPanel>,
    files: FilesButton,
    agent: AgentRail,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
    let button = |id: &'static str, icon: CatalogIcon, label: SharedString| {
        IconButton::new(id, icon, label).sm()
    };
    let files_button = button(
        "right-sidebar-files",
        CatalogIcon::Folder,
        t!("workspace.right_sidebar.files"),
    );
    let files_button = match files {
        FilesButton::Open => files_button.tooltip(t!("sftp_tab.files.open")),
        FilesButton::Current => files_button
            .pressed(true)
            .tooltip(t!("sftp_tab.files.open")),
        FilesButton::Unavailable(reason) => files_button.dimmed(true).tooltip(reason),
    };
    div()
        .id("right-sidebar")
        .test_support()
        .flex()
        .flex_none()
        .items_center()
        .gap(space::SPACE_2)
        // 右边不留白：分隔线紧挨着 Agent 图标，左边才留 2。
        .pl(space::SPACE_2)
        .child(files_button.dispatch(Box::new(OpenSftp), target, tokens))
        .child(
            button(
                "right-sidebar-snippets",
                CatalogIcon::CodeXml,
                t!("workspace.right_sidebar.snippets"),
            )
            .pressed(open == Some(RightPanel::Snippets))
            .dispatch(Box::new(ShowSnippets), target, tokens),
        )
        .child(
            button(
                "right-sidebar-credentials",
                CatalogIcon::KeyRound,
                t!("workspace.right_sidebar.credentials"),
            )
            .pressed(open == Some(RightPanel::Credentials))
            .dispatch(Box::new(ShowCredentials), target, tokens),
        )
        .child(
            div()
                .relative()
                .child(
                    button(
                        "right-sidebar-agent",
                        CatalogIcon::Sparkles,
                        t!("workspace.right_sidebar.agent"),
                    )
                    .pressed(open == Some(RightPanel::Agent))
                    .tooltip(agent.tooltip.clone())
                    .dispatch(Box::new(ShowAgent), target, tokens),
                )
                .children(agent.dot.map(|dot| {
                    div()
                        .id("right-sidebar-agent-dot")
                        .test_support()
                        .aria_label(agent.tooltip.clone())
                        .absolute()
                        .top(px(2.))
                        .right(px(2.))
                        .size(px(7.))
                        .rounded_full()
                        .border(px(1.5))
                        .border_color(tokens.tabbar)
                        .bg(match dot {
                            RailDot::Attention => tokens.warn,
                            RailDot::Busy => tokens.ok,
                        })
                })),
        )
}
