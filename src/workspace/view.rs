//! `Workspace`：窗口壳实体。
//!
//! 唯一的 UI 级 seam 是注入式构造 `Workspace::new_with_services`：生产代码传真实服务，
//! 无头 UI 测试传假实现（`tests/ui/support/mod.rs` 的 `fake_services`）。

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use micaterm_hosts::secrets::{InMemorySecretStore, SharedSecretStore};
use micaterm_terminal::{LocalPtyTransportProvider, SharedLocalTerminalTransportProvider};

use super::handlers;
use super::host_sidebar::{render_collapsed_host_sidebar, render_host_sidebar};
use super::notices::NoticeTimes;
use super::remote::PromptQueue;
use super::right_sidebar::{RightPanel, RightPanelWidths};
use super::title_bar::{TitleBarState, render_title_bar};
use crate::app::PRODUCT_NAME;
use crate::backend::sftp::{
    SharedLocalDirectoryProvider, SharedSftpTransportProvider, SshSftpTransportProvider,
    SystemLocalDirectoryProvider,
};
use crate::backend::{
    SharedConnectionTester, SharedRemoteTerminals, SshConnectionTester, SshRemoteTerminals,
};
use crate::card::{CardId, StatusDot, TerminalCard};
use crate::cli::{CliIntegration, CliService, IntegrationPaths, SshCliBackend};
use crate::credential::CredentialPanel;
use crate::host::{HostData, HostList, HostStore, HostStoreEvent};
use crate::i18n::t;
use crate::settings::{SettingsStore, SettingsStoreEvent, WindowBackdrop, WindowSettings, apply};
use crate::snippets::SnippetPanel;
use crate::split::NodeId;
use crate::tabs::{Tab, TabId, TabList, TabRail, TabSnapshot, TerminalTab};
use crate::theme::{Tokens, TypeScale, size, ui_font};
use crate::vault::{Vault, VaultStore, VaultStoreEvent};

/// 窗口壳用到的外部服务。生产代码用 `WorkspaceServices::new()` 建真实的，
/// 测试用 `tests/ui/support` 的 `fake_services()` 建假的。
///
/// 后续工单每加一项服务（主机数据库、保险库、SFTP……），就在这里加一个字段，
/// 同时在 `fake_services()` 里给它一个假实现。
pub struct WorkspaceServices {
    /// 本地终端的传输：生产是本机 PTY，测试注入假的。
    pub local_terminals: SharedLocalTerminalTransportProvider,
    /// 主机、凭据、命令片段：生产是数据目录里的 `micaterm.db`，测试是内存里的（`HostData::empty`、`seed`）。
    /// 窗口壳构造时把它取走、包成 `HostStore` 实体，之后这里只剩一份空的。
    pub hosts: HostData,
    /// 数据库打不开时给用户看的原因（这次运行用的是只在内存里的空数据，改动不会保存）。
    /// 窗口壳打开后弹出错误通知（工单 11）。
    pub hosts_problem: Option<SharedString>,
    /// 密码、口令、私钥存在哪里（工单 11）：主机数据把秘密写到这里。
    /// 生产是数据目录里的保险库（见下面的 `vault`，打不开时是只在内存里的存储）；测试注入 `InMemorySecretStore`。
    pub secrets: SharedSecretStore,
    /// 远程终端（工单 12）：生产是 backend 的 SSH 连接表（按主机共享连接），测试注入假的。
    pub remote_terminals: SharedRemoteTerminals,
    /// 主机对话框的「测试连接」（工单 12）：生产真的登录一次，测试注入假的。
    pub connection_tester: SharedConnectionTester,
    /// 数据目录里的保险库（工单 13）：主机栏底部的锁、设置 › 安全、第一次保存秘密时的设置主密码框、
    /// 用到秘密时的解锁小卡片都看它。有保险库时 `secrets` 和连接、测试连接读写的就是它
    /// （生产 `WorkspaceServices::new` 三者一起换）；`None` 时秘密只在 `secrets` 里，不弹任何保险库的框
    /// （测试的默认，和生产上保险库打不开时）。测试注入 `tests/ui/support/vault.rs` 的保险库。
    pub vault: Option<std::sync::Arc<Vault>>,
    /// 保险库打不开时给用户看的原因（这次运行的秘密只在内存里）。窗口壳打开后弹出错误通知。
    pub vault_problem: Option<SharedString>,
    /// SFTP 标签的连接（工单 20）：生产真的登录，测试注入假的。
    pub sftp: SharedSftpTransportProvider,
    /// SFTP 标签本地一侧的文件（工单 20）：生产是这台电脑，测试注入假的，不碰真实磁盘。
    pub local_files: SharedLocalDirectoryProvider,
    /// 续传记录没有地方放时给用户看的原因（这次运行的传输不能续传）。窗口壳打开后弹出错误通知。
    pub sftp_problem: Option<SharedString>,
    /// 数据目录（工单 26）：设置 › 应用显示它，窗口的大小和位置记在它下面的 `window.json`。
    /// 生产是 `crate::app::data_dir()`，取不到时 `None`（不记窗口、设置里说没有数据目录）；
    /// 测试默认 `None`，要测记住窗口时给一个临时目录。
    pub data_dir: Option<std::path::PathBuf>,
    /// 外部 CLI（Agent CLI 工单 01）：生产在数据目录对应的端点上听（同一个数据目录已有 MicaTerm
    /// 在听时让给它）；测试默认 `CliService::Off`，要测时在临时端点上听。
    pub cli: CliService,
    /// 命令装进 PATH、Skill 装给 Agent 的位置（工单 05）：生产是用户目录和程序旁边；
    /// 测试默认 `None`（不碰真的用户目录），要测时给临时目录。
    pub cli_install: Option<IntegrationPaths>,
    /// 自动检测 Agent 命令时找的 `PATH`（Agent CLI 工单 07）：生产是 `None`（这个进程的）；
    /// 测试给空的，结果不看跑测试的机器装没装 claude。
    pub agent_path: Option<std::ffi::OsString>,
    /// Claude Code 放对话记录的目录（启动框看文件夹里有没有上次的对话）：生产是
    /// `CLAUDE_CONFIG_DIR` 或 `~/.claude`；测试默认 `None`（哪里都没聊过），要测时给临时目录。
    pub agent_history: Option<std::path::PathBuf>,
}

impl WorkspaceServices {
    /// 真实的服务。
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let data_dir = crate::app::data_dir().ok();
        let (hosts, hosts_problem) = match crate::host::load_hosts() {
            Ok(hosts) => (hosts, None),
            Err(problem) => (HostData::empty(), Some(problem)),
        };
        let (vault, vault_problem) = open_vault();
        let secrets: SharedSecretStore = match &vault {
            Some(vault) => vault.clone(),
            None => std::sync::Arc::new(InMemorySecretStore::default()),
        };
        // 连接读秘密经同一个存储（保险库，打不开时是内存里的）；主机密钥记在数据目录的 known_hosts。
        let known_hosts = crate::app::known_hosts_path()
            .unwrap_or_else(|_| std::path::PathBuf::from(crate::app::KNOWN_HOSTS_FILE));
        let source = micaterm_hosts::secrets::StoreSource::shared(secrets.clone());
        let (sftp, sftp_problem) = sftp_provider(
            micaterm_ssh::SshConnector::new(known_hosts.clone(), source.clone()),
            crate::app::upload_resume_dir(),
            crate::app::download_resume_dir(),
        );
        // 外部 CLI 的 `exec` 和传输也经同一份 known_hosts 和保险库登录（Agent CLI 工单 02、03），
        // 传输和 SFTP 标签用同一个引擎、同一处续传记录。
        let sftp: SharedSftpTransportProvider = std::sync::Arc::new(sftp);
        let cli = CliService::start(
            data_dir.as_deref(),
            std::sync::Arc::new(SshCliBackend::new(
                micaterm_ssh::SshConnector::new(known_hosts.clone(), source.clone()),
                sftp.clone(),
            )),
        );
        Self {
            local_terminals: std::sync::Arc::new(LocalPtyTransportProvider),
            hosts,
            hosts_problem,
            remote_terminals: std::sync::Arc::new(SshRemoteTerminals::new(
                known_hosts.clone(),
                source.clone(),
            )),
            connection_tester: std::sync::Arc::new(SshConnectionTester::new(known_hosts, source)),
            secrets,
            vault,
            vault_problem,
            sftp,
            local_files: std::sync::Arc::new(SystemLocalDirectoryProvider),
            sftp_problem,
            cli,
            cli_install: IntegrationPaths::system(),
            agent_path: None,
            agent_history: crate::agent::claude_config_dir(),
            data_dir,
        }
    }
}

/// 建 SFTP 连接。续传记录只放在数据目录里（ADR 0003）：取不到数据目录就不记，告诉用户，
/// 不要静默写到当前工作目录。
fn sftp_provider(
    connector: micaterm_ssh::SshConnector,
    upload: std::io::Result<std::path::PathBuf>,
    download: std::io::Result<std::path::PathBuf>,
) -> (SshSftpTransportProvider, Option<SharedString>) {
    match (upload, download) {
        (Ok(upload), Ok(download)) => (
            SshSftpTransportProvider::new(connector, upload, download),
            None,
        ),
        (Err(error), _) | (_, Err(error)) => (
            SshSftpTransportProvider::without_resume_records(connector),
            Some(t!("sftp_tab.connection.no_resume_folder", error = error)),
        ),
    }
}

/// 打开数据目录里的保险库。读文件、读钥匙串会阻塞（macOS 上可能弹授权框），放到另一个线程上，
/// 和读数据库一样在窗口出来之前做完。打不开时返回原因，这次运行的秘密只放在内存里。
fn open_vault() -> (Option<std::sync::Arc<Vault>>, Option<SharedString>) {
    let opened = std::thread::spawn(crate::secrets::open_vault)
        .join()
        .unwrap_or_else(|_| Err(anyhow::anyhow!("opening the vault panicked")));
    match opened {
        Ok(vault) => (Some(vault), None),
        Err(error) => {
            let problem = t!("vault.error.open_failed", error = format!("{error:#}"));
            (None, Some(problem))
        }
    }
}

/// 窗口壳。
pub struct Workspace {
    /// 窗口壳根节点的焦点：按钮一律经它派发 action，焦点为空时也能送到这里的处理器。
    pub(super) focus_handle: FocusHandle,
    /// 新标签页的焦点：没有标签时由它持有焦点，快捷键才有焦点路径可走。
    pub(super) new_tab_focus: FocusHandle,
    pub(super) settings: Entity<SettingsStore>,
    pub(super) services: WorkspaceServices,
    /// 主机栏是否显示。
    pub(super) host_sidebar_open: bool,
    /// 标签（终端、SFTP、编辑器），从左到右，和在前的那个。
    pub(super) tabs: TabList<Tab>,
    /// 下一张卡片的 id：从 1 起，不复用。
    pub(super) next_card_id: u64,
    /// 下一个 SFTP 标签的 id：从 1 起，不复用。
    pub(super) next_sftp_id: u64,
    /// 关闭标签前问的那个框（`request_close_tabs`），开着时不再叠一个。
    pub(super) close_dialog: Option<crate::ui::DialogLayer>,
    /// 开着的退出询问框，不再叠一个。
    pub(super) quit_dialog: Option<crate::ui::DialogLayer>,
    /// 关标签或退出正等着写回结束。
    pub(super) save_wait: Option<super::handlers::SaveWait>,
    /// 每张卡片每类通知上一次发出的时间，用来限频（`notices`）。
    pub(super) notice_times: NoticeTimes,
    /// 标签轨道的滚动位置。
    pub(super) tab_rail: TabRail,
    /// 窗口在不在前：不在前时哪张卡片用户都看不到（橙点，`notices`）。
    pub(super) window_active: bool,
    /// 主机数据（工单 11），主机栏和各个对话框都观察它。
    pub(super) hosts: Entity<HostStore>,
    /// 主机栏里的搜索框和主机列表（工单 11）。
    pub(super) host_list: Entity<HostList>,
    /// 右侧栏展开的面板。
    pub(super) right_panel: Option<RightPanel>,
    /// 右侧各面板拖到的宽度。
    pub(super) right_panel_widths: RightPanelWidths,
    /// 右侧 Agent 面板里跑着的 Agent（Agent CLI 工单 07）：收起面板时还在。
    pub(super) agent: Option<crate::agent::AgentPanel>,
    /// Agent 们最后看到的屏幕（工单 09，屏幕规则等它停下来才判）。
    pub(super) agent_screens: super::agent_state::AgentScreens,
    /// 命令片段面板（工单 19）。
    pub(super) snippet_panel: SnippetPanel,
    /// 连接表的问题，一次问一个（工单 12）。
    pub(super) prompts: PromptQueue,
    /// 解锁框开着（或者等这次更新结束就弹）：等保险库的远程卡片共用这一个框（工单 33）。
    /// 哪些卡片在等只看卡片自己的 `VaultHold::Waiting`。
    pub(super) vault_unlock_asked: bool,
    /// 保险库（工单 13），也装成了全局量，对话框和设置页经 `VaultStore::global` 找到它。
    pub(super) vault: Entity<VaultStore>,
    /// 凭据面板（工单 14）。
    pub(super) credential_panel: Entity<CredentialPanel>,
    /// 外部 CLI 的命令和 Skill 装没装（工单 05），设置 › Agent 显示它。
    pub(super) cli_integration: Entity<CliIntegration>,
    /// 一直收连接表的事件（工单 12）。
    _remote_events: Task<()>,
    /// 等窗口停稳再写 `window.json` 的那一次（工单 26）；窗口又动了就换一个新的。
    pub(super) window_state_save: Option<Task<()>>,
    pub(super) subscriptions: Vec<Subscription>,
}

impl Workspace {
    /// 生产环境的窗口壳：真实的服务。
    pub fn new(
        settings: Entity<SettingsStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_services(settings, WorkspaceServices::new(), window, cx)
    }

    /// 注入式构造：窗口壳用 `services` 里的服务，而不是自己去建。
    pub fn new_with_services(
        settings: Entity<SettingsStore>,
        mut services: WorkspaceServices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        // 关窗口、退出都先问：有未保存的修改、未完成的传输时（工单 23）。
        let this = cx.weak_entity();
        crate::app::set_quit_guard(
            window.window_handle(),
            move |window, cx| {
                this.update(cx, |this, cx| this.ask_before_quit(window, cx))
                    .unwrap_or(false)
            },
            cx,
        );
        window.on_window_should_close(cx, |window, cx| !crate::app::quit_held_back(window, cx));
        let (hosts, host_list, host_subscriptions) =
            Self::open_hosts(&mut services, &focus_handle, window, cx);
        let snippet_panel = SnippetPanel::new();
        let mut subscriptions = vec![
            // 设置写不进文件时告诉用户，不静默吞掉。
            cx.subscribe_in(
                &settings,
                window,
                |_, _, event: &SettingsStoreEvent, window, cx| match event {
                    SettingsStoreEvent::PersistFailed(message) => {
                        window.push_notification(Notification::error(message.clone()), cx);
                    }
                },
            ),
            // 应用外观、界面字号、终端主题、字体和交互跟着设置走：开着的终端立刻变。
            cx.observe_in(&settings, window, |_, settings, window, cx| {
                apply(&settings.read(cx).settings(), window, cx);
            }),
            // 跟随系统时，系统换深浅色，窗口跟着换。
            cx.observe_window_appearance(window, |this, window, cx| {
                apply(&this.settings.read(cx).settings(), window, cx);
            }),
            // 窗口回到前面：看得见的卡片熄掉橙点。
            cx.observe_window_activation(window, |this, window, cx| {
                this.window_active = window.is_window_active();
                this.see_cards(cx);
                this.see_agent_panel();
                cx.notify();
            }),
            // 切标签、放大还原、关卡片……窗口壳一变，就看看哪些卡片又看得见了。
            cx.observe_self(|this, cx| this.see_cards(cx)),
        ];
        subscriptions.extend(host_subscriptions);
        let (vault, vault_subscriptions) = Self::open_vault(&mut services, window, cx);
        subscriptions.extend(vault_subscriptions);
        apply(&settings.read(cx).settings(), window, cx);
        // 点终端的系统通知，回到那张卡片。
        Self::listen_for_notice_clicks(window, cx);
        let new_tab_focus = cx.focus_handle();
        // 启动时没有标签：新标签页持有焦点。
        new_tab_focus.focus(window, cx);
        let remote_events = Self::listen_for_remote_events(&services, window, cx);
        let credential_panel = {
            let (hosts, target) = (hosts.clone(), focus_handle.clone());
            cx.new(|cx| CredentialPanel::new(hosts, target, cx))
        };
        // 命令和 Skill：启动时把旧版本留下的副本更新掉，再看一遍装没装（工单 05）。
        let cli_integration = {
            let paths = services.cli_install.clone();
            cx.new(|cx| {
                let mut integration = CliIntegration::new(paths);
                integration.update_outdated(cx);
                integration
            })
        };
        let window_state_path = services
            .data_dir
            .as_ref()
            .map(|dir| crate::app::window_state_path(dir));
        let mut this = Self {
            focus_handle,
            new_tab_focus,
            settings,
            services,
            host_sidebar_open: true,
            tabs: TabList::default(),
            next_card_id: 1,
            next_sftp_id: 1,
            close_dialog: None,
            quit_dialog: None,
            save_wait: None,
            notice_times: NoticeTimes::default(),
            tab_rail: TabRail::default(),
            window_active: window.is_window_active(),
            hosts,
            host_list,
            right_panel: None,
            right_panel_widths: RightPanelWidths::new(cx),
            agent: None,
            agent_screens: Default::default(),
            snippet_panel,
            prompts: PromptQueue::default(),
            vault_unlock_asked: false,
            vault,
            credential_panel,
            cli_integration,
            _remote_events: remote_events,
            window_state_save: None,
            subscriptions,
        };
        // 外部 CLI 跟着设置的开关、主机和保险库走（Agent CLI 工单 01）。
        let cli = this.watch_cli(&this.hosts.clone(), &this.vault.clone(), window, cx);
        this.subscriptions.extend(cli);
        // Agent 的状态：看面板和本地卡片里 Agent 的屏幕（工单 09）。
        this.watch_agents(window, cx);
        // 记住窗口大小和位置（工单 26）：没有数据目录就不记。
        if let Some(path) = window_state_path {
            this.remember_window(path, window, cx);
        }
        this
    }

    /// 把注入的主机数据建成 `HostStore` 和主机栏的列表；写不进数据库、保险库时弹出错误通知。
    /// 数据库打不开时用只在内存里的空数据，并告诉用户这次的改动不会保存。
    fn open_hosts(
        services: &mut WorkspaceServices,
        target: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<HostStore>, Entity<HostList>, Vec<Subscription>) {
        let data = std::mem::replace(&mut services.hosts, HostData::empty());
        if let Some(problem) = services.hosts_problem.take() {
            // 窗口还没挂上 `Root`，等下一帧再弹。
            window.defer(cx, move |window, cx| {
                window.push_notification(Notification::error(problem), cx);
            });
        }
        let secrets = services.secrets.clone();
        let hosts = cx.new(|_| HostStore::new(data.with_secrets(secrets)));
        let failures = cx.subscribe_in(
            &hosts,
            window,
            |_, _, event: &HostStoreEvent, window, cx| match event {
                HostStoreEvent::PersistFailed(message) => {
                    window.push_notification(Notification::error(message.clone()), cx);
                }
            },
        );
        let host_list = cx.new(|cx| HostList::new(hosts.clone(), target.clone(), window, cx));
        // 主机栏标题里的「主机 N」跟着变。
        let changes = cx.observe(&hosts, |_, _, cx| cx.notify());
        (hosts, host_list, vec![failures, changes])
    }

    /// 把注入的保险库包成 `VaultStore`、装成全局量；保险库操作没成、保险库打不开时弹出错误通知。
    /// 主机栏底部的锁按钮跟着保险库的状态变。
    fn open_vault(
        services: &mut WorkspaceServices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<VaultStore>, Vec<Subscription>) {
        for problem in [services.vault_problem.take(), services.sftp_problem.take()]
            .into_iter()
            .flatten()
        {
            window.defer(cx, move |window, cx| {
                window.push_notification(Notification::error(problem), cx);
            });
        }
        let vault = cx.new(|_| VaultStore::new(services.vault.clone()));
        VaultStore::install(&vault, cx);
        let failures = cx.subscribe_in(
            &vault,
            window,
            |_, _, event: &VaultStoreEvent, window, cx| match event {
                VaultStoreEvent::Failed(message) => {
                    window.push_notification(Notification::error(message.clone()), cx);
                }
            },
        );
        let changes = cx.observe(&vault, |_, _, cx| cx.notify());
        (vault, vec![failures, changes])
    }

    /// 保险库，测试用：看它的状态。
    pub fn vault(&self) -> &Entity<VaultStore> {
        &self.vault
    }

    /// 主机栏里选中的主机。
    pub fn selected_host(&self, cx: &App) -> Option<micaterm_hosts::HostId> {
        self.host_list.read(cx).selected_host()
    }

    /// 主机栏搜索时高亮的主机。
    pub fn highlighted_host(&self, cx: &App) -> Option<micaterm_hosts::HostId> {
        self.host_list.read(cx).highlighted_host(cx)
    }

    /// 主机栏列出的主机，从上到下（搜索时只有匹配的）。
    pub fn listed_hosts(&self, cx: &App) -> Vec<micaterm_hosts::HostId> {
        self.host_list.read(cx).listed_hosts(cx)
    }

    /// 设置。
    pub fn settings(&self) -> &Entity<SettingsStore> {
        &self.settings
    }

    /// 主机栏是否显示。
    pub fn is_host_sidebar_open(&self) -> bool {
        self.host_sidebar_open
    }

    /// 主机、凭据、命令片段。
    pub fn hosts(&self) -> &Entity<HostStore> {
        &self.hosts
    }

    /// 右侧栏展开的面板。
    pub fn right_panel(&self) -> Option<RightPanel> {
        self.right_panel
    }

    /// 标签，从左到右。
    pub fn tab_ids(&self) -> Vec<TabId> {
        self.tabs.ids()
    }

    /// 在前的标签。
    pub fn active_tab(&self) -> Option<TabId> {
        self.tabs.active()
    }

    /// 在前的标签，是终端标签时。卡片的处理器都经它，在 SFTP、编辑器标签里自然什么都不做。
    pub(super) fn active_terminal_tab(&self) -> Option<&TerminalTab> {
        self.tabs.active_content().and_then(Tab::as_terminal)
    }

    /// 全部终端标签，从左到右。
    pub(super) fn terminal_tabs(&self) -> impl Iterator<Item = &TerminalTab> {
        self.tabs.iter().filter_map(|(_, tab)| tab.as_terminal())
    }

    /// 一个标签里的卡片。SFTP 标签和编辑器标签没有卡片。
    pub fn tab_cards(&self, tab: TabId) -> Vec<Entity<TerminalCard>> {
        self.tabs
            .get(tab)
            .and_then(Tab::as_terminal)
            .map(|tab| tab.cards().cloned().collect())
            .unwrap_or_default()
    }

    /// id 为 `card` 的卡片，在哪个标签里都找。
    pub fn card(&self, card: CardId, cx: &App) -> Option<Entity<TerminalCard>> {
        self.terminal_tabs()
            .flat_map(|tab| tab.cards())
            .find(|entity| entity.read(cx).id() == card)
            .cloned()
    }

    /// 卡片终端屏幕上的文字，测试用。
    pub fn card_screen_text(&self, card: CardId, cx: &App) -> Option<String> {
        self.card(card, cx)
            .map(|entity| entity.read(cx).screen_text(cx))
    }

    /// 在前那个标签的焦点卡片。SFTP 标签、编辑器标签和新标签页没有焦点卡片。
    pub fn focused_card(&self, cx: &App) -> Option<CardId> {
        self.active_terminal_tab()
            .map(|tab| tab.focused_card().read(cx).id())
    }

    /// 在前那个标签里放大着的卡片。
    pub fn zoomed_card(&self) -> Option<CardId> {
        self.active_terminal_tab()
            .and_then(|tab| tab.tree().zoomed())
    }

    /// 在前那个标签里分隔线 `node` 的比例（左边或上边占的份额）。
    pub fn divider_ratio(&self, node: NodeId) -> Option<f32> {
        self.active_terminal_tab()
            .and_then(|tab| tab.tree().ratio(node))
    }

    /// 卡片图块上的程序名（「Neovim」；shell 在提示符下是 shell 名）。
    pub fn card_tile(&self, card: CardId, cx: &App) -> Option<String> {
        self.card(card, cx)
            .map(|entity| entity.read(cx).tile_program().name)
    }

    /// 卡片的图块是不是灰的（远程卡片没连着，工单 12）。
    pub fn card_tile_off(&self, card: CardId, cx: &App) -> Option<bool> {
        self.card(card, cx)
            .map(|entity| entity.read(cx).tile_program().off)
    }

    /// 标签代表的程序名（终端标签是焦点卡片的）。
    pub fn tab_tile(&self, tab: TabId, cx: &App) -> Option<String> {
        self.tabs
            .get(tab)
            .and_then(|tab| tab.tile(cx))
            .map(|program| program.name)
    }

    /// 卡片的状态点（标签上的点由它汇总）。
    pub fn card_dot(&self, card: CardId, cx: &App) -> Option<StatusDot> {
        self.card(card, cx)
            .map(|entity| entity.read(cx).status_dot(cx))
    }

    /// 标签上的状态点。
    pub fn tab_dot(&self, tab: TabId, cx: &App) -> Option<StatusDot> {
        self.tabs.get(tab).map(|tab| tab.dot(cx))
    }

    /// 还留着通知限频记录的卡片，从小到大，测试用：关掉的卡片不该在里面。
    pub fn notified_cards(&self) -> Vec<CardId> {
        self.notice_times.cards()
    }

    /// 标签轨道要画的快照：渲染函数不读实体。
    fn tab_snapshots(&self, cx: &App) -> Vec<TabSnapshot> {
        let active = self.tabs.active();
        self.tabs
            .iter()
            .map(|(id, tab)| tab.snapshot(id, active == Some(id), cx))
            .collect()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let target = self.focus_handle.clone();
        // 有标签时是在前那个标签的内容，没有时是新标签页。
        let content =
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .map(|content| match self.tabs.active_content() {
                    Some(tab) => content.child(tab.content(cx)),
                    None => content.child(self.render_new_tab_page(&target, &tokens, window, cx)),
                });
        // 展开了面板时，内容区和面板之间是一条能左右拖的分隔线。
        let body = match self.right_panel {
            Some(panel) => {
                let panel_element = match panel {
                    RightPanel::Snippets => self.render_snippet_panel(&tokens, cx),
                    RightPanel::Credentials => self.credential_panel.clone().into_any_element(),
                    RightPanel::Agent => match &self.agent {
                        Some(agent) => agent.render(&target, &tokens),
                        None => div().into_any_element(),
                    },
                };
                self.right_panel_widths
                    .with_panel(panel, content, panel_element)
            }
            None => content.into_any_element(),
        };
        // 主区域：内容底色，主机栏开着时左边一条细线（26 号 `.flush .panel`）。
        let main = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .bg(tokens.content)
            .when(self.host_sidebar_open, |main| {
                main.border_l(size::LINE).border_color(tokens.line)
            })
            .child(render_title_bar(
                TitleBarState {
                    host_sidebar_open: self.host_sidebar_open,
                    rail: &self.tab_rail,
                    tabs: self.tab_snapshots(cx),
                    right_panel: self.right_panel,
                    files: self.files_button(cx),
                    agent: self.agent_rail(),
                },
                &target,
                &tokens,
                window,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(div().flex().flex_1().min_w_0().child(body)),
            );
        // action 处理器不挂在这里，挂在包住整个 `Root` 的外层（`render_frame`）。
        div()
            .id("workspace")
            .track_focus(&self.focus_handle)
            .relative()
            .flex()
            .size_full()
            .overflow_hidden()
            .font(ui_font())
            .text_color(tokens.text)
            .text_size(TypeScale::current(cx).body)
            // 窗口底就是侧栏底；开了 Mica 时是半透明的，让系统的 Mica 从侧栏透出来（工单 26）。
            .bg(tokens.window_background(WindowBackdrop::current(cx)))
            .child(if self.host_sidebar_open {
                render_host_sidebar(
                    self.host_list.clone(),
                    self.vault.read(cx).status(),
                    cx.theme().is_dark(),
                    &target,
                    &tokens,
                    TypeScale::current(cx),
                )
                .into_any_element()
            } else if cfg!(target_os = "macos") {
                // macOS 收起时整条侧栏都不画：红绿灯要占左上角。
                div().into_any_element()
            } else {
                render_collapsed_host_sidebar(
                    self.vault.read(cx).status(),
                    cx.theme().is_dark(),
                    &target,
                    &tokens,
                    TypeScale::current(cx),
                )
                .into_any_element()
            })
            .child(main)
    }
}

impl Workspace {
    /// 包住整个 `Root`（窗口壳内容、对话框、侧滑层、通知）的外层：`Workspace` 键上下文和全部
    /// action 处理器挂在这里，对话框里派发的 action 也冒泡得到（见 `frame` 模块）。
    pub(super) fn render_frame(
        &mut self,
        surface: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("workspace-frame")
            .key_context("Workspace")
            .map(|root| handlers::bind_window_actions(root, cx))
            .map(|root| handlers::bind_tabs_actions(root, cx))
            .map(|root| handlers::bind_cards_actions(root, cx))
            .map(|root| handlers::bind_terminal_actions(root, cx))
            .map(|root| handlers::bind_settings_actions(root, cx))
            .map(|root| handlers::bind_hosts_actions(root, cx))
            .map(|root| handlers::bind_snippets_actions(root, cx))
            .map(|root| handlers::bind_remote_actions(root, cx))
            .map(|root| handlers::bind_vault_actions(root, cx))
            .map(|root| handlers::bind_credentials_actions(root, cx))
            .map(|root| handlers::bind_sftp_actions(root, cx))
            .map(|root| handlers::bind_editor_actions(root, cx))
            .map(|root| handlers::bind_agent_actions(root, cx))
            .size_full()
            .child(surface)
            .into_any_element()
    }
}

/// 主窗口的选项：无边框（标题栏自己画），开在上次的地方（`saved`，设置记住窗口时，工单 26）。
/// 背景先不透明（底色由根节点画），窗口壳应用设置时再按 Mica 开关换（`settings::apply`）。
pub fn window_options(
    saved: Option<&super::WindowState>,
    settings: WindowSettings,
    cx: &App,
) -> WindowOptions {
    let (window_bounds, display_id) = super::window_placement(saved, settings, cx);
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(PRODUCT_NAME.into()),
            // 隐藏系统标题栏；Windows 上保留缩放边框和贴靠，四边缩放交给平台层。
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(14.))),
        }),
        window_bounds: Some(window_bounds),
        display_id,
        window_min_size: Some(super::window_state::MIN_WINDOW_SIZE),
        window_background: WindowBackgroundAppearance::Opaque,
        app_id: Some(crate::app::APP_IDENTITY.into()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::sftp_provider;

    fn connector() -> micaterm_ssh::SshConnector {
        micaterm_ssh::SshConnector::new(
            std::path::PathBuf::from("known_hosts"),
            micaterm_hosts::secrets::StoreSource::shared(Arc::new(
                micaterm_hosts::secrets::InMemorySecretStore::default(),
            )),
        )
    }

    /// 取不到数据目录不是静默写到当前工作目录：不记续传记录，说明原因。
    #[test]
    fn without_a_data_folder_resuming_is_off_and_says_why() {
        let lost = || Err(std::io::Error::other("没有可执行文件的路径"));
        let (_, problem) = sftp_provider(connector(), lost(), Ok("/data/download-resume".into()));
        let problem = problem.expect("the user is told");
        assert!(problem.contains("resume"), "{problem}");
        assert!(problem.contains("没有可执行文件的路径"), "{problem}");
        let (_, problem) = sftp_provider(connector(), Ok("/data/upload-resume".into()), lost());
        assert!(problem.is_some());

        let (_, problem) = sftp_provider(
            connector(),
            Ok("/data/upload-resume".into()),
            Ok("/data/download-resume".into()),
        );
        assert_eq!(problem, None);
    }
}
