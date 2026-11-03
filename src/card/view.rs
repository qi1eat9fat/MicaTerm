//! `TerminalCard`：一张卡片，持有它的终端，画头部、终端和状态栏。
//! 卡片外壳照 Rex 的卡片新写；行为参考 since2006/shell-rs @ b2a307a 的
//! `src/terminal/local_terminal_panel.rs`（目录事件的转发）。

use std::path::PathBuf;

use gpui_kit::*;
use qishell_terminal::{
    LocalLaunch, LocalShell, LocalShellInfo, SharedLocalTerminalTransportProvider, TerminalEvent,
    TerminalView,
};

use super::CardId;
use super::directory::{display_directory, home_dir, reported_path};
use super::header::render_header;
use super::status_bar::{StatusBar, render_status_bar};
use crate::i18n::t;
use crate::theme::{CARD_RADIUS, Tokens};

/// 卡片告诉持有它的人（窗口壳）的事。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardEvent {
    /// 名称、目录变了：标签跟着重画。
    Changed,
}

/// 卡片里跑的是什么。M2 加远程（工单 12）。
enum CardKind {
    /// 本地 shell：名称和版本在后台查，查到前是 `None`。
    Local {
        shell: LocalShell,
        info: Option<LocalShellInfo>,
    },
}

pub struct TerminalCard {
    id: CardId,
    kind: CardKind,
    terminal: Entity<TerminalView>,
    /// 终端的焦点：焦点在它里面就是这张卡片聚焦。
    focus: FocusHandle,
    /// 窗口壳的焦点：按钮经它派发 action。
    target: FocusHandle,
    /// 启动时的目录；没有就是主目录。
    launch_directory: Option<PathBuf>,
    /// shell 自己报告的目录（OSC 7 等），优先。
    reported_directory: Option<PathBuf>,
    /// 本机说 shell 在的目录。
    working_directory: Option<PathBuf>,
    _info_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CardEvent> for TerminalCard {}

impl TerminalCard {
    /// 一张本地卡片：按 `launch` 起 shell，传输由 `provider` 建（测试注入假的）。
    /// `target` 是窗口壳的焦点，卡片上的按钮经它派发 action。
    pub fn local(
        id: CardId,
        launch: LocalLaunch,
        provider: SharedLocalTerminalTransportProvider,
        target: FocusHandle,
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
        let focus = terminal.read(cx).focus_handle();
        let subscriptions = vec![
            cx.observe(&terminal, |_, _, cx| cx.notify()),
            cx.subscribe(&terminal, Self::on_terminal_event),
            // 聚焦与否换描边和按钮：终端拿到或失去焦点时重画卡片。
            cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
            cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
        ];
        // shell 名和版本要看文件系统，放到后台查。
        let shell = launch.shell.clone();
        let info_task = cx.spawn(async move |this, cx| {
            let query = shell.clone();
            let info = cx
                .background_spawn(async move { provider.shell_info(&query) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let CardKind::Local { info: slot, .. } = &mut this.kind;
                *slot = Some(info);
                cx.emit(CardEvent::Changed);
                cx.notify();
            });
        });
        Self {
            id,
            kind: CardKind::Local {
                shell: launch.shell,
                info: None,
            },
            terminal,
            focus,
            target,
            launch_directory: launch.directory,
            reported_directory: None,
            working_directory: None,
            _info_task: info_task,
            _subscriptions: subscriptions,
        }
    }

    fn on_terminal_event(
        &mut self,
        _: Entity<TerminalView>,
        event: &TerminalEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalEvent::DirectoryChanged(directory) => {
                self.reported_directory = Some(reported_path(&directory.path));
            }
            TerminalEvent::WorkingDirectory(folder) => {
                self.working_directory = Some(folder.clone());
            }
            _ => return,
        }
        cx.emit(CardEvent::Changed);
        cx.notify();
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

    /// 本地卡片跑的 shell。
    pub fn local_shell(&self) -> Option<&LocalShell> {
        match &self.kind {
            CardKind::Local { shell, .. } => Some(shell),
        }
    }

    /// 头部和标签上的名称：本地卡片是 shell 名（`pwsh`），查到之前是 `…`。
    pub fn name(&self) -> SharedString {
        match &self.kind {
            CardKind::Local { info, .. } => info
                .as_ref()
                .map(|info| SharedString::from(info.name.clone()))
                .unwrap_or_else(|| "…".into()),
        }
    }

    /// shell 现在所在的目录：shell 报告的优先，其次是本机读到的、启动时的；都没有是 `None`（主目录）。
    /// 工单 05 在同一个目录里分屏用它。
    pub fn directory(&self) -> Option<PathBuf> {
        self.reported_directory
            .clone()
            .or_else(|| self.working_directory.clone())
            .or_else(|| self.launch_directory.clone())
    }

    /// 头部和标签上显示的目录：主目录下的写成 `~` 开头。
    pub fn directory_label(&self) -> String {
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

    fn status_bar(&self, cx: &App) -> StatusBar {
        let status = self.terminal.read(cx).status(cx);
        match &self.kind {
            CardKind::Local { info, .. } => StatusBar {
                state: t!("card.local.title"),
                detail: info.as_ref().map(|info| info.label().to_string().into()),
                columns: status.columns(),
                rows: status.rows(),
            },
        }
    }
}

impl Render for TerminalCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let focused = self.is_focused(window, cx);
        let id = self.id;
        let name = self.name();
        let directory = self.directory_label();
        let status = self.status_bar(cx);
        let terminal = self.terminal.clone();
        let focus = self.focus.clone();
        div()
            .id(id.element("card"))
            .test_support()
            .group("card")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .rounded(CARD_RADIUS)
            .bg(if focused {
                tokens.card_focused
            } else {
                tokens.card
            })
            .border_1()
            .border_color(if focused {
                tokens.card_focused_border
            } else {
                tokens.card_border
            })
            .shadow(vec![BoxShadow {
                color: if focused {
                    tokens.shadow_focused
                } else {
                    tokens.shadow
                },
                offset: point(px(0.), px(6.)),
                blur_radius: px(22.),
                spread_radius: px(-2.),
                inset: false,
            }])
            // 点卡片的任何地方（头部、状态栏）都把焦点交给终端。
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx)
            })
            .child(render_header(
                id,
                name,
                directory,
                focused,
                &self.target,
                &tokens,
            ))
            .child(
                // 圆角只按矩形裁剪：内容四周留边。
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(4.))
                    .pb(px(4.))
                    .child(terminal),
            )
            .child(render_status_bar(id, &status, &tokens))
    }
}
