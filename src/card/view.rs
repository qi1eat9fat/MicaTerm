//! `TerminalCard`：一张卡片（26 号的窗格），持有它的终端，从上到下画顶行（名称、在跑的程序、分屏和关闭按钮，
//! 2026-11-14 用户定）、终端和底部状态栏。本地卡片和远程卡片共用它，
//! 不同的地方按 `CardKind` 分支（远程的那一部分在 `remote.rs`）。
//! 卡片外壳照 Rex 的卡片新写；行为参考 since2006/shell-rs @ b2a307a 的
//! `src/terminal/local_terminal_panel.rs`（目录事件的转发）和 `src/terminal/terminal_panel.rs`
//! （远程面板：原地重连、断开、右键菜单）。

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use micaterm_hosts::{Host, HostId, HostOs};
use micaterm_terminal::{
    LocalLaunch, LocalShell, LocalShellInfo, SharedLocalTerminalTransportProvider,
    SharedTerminalTransportFactory, TerminalEvent, TerminalFont, TerminalMenuItems, TerminalNotice,
    TerminalView,
};

use super::CardId;
use super::directory::{display_directory, home_dir, reported_path};
use super::fit::measure_width;
use super::holding::holding_factory;
use super::remote::{RemoteCard, RemoteLaunch, RemoteState, RemoteUpdate, VaultHold};
use super::status_bar::{StatusBar, StatusState, render_status_bar};
use super::status_dot::{ACTIVE_FOR, StatusDot};
use super::title_row::{CardPlace, Paint, TitleRow, render_title_row};
use crate::app::{
    CloseCard, CloseFocusedCard, CopyCardAddress, DisconnectCard, HandCardToAgent,
    MoveCardToNewTab, OpenCardSftp, ReconnectCard, TakeCardBack, ToggleZoomCard, ZoomCard,
};
use crate::backend::{ConnectionId, RemoteTarget};
use crate::i18n::t;
use crate::programs::{Program, program_of, shell_program};
use crate::theme::{Tokens, TypeScale};

/// 终端文字离顶行、窗格左右两边的距离（26 号：顶行下面 `.term` 的上边距收到 4，左右 20）。
/// 终端自己四周留 0.5 rem，上边距不会比它小。
const TERMINAL_TOP: Pixels = px(8.);
const TERMINAL_SIDE: Pixels = px(20.);

/// 卡片告诉持有它的人（窗口壳）的事。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardEvent {
    /// 名称、目录、程序图块、状态点、连接状态变了：标签跟着重画。
    Changed,
    /// 程序结束了（前台从别的程序回到 shell），或者响铃了：卡片看不见时，窗口壳给它亮橙点（工单 06）。
    WantsAttention,
}

/// 卡片里跑的是什么。
enum CardKind {
    /// 本地 shell：名称和版本在后台查，查到前是 `None`。
    Local {
        shell: LocalShell,
        info: Option<LocalShellInfo>,
    },
    /// 远程终端（工单 12）：连到哪台主机、连接的状态，见 `remote.rs`。
    Remote(Box<RemoteCard>),
}

pub struct TerminalCard {
    id: CardId,
    kind: CardKind,
    terminal: Entity<TerminalView>,
    /// 终端的焦点：焦点在它里面就是这张卡片聚焦。
    focus: FocusHandle,
    /// 窗口壳的焦点：按钮经它派发 action。
    /// 启动时的目录；没有就是主目录。
    launch_directory: Option<PathBuf>,
    /// shell 自己报告的目录（OSC 7 等），优先。
    reported_directory: Option<PathBuf>,
    /// 本机说 shell 在的目录。
    working_directory: Option<PathBuf>,
    /// 是不是所在标签的焦点卡片：顶行按钮常显。由标签按分屏树告诉它。
    focused: bool,
    /// 是不是放大着：顶行多一个「还原」，右键菜单的「放大卡片」换成「还原卡片」。
    zoomed: bool,
    /// 是不是标签里唯一的一张：顶行不放「关闭」，右键菜单的「移到新标签」变灰。
    alone: bool,
    /// 前台程序（本机看到的进程名）；`None` 是 shell 自己在提示符下（工单 06）。
    program: Option<String>,
    /// 最近一次有输出的时候（执行器的时钟，测试能拨它）。
    output_at: Option<Instant>,
    /// 在看不见的地方结束或响铃过，还没被看见：橙点。由窗口壳设、清。
    attention: bool,
    /// 上一次告诉窗口壳的状态点：变了才再说。
    shown_dot: StatusDot,
    /// 上一次告诉窗口壳的连接状态（远程卡片）：变了才再说，标签上的图块跟着变灰。
    shown_state: Option<RemoteState>,
    /// 上一帧量到的卡片宽度：顶行、状态栏正中那一段放不放得下看它（还没量过是 0）。
    width: Rc<Cell<f32>>,
    /// 绿点到点熄灭的定时器，和它是不是还在等。
    _quiet_task: Option<Task<()>>,
    quiet_scheduled: bool,
    _info_task: Option<Task<()>>,
    /// 交给 Agent 了（Agent CLI 工单 06）：CLI 认的卡片 ID，每次交出去新起一个，收回就作废。
    agent: Option<SharedString>,
    /// 本地卡片自己的 CLI 卡片 ID（Agent CLI 工单 07）：开卡片时起好，放进它的 `MICATERM_SELF_CARD`。
    /// 远程卡片没有。
    self_id: Option<SharedString>,
    /// 同一件事给右键菜单看（菜单打开时才建，读不到卡片）：交出去了就显示「收回」。
    agent_flag: Rc<Cell<bool>>,
    /// 前台是 claude、codex 时它在干什么（Agent CLI 工单 09）。
    agent_tracker: crate::agent::AgentTracker,
    /// Agent 正在这张卡片里执行命令：状态栏的标记写「执行中」。窗口壳定时看。
    agent_running: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CardEvent> for TerminalCard {}

impl TerminalCard {
    /// 一张本地卡片：按 `launch` 起 shell，传输由 `provider` 建（测试注入假的）。
    pub fn local(
        id: CardId,
        launch: LocalLaunch,
        provider: SharedLocalTerminalTransportProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let factory = provider.factory_for(&launch);
        let terminal = cx.new(|cx| {
            TerminalView::new(
                id.element("terminal"),
                |_| t!("card.local.title"),
                factory,
                window,
                cx,
            )
        });
        // shell 名和版本要看文件系统，放到后台查。
        let shell = launch.shell.clone();
        let info_task = cx.spawn(async move |this, cx| {
            let query = shell.clone();
            let info = cx
                .background_spawn(async move { provider.shell_info(&query) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let CardKind::Local { info: slot, .. } = &mut this.kind {
                    *slot = Some(info);
                }
                cx.emit(CardEvent::Changed);
                cx.notify();
            });
        });
        let kind = CardKind::Local {
            shell: launch.shell,
            info: None,
        };
        let mut card = Self::with_terminal(id, kind, terminal, window, cx);
        card.launch_directory = launch.directory;
        card._info_task = Some(info_task);
        card
    }

    /// 一张远程卡片（工单 12）：在 `launch.target` 的连接上开一个 shell 通道。断开后按 Enter
    /// 原地重连（`ReconnectCard`）、按 Space 关闭（`CloseCard`）；右键菜单有复制 IP、重新连接、断开连接。
    pub fn remote(
        id: CardId,
        launch: RemoteLaunch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let RemoteLaunch {
            host,
            target: remote,
            factory,
        } = launch;
        let name = host.name.clone();
        let agent_flag = Rc::new(Cell::new(false));
        let menu_flag = agent_flag.clone();
        let terminal = cx.new(|cx| {
            let mut view = TerminalView::new(
                id.element("terminal"),
                move |_| name.clone(),
                factory,
                window,
                cx,
            );
            view.set_ended_actions(ReconnectCard(id), CloseCard(id), cx);
            view.set_menu_items(remote_menu_items(id, &host, menu_flag));
            view
        });
        let kind = CardKind::Remote(Box::new(RemoteCard::new(host, remote)));
        let mut card = Self::with_terminal(id, kind, terminal, window, cx);
        card.shown_state = card.remote_state(cx);
        card.agent_flag = agent_flag;
        card
    }

    /// 一张等保险库解锁的远程卡片（工单 33）：先不连，终端是占位的，状态栏写「等待解锁保险库」；
    /// 之后同 [`Self::wait_for_vault`]。
    pub fn remote_waiting_for_vault(
        id: CardId,
        host: Host,
        remote: RemoteTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let launch = RemoteLaunch {
            host,
            target: remote,
            factory: holding_factory(),
        };
        let mut card = Self::remote(id, launch, window, cx);
        if let CardKind::Remote(remote) = &mut card.kind {
            remote.hold_for_vault(VaultHold::Waiting);
        }
        card.shown_state = card.remote_state(cx);
        card
    }

    fn with_terminal(
        id: CardId,
        kind: CardKind,
        terminal: Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = terminal.read(cx).focus_handle();
        set_card_items(&terminal, id, false, true, cx);
        // 查找浮卡按窗格的边摆，不是按终端的边。
        terminal.update(cx, |terminal, _| {
            terminal.set_text_inset(TERMINAL_TOP, TERMINAL_SIDE)
        });
        let subscriptions = vec![
            cx.observe(&terminal, |this, terminal, cx| {
                this.refresh_remote_state(cx);
                // 标题和全屏不单独发事件：每次重画看一眼（工单 17）。
                if let CardKind::Remote(remote) = &mut this.kind {
                    let terminal = terminal.read(cx);
                    let (title, full_screen) = (terminal.title(cx), terminal.is_full_screen(cx));
                    let was_running = remote.activity.running;
                    if remote.observe_screen(title.as_deref(), full_screen) {
                        this.remote_activity_changed(was_running, cx);
                    }
                }
                cx.notify();
            }),
            cx.subscribe(&terminal, Self::on_terminal_event),
            // 聚焦与否换光标、顶行和状态栏的颜色：终端拿到或失去焦点时重画卡片。
            cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
            cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
        ];
        Self {
            id,
            kind,
            terminal,
            focus,
            launch_directory: None,
            reported_directory: None,
            working_directory: None,
            // 新卡片总是它所在标签的焦点卡片（新标签、分屏都把焦点给新卡片）。
            focused: true,
            zoomed: false,
            alone: true,
            program: None,
            output_at: None,
            attention: false,
            shown_dot: StatusDot::None,
            shown_state: None,
            width: Rc::new(Cell::new(0.)),
            _quiet_task: None,
            quiet_scheduled: false,
            _info_task: None,
            agent: None,
            self_id: None,
            agent_flag: Rc::default(),
            agent_running: false,
            agent_tracker: Default::default(),
            _subscriptions: subscriptions,
        }
    }

    /// 交给 Agent（只有远程卡片）：起一个新的 CLI 卡片 ID 交回去。已经交出去的照旧用原来的。
    pub fn hand_to_agent(&mut self, cx: &mut Context<Self>) -> Option<SharedString> {
        if !matches!(self.kind, CardKind::Remote(_)) {
            return None;
        }
        let id = self
            .agent
            .get_or_insert_with(|| micaterm_hosts::PublicId::generate().to_string().into())
            .clone();
        self.agent_flag.set(true);
        cx.notify();
        Some(id)
    }

    /// 收回：CLI 卡片 ID 作废，之后的 `exec --card` 回 `no_terminal`。
    pub fn take_back(&mut self, cx: &mut Context<Self>) {
        self.agent = None;
        self.agent_flag.set(false);
        self.agent_running = false;
        cx.notify();
    }

    /// 本地卡片自己的 CLI 卡片 ID（窗口壳开卡片时定，见 [`Self::set_self_id`]）。
    pub fn self_id(&self) -> Option<&SharedString> {
        self.self_id.as_ref()
    }

    /// 开本地卡片时记下它的 CLI 卡片 ID：和放进它 shell 环境里的 `MICATERM_SELF_CARD` 是同一个。
    pub fn set_self_id(&mut self, id: SharedString) {
        self.self_id = Some(id);
    }

    /// 交出去时 CLI 认的卡片 ID；没交出去是 `None`。
    pub fn agent_id(&self) -> Option<&SharedString> {
        self.agent.as_ref()
    }

    /// 本地卡片的前台程序是 Claude Code、Codex 时是哪个（工单 09）；远程卡片和别的程序 `None`。
    pub fn agent_kind(&self) -> Option<crate::cli::AgentKind> {
        if !matches!(self.kind, CardKind::Local { .. }) {
            return None;
        }
        let program = self.program.as_deref()?;
        let name = std::path::Path::new(program)
            .file_stem()?
            .to_string_lossy()
            .to_lowercase();
        match name.as_str() {
            "claude" => Some(crate::cli::AgentKind::ClaudeCode),
            "codex" => Some(crate::cli::AgentKind::Codex),
            _ => None,
        }
    }

    pub fn agent_tracker(&self) -> &crate::agent::AgentTracker {
        &self.agent_tracker
    }

    /// 改 Agent 的状态；变了就重画、告诉窗口壳，交回 `true`。
    pub fn update_agent(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut crate::agent::AgentTracker) -> bool,
    ) -> bool {
        let changed = change(&mut self.agent_tracker);
        if changed {
            cx.emit(CardEvent::Changed);
            cx.notify();
        }
        changed
    }

    /// Agent 在不在这里执行命令（窗口壳定时问终端、告诉卡片）。
    pub fn set_agent_running(&mut self, running: bool, cx: &mut Context<Self>) {
        if self.agent_running != running {
            self.agent_running = running;
            cx.notify();
        }
    }

    fn on_terminal_event(
        &mut self,
        _: Entity<TerminalView>,
        event: &TerminalEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalEvent::DirectoryChanged(directory) => match &mut self.kind {
                CardKind::Local { .. } => {
                    self.reported_directory = Some(reported_path(&directory.path));
                }
                CardKind::Remote(remote) => {
                    if !remote.report_directory(directory) {
                        return;
                    }
                }
            },
            // 远程 shell 集成说的命令状态（工单 17）；本地卡片看本机的前台进程，不用它。
            TerminalEvent::ShellCommand(command) => {
                if let CardKind::Remote(remote) = &mut self.kind {
                    let was_running = remote.activity.running;
                    if remote.shell_command(command.clone()) {
                        self.remote_activity_changed(was_running, cx);
                    }
                }
                return;
            }
            TerminalEvent::WorkingDirectory(folder) => {
                self.working_directory = Some(folder.clone());
            }
            TerminalEvent::ForegroundProgram(program) => {
                // 别的程序结束、回到了 shell：卡片看不见时要亮橙点（gorex 的 `apply`）。
                let finished = self.running_program().is_some()
                    && program
                        .as_deref()
                        .is_none_or(|program| program_of(program).shell);
                self.program = program.clone();
                if finished {
                    cx.emit(CardEvent::WantsAttention);
                }
                self.refresh_dot(cx);
            }
            // 重新启动（Enter 原地重连）：新的 shell 在提示符下。
            TerminalEvent::Started => {
                self.program = None;
                self.refresh_remote_state(cx);
                self.refresh_dot(cx);
            }
            TerminalEvent::Output => {
                self.output_at = Some(cx.background_executor().now());
                self.refresh_dot(cx);
                return;
            }
            TerminalEvent::Notice(TerminalNotice::Bell { .. }) => {
                cx.emit(CardEvent::WantsAttention);
                return;
            }
            _ => return,
        }
        cx.emit(CardEvent::Changed);
        cx.notify();
    }

    /// 远程卡片的连接状态变了就告诉窗口壳：标签上的图块跟着变灰、变回来。
    fn refresh_remote_state(&mut self, cx: &mut Context<Self>) {
        let state = self.remote_state(cx);
        if state != self.shown_state {
            let was_ended = matches!(self.shown_state, Some(RemoteState::Disconnected(_)));
            self.shown_state = state.clone();
            if let Some(RemoteState::Disconnected(reason)) = &state
                && !was_ended
            {
                // 断开了：像普通输出一样在终端末尾写两三行（26 号，用户定），不另画覆盖层。
                let notice =
                    ended_notice(reason, &chrono::Local::now().format("%H:%M:%S").to_string());
                self.terminal
                    .update(cx, |terminal, cx| terminal.append_output(&notice, cx));
            }
            cx.emit(CardEvent::Changed);
        }
    }

    /// 远程卡片被动解析出的东西变了（工单 17）：程序跑完回到提示符时，卡片看不见就要亮橙点；
    /// 图块、目录、状态点跟着变。
    fn remote_activity_changed(&mut self, was_running: bool, cx: &mut Context<Self>) {
        let running = self
            .remote_part()
            .is_some_and(|remote| remote.activity.running);
        if was_running && !running {
            cx.emit(CardEvent::WantsAttention);
        }
        self.refresh_dot(cx);
        cx.emit(CardEvent::Changed);
        cx.notify();
    }

    /// 前台在跑的程序（shell 以外的）：本地卡片是本机看到的前台进程；远程卡片是被动解析出的
    /// （只知道在跑、不知道是谁时是系统徽章）。
    fn running_program(&self) -> Option<Program> {
        match &self.kind {
            CardKind::Local { .. } => self
                .program
                .as_deref()
                .map(program_of)
                .filter(|program| !program.shell),
            CardKind::Remote(remote) => remote.running_program(),
        }
    }

    /// 图块上显示的程序：前台程序；shell 在提示符下时，本地卡片是 shell 的终端图块，
    /// 远程卡片是主机的系统徽章（拿不到程序名时也是它，还没探测出系统时是名称首字；被动解析出
    /// 程序名后换成程序图块，工单 17）。远程卡片没连着时图块变灰。
    pub fn tile_program(&self) -> Program {
        match &self.kind {
            CardKind::Local { .. } => self
                .running_program()
                .unwrap_or_else(|| shell_program(&self.name())),
            CardKind::Remote(remote) => {
                let tile = self.running_program().unwrap_or_else(|| remote.badge());
                if matches!(self.shown_state, Some(RemoteState::Connected)) {
                    tile
                } else {
                    tile.offline()
                }
            }
        }
    }

    /// 状态点：在看不见的地方结束或响铃过是橙点，shell 以外的程序 1.5 秒内有输出是绿点。
    pub fn status_dot(&self, cx: &App) -> StatusDot {
        StatusDot::of(
            self.attention,
            self.running_program().as_ref(),
            self.output_at,
            cx.background_executor().now(),
        )
    }

    /// 窗口壳说这张卡片要不要注意（看不见时结束或响铃了 / 又看得见了）。
    pub fn set_attention(&mut self, attention: bool, cx: &mut Context<Self>) {
        if self.attention != attention {
            self.attention = attention;
            self.refresh_dot(cx);
        }
    }

    /// 是不是在等着被看见（橙点）。
    pub fn wants_attention(&self) -> bool {
        self.attention
    }

    /// 状态点变了就重画、告诉窗口壳（标签上的点跟着变）；亮着绿点就定好熄灭的时候。
    fn refresh_dot(&mut self, cx: &mut Context<Self>) {
        let dot = self.status_dot(cx);
        if dot != self.shown_dot {
            self.shown_dot = dot;
            cx.emit(CardEvent::Changed);
            cx.notify();
        }
        if dot == StatusDot::Busy && !self.quiet_scheduled {
            self.quiet_scheduled = true;
            self._quiet_task = Some(cx.spawn(async move |this, cx| {
                loop {
                    let Ok(Some(left)) = this.update(cx, |this, cx| {
                        let now = cx.background_executor().now();
                        this.output_at
                            .map(|at| ACTIVE_FOR.saturating_sub(now.saturating_duration_since(at)))
                            .filter(|left| !left.is_zero())
                    }) else {
                        break;
                    };
                    cx.background_executor().timer(left).await;
                }
                let _ = this.update(cx, |this, cx| {
                    this.quiet_scheduled = false;
                    this.refresh_dot(cx);
                });
            }));
        }
    }

    pub fn id(&self) -> CardId {
        self.id
    }

    /// 卡片里的终端：复制、粘贴、查找、字号这些由窗口壳转发给它（工单 03）。
    pub fn terminal(&self) -> &Entity<TerminalView> {
        &self.terminal
    }

    /// 卡片里终端的焦点：`window.focus(&card.read(cx).focus_handle(), cx)` 把焦点交给这张卡片。
    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// 焦点在不在这张卡片里。
    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
    }

    /// 标签按分屏树告诉卡片：它是不是焦点卡片、是不是放大着、是不是标签里唯一的一张。
    pub fn set_place(&mut self, focused: bool, zoomed: bool, alone: bool, cx: &mut Context<Self>) {
        if (self.focused, self.zoomed, self.alone) != (focused, zoomed, alone) {
            if (self.zoomed, self.alone) != (zoomed, alone) {
                set_card_items(&self.terminal, self.id, zoomed, alone, cx);
            }
            self.focused = focused;
            self.zoomed = zoomed;
            self.alone = alone;
            cx.notify();
        }
    }

    /// 是不是所在标签的焦点卡片。
    pub fn is_focused_card(&self) -> bool {
        self.focused
    }

    /// 是不是放大着。
    pub fn is_zoomed(&self) -> bool {
        self.zoomed
    }

    /// 本地卡片跑的 shell。
    pub fn local_shell(&self) -> Option<&LocalShell> {
        match &self.kind {
            CardKind::Local { shell, .. } => Some(shell),
            CardKind::Remote(_) => None,
        }
    }

    /// 远程卡片连的主机（工单 12）。主机后来被删了也还是它的 id。
    pub fn remote_host(&self) -> Option<HostId> {
        self.remote_part().map(|remote| remote.host().id)
    }

    /// 远程卡片打开（或上次重连）时的主机：主机后来被编辑、删除，卡片照旧用它。
    pub fn remote_host_snapshot(&self) -> Option<&Host> {
        self.remote_part().map(RemoteCard::host)
    }

    /// 远程卡片现在怎么连（工单 16 分屏时用同一个，复用连接）。
    pub fn remote_target(&self) -> Option<&RemoteTarget> {
        self.remote_part().map(RemoteCard::target)
    }

    /// 远程卡片所在的连接：shell 通道开好以后才有（工单 16）。
    pub fn connection(&self) -> Option<ConnectionId> {
        self.remote_part().and_then(RemoteCard::connection)
    }

    /// 远程主机的系统：探测出的，或者主机里记下的。
    pub fn remote_os(&self) -> Option<HostOs> {
        self.remote_part().and_then(RemoteCard::os)
    }

    /// 远程主机的 `uname -n`：探测出来之前是 `None`（工单 17 核对目录报告用）。
    pub fn remote_hostname(&self) -> Option<&str> {
        self.remote_part().and_then(RemoteCard::hostname)
    }

    /// 远程卡片所在连接最近一次的往返延迟。
    pub fn remote_latency(&self) -> Option<crate::backend::Latency> {
        self.remote_part().and_then(RemoteCard::latency)
    }

    /// 远程卡片被动解析出的东西（工单 17）：程序、目录、标题。
    pub fn remote_activity(&self) -> Option<&super::RemoteActivity> {
        self.remote_part().map(|remote| &remote.activity)
    }

    /// 远程卡片的连接状态；本地卡片是 `None`。
    pub fn remote_state(&self, cx: &App) -> Option<RemoteState> {
        self.remote_part()
            .map(|remote| remote.state(&self.terminal.read(cx).lifecycle(cx)))
    }

    fn remote_part(&self) -> Option<&RemoteCard> {
        match &self.kind {
            CardKind::Remote(remote) => Some(remote),
            CardKind::Local { .. } => None,
        }
    }

    /// 收下窗口壳转来的连接事件（工单 12）；返回探测出的系统，窗口壳把它记进主机数据。
    pub fn apply_remote(&mut self, update: RemoteUpdate, cx: &mut Context<Self>) -> Option<HostOs> {
        let probed = matches!(update, RemoteUpdate::Probed(_));
        let (directory, title) = if probed {
            let terminal = self.terminal.read(cx);
            (terminal.directory(cx), terminal.title(cx))
        } else {
            (None, None)
        };
        let CardKind::Remote(remote) = &mut self.kind else {
            return None;
        };
        let os = remote.apply(update);
        // 知道了 `uname -n`：之前带主机名、没信的目录报告和标题重新核对（工单 17）。
        if probed {
            remote.recheck(directory.as_ref(), title.as_deref());
        }
        cx.emit(CardEvent::Changed);
        cx.notify();
        os
    }

    /// 远程卡片能用作 SFTP 标签和分屏初始目录的远程目录（工单 17）：远程报告的；从窗口标题解析的
    /// 只有 `/` 或 `~/` 开头才算。本地卡片和不知道时是 `None`（从主目录开始）。
    pub fn remote_start_directory(&self) -> Option<&str> {
        self.remote_part()
            .and_then(|remote| remote.activity.start_directory.as_deref())
    }

    /// 原地重连：用 `host` 现在的样子和 `target`（编辑过就是新的）建新的传输。
    pub fn reconnect(
        &mut self,
        host: Host,
        target: RemoteTarget,
        factory: SharedTerminalTransportFactory,
        cx: &mut Context<Self>,
    ) {
        let CardKind::Remote(remote) = &mut self.kind else {
            return;
        };
        remote.reconnecting(host, target);
        self.program = None;
        self.terminal.update(cx, |terminal, cx| {
            terminal.restart_with_factory(factory, cx)
        });
        self.refresh_remote_state(cx);
        cx.notify();
    }

    /// 等保险库解锁再连（工单 33）：先不连，终端换成占位的，状态栏写「等待解锁保险库」。
    /// `host` 和 `target` 是解锁后要连的（同 [`Self::reconnect`]）。新卡片和原地重连都经这里；
    /// 窗口壳等用户答了解锁框，解开就 [`Self::reconnect`]，取消就 [`Self::vault_declined`]。
    pub fn wait_for_vault(&mut self, host: Host, target: RemoteTarget, cx: &mut Context<Self>) {
        if self.remote_part().is_none() {
            return;
        }
        self.reconnect(host, target, holding_factory(), cx);
        if let CardKind::Remote(remote) = &mut self.kind {
            remote.hold_for_vault(VaultHold::Waiting);
        }
        self.refresh_remote_state(cx);
        cx.notify();
    }

    /// 用户取消了解锁（工单 33）：卡片显示未连接，原因是「保险库没有解锁」；Enter 原地重连。
    pub fn vault_declined(&mut self, cx: &mut Context<Self>) {
        let CardKind::Remote(remote) = &mut self.kind else {
            return;
        };
        remote.hold_for_vault(VaultHold::Declined);
        // 原因写在断开提示里（`refresh_remote_state`），这里不再追加一行。
        self.terminal
            .update(cx, |terminal, cx| terminal.stop("", cx));
        self.refresh_remote_state(cx);
        cx.notify();
    }

    /// 远程卡片是不是因为保险库停着没连（工单 33）；照常连的、本地卡片是 `None`。
    pub fn vault_hold(&self) -> Option<VaultHold> {
        self.remote_part().and_then(RemoteCard::vault_hold)
    }

    /// 这台主机不再探测系统（工单 34）：卡片记着的主机快照和登录也不探测，重连时用得上。
    pub fn stop_probing(&mut self) {
        if let CardKind::Remote(remote) = &mut self.kind {
            remote.stop_probing();
        }
    }

    /// 断开连接：关掉这张卡片的 shell 通道，卡片留着，可以按 Enter 重连。
    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        let CardKind::Remote(remote) = &mut self.kind else {
            return;
        };
        remote.stopped();
        self.terminal
            .update(cx, |terminal, cx| terminal.stop("", cx));
        self.refresh_remote_state(cx);
        cx.notify();
    }

    /// 头部和标签上的名称：本地卡片是 shell 名（`pwsh`），查到之前是 `…`；远程卡片是主机名。
    pub fn name(&self) -> SharedString {
        match &self.kind {
            CardKind::Local { info, .. } => info
                .as_ref()
                .map(|info| SharedString::from(info.name.clone()))
                .unwrap_or_else(|| "…".into()),
            CardKind::Remote(remote) => remote.host().name.clone(),
        }
    }

    /// shell 现在所在的本机目录：shell 报告的优先，其次是本机读到的、启动时的；都没有是 `None`（主目录）。
    /// 工单 05 在同一个目录里分屏用它。远程卡片总是 `None`（远程目录见 `directory_label`）。
    pub fn directory(&self) -> Option<PathBuf> {
        if self.remote_part().is_some() {
            return None;
        }
        self.reported_directory
            .clone()
            .or_else(|| self.working_directory.clone())
            .or_else(|| self.launch_directory.clone())
    }

    /// 头部和标签上显示的目录：本地卡片主目录下的写成 `~` 开头；远程卡片是远程报告的目录，
    /// 没有时是窗口标题里的（工单 17），都没有时是空的。
    pub fn directory_label(&self) -> String {
        if let Some(remote) = self.remote_part() {
            return remote.directory().unwrap_or_default().to_string();
        }
        let home = home_dir();
        match self.directory() {
            Some(directory) => display_directory(&directory, home.as_deref()),
            None => "~".to_string(),
        }
    }

    /// 名称加目录，标签和头部的无障碍名称都是它（「pwsh ~」）。
    pub fn title(&self) -> String {
        format!("{} {}", self.name(), self.directory_label())
    }

    /// 屏幕上的文字，测试用。
    pub fn screen_text(&self, cx: &App) -> String {
        self.terminal.read(cx).screen_text(cx)
    }

    /// 关掉卡片的 shell。卡片随后由窗口壳移走。
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.terminal
            .update(cx, |terminal, cx| terminal.shutdown(cx));
    }

    /// 顶行：名称和前台在跑的程序。
    fn title_row(&self) -> TitleRow {
        TitleRow {
            remote: self.remote_part().is_some(),
            name: self.name(),
            program: self
                .running_program()
                .map(|program| SharedString::from(program.name)),
        }
    }

    fn status_bar(&self, cx: &App) -> StatusBar {
        let status = self.terminal.read(cx).status(cx);
        let (columns, rows) = (status.columns(), status.rows());
        match &self.kind {
            // 本地卡片：路径是 shell 所在的目录，没有连接状态和系统。
            CardKind::Local { .. } => StatusBar {
                directory: self.directory_label().into(),
                state: StatusState::Local,
                os: None,
                agent: None,
                columns,
                rows,
            },
            CardKind::Remote(remote) => StatusBar {
                directory: remote
                    .directory()
                    .map(SharedString::from)
                    .unwrap_or_else(|| "~".into()),
                state: match remote.state(status.lifecycle()) {
                    RemoteState::Connecting => StatusState::Pending(t!("card.remote.connecting")),
                    RemoteState::WaitingForVault => {
                        StatusState::Pending(t!("card.remote.waiting_for_vault"))
                    }
                    RemoteState::Connected => StatusState::Connected {
                        latency: remote.latency_label(),
                    },
                    RemoteState::Disconnected(reason) => StatusState::Disconnected { reason },
                },
                os: remote.os().map(|os| os.label().into()),
                agent: self.agent.as_ref().map(|_| self.agent_running),
                columns,
                rows,
            },
        }
    }
}

/// 终端右键菜单的最后一组（spec 第 3 节：卡片没有头部，放大、移到新标签、关闭放在这里）。
/// 标签里只有这一张时「移到新标签」变灰。
fn set_card_items(
    terminal: &Entity<TerminalView>,
    id: CardId,
    zoomed: bool,
    alone: bool,
    cx: &mut App,
) {
    let zoom = if zoomed {
        t!("card.restore")
    } else {
        t!("card.zoom")
    };
    terminal.update(cx, |terminal, _| {
        terminal.set_card_items(Rc::new(move |menu| {
            menu.menu(zoom.clone(), Box::new(ZoomCard(id)))
                .keys_of(Box::new(ToggleZoomCard))
                .menu_with_disabled(
                    t!("card.move_to_new_tab"),
                    Box::new(MoveCardToNewTab(id)),
                    alone,
                )
                .menu(t!("card.close"), Box::new(CloseCard(id)))
                .keys_of(Box::new(CloseFocusedCard))
        }))
    });
}

/// 断开时写进终端的几行（26 号「已断开」）：`[连接已断开]` 用终端红色，后面淡色写原因和时间；
/// 下面两行「按 Enter 重新连接」「按 Space 关闭」，Enter、Space 用终端黄色，其余淡色。
/// 颜色都是终端主题的 ANSI 色（红 31、黄 33、淡色用亮黑 90），跟着终端配色变。
fn ended_notice(reason: &str, time: &str) -> String {
    const RED: &str = "\x1b[31m";
    const YELLOW: &str = "\x1b[33m";
    const DIM: &str = "\x1b[90m";
    const RESET: &str = "\x1b[0m";
    let ended = t!("card.remote.ended");
    let press = t!("card.remote.ended_press");
    let reconnect = t!("card.remote.ended_reconnect");
    let close = t!("card.remote.ended_close");
    format!(
        "\r\n\r\n{RED}[{ended}]{RESET} {DIM}{reason} · {time}{RESET}\r\n\
         {DIM}{press} {RESET}{YELLOW}Enter{RESET}{DIM} {reconnect}{RESET}\r\n\
         {DIM}{press} {RESET}{YELLOW}Space{RESET}{DIM} {close}{RESET}"
    )
}

/// 远程终端右键菜单里卡片加的项：「复制」下面的复制 IP（域名时叫复制主机名），最后的重新连接、
/// 断开连接（已经断开时不能点），和它们前面的打开 SFTP（工单 20）。
fn remote_menu_items(id: CardId, host: &Host, agent: Rc<Cell<bool>>) -> TerminalMenuItems {
    let copy_label = if host.address_is_ip() {
        t!("card.menu.copy_ip")
    } else {
        t!("card.menu.copy_host_name")
    };
    TerminalMenuItems {
        copy: Rc::new(move |menu, _| menu.menu(copy_label.clone(), Box::new(CopyCardAddress(id)))),
        commands: Rc::new(move |menu, lifecycle| {
            // 交给 Agent / 收回（Agent CLI 工单 06）。
            let menu = if agent.get() {
                menu.menu(t!("card.menu.take_back"), Box::new(TakeCardBack(id)))
            } else {
                menu.menu(t!("card.menu.hand_to_agent"), Box::new(HandCardToAgent(id)))
            };
            menu.menu(t!("card.menu.open_sftp"), Box::new(OpenCardSftp(id)))
                .menu(t!("card.menu.reconnect"), Box::new(ReconnectCard(id)))
                // 断开连接是红字（26 号 `.mi.danger`）。
                .danger(
                    t!("card.menu.disconnect"),
                    Box::new(DisconnectCard(id)),
                    !lifecycle.accepts_input(),
                )
        }),
    }
}

/// 还没连上时卡片正文左上角的一行（原型「用到秘密时：解锁保险库」）：「正在连接 …（经 …）…」，
/// 淡色、终端字体。终端连上开始输出就不画了。
fn render_connecting(
    id: CardId,
    line: SharedString,
    tokens: &Tokens,
    cx: &App,
) -> impl IntoElement {
    let font = TerminalFont::current(cx);
    div()
        .id(id.element("card-connecting"))
        .test_support()
        .aria_label(line.clone())
        .absolute()
        .top(px(14.))
        .left(px(20.))
        .right(px(20.))
        .truncate()
        .when_some(font.family, |line, family| line.font_family(family))
        .text_size(font.size)
        .text_color(tokens.st_dim)
        .child(line)
}

impl Render for TerminalCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        // 顶行、状态栏的深浅看终端有没有焦点（不只是标签的焦点卡片）：和光标是实心还是空心一致。
        let focused = self.focused && self.focus.contains_focused(window, cx);
        let id = self.id;
        let place = CardPlace {
            focused,
            active: self.focused,
            zoomed: self.zoomed,
            alone: self.alone,
        };
        let width = self.width.get();
        let paint = Paint {
            target: &self.focus,
            window,
            tokens: &tokens,
            scale,
        };
        let title = self.title_row();
        let status = self.status_bar(cx);
        let state = self.remote_state(cx);
        let terminal = self.terminal.clone();
        let focus = self.focus.clone();
        let terminal_margin = window.rem_size() * 0.5;
        div()
            .id(id.element("card"))
            .test_support()
            .aria_label(self.title())
            .group("card")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .relative()
            .bg(tokens.content)
            // 点窗格的任何地方（顶行、状态栏）都把焦点交给终端。
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx)
            })
            .child(render_title_row(id, &title, place, width, &paint))
            .child(
                // 文字离顶行 8、左右 20。终端自己四周留 0.5 rem（rem 跟着界面字号），这里补上差的那些。
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .pt(if TERMINAL_TOP > terminal_margin {
                        TERMINAL_TOP - terminal_margin
                    } else {
                        px(0.)
                    })
                    .px(TERMINAL_SIDE - terminal_margin)
                    .child(div().flex().flex_1().min_w_0().child(terminal))
                    .when_some(state, |body, state| match state {
                        RemoteState::Connecting | RemoteState::WaitingForVault => {
                            body.children(self.remote_part().map(|remote| {
                                render_connecting(id, remote.connecting_line(), &tokens, cx)
                            }))
                        }
                        RemoteState::Connected | RemoteState::Disconnected(_) => body,
                    }),
            )
            .child(render_status_bar(id, &status, focused, width, &paint))
            .child(measure_width(self.width.clone(), cx.entity_id()))
    }
}
