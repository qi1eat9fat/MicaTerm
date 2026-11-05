//! `Workspace`：窗口壳实体。
//!
//! 唯一的 UI 级 seam 是注入式构造 `Workspace::new_with_services`：生产代码传真实服务，
//! 无头 UI 测试传假实现（`tests/ui/support/mod.rs` 的 `fake_services`）。

use gpui_kit::component::input::InputEvent;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use qishell_hosts::secrets::{InMemorySecretStore, SharedSecretStore};
use qishell_terminal::{LocalPtyTransportProvider, SharedLocalTerminalTransportProvider};

use super::handlers;
use super::host_sidebar::render_host_sidebar;
use super::notices::NoticeTimes;
use super::quick_connect::{QuickConnectAnchor, QuickConnectPopup};
use super::remote::PromptQueue;
use super::right_sidebar::{RightPanel, render_right_sidebar};
use super::title_bar::render_title_bar;
use crate::app::PRODUCT_NAME;
use crate::backend::{
    SharedConnectionTester, SharedRemoteTerminals, SshConnectionTester, SshRemoteTerminals,
};
use crate::card::{CardId, StatusDot, TerminalCard};
use crate::host::{HostData, HostList, HostStore, HostStoreEvent};
use crate::i18n::t;
use crate::settings::{SettingsStore, SettingsStoreEvent, apply};
use crate::snippets::SnippetPanel;
use crate::split::NodeId;
use crate::tabs::{TabId, TabList, TabRail, TabSnapshot, TerminalTab};
use crate::theme::{GAP, Tokens};
use crate::vault::{Vault, VaultStore, VaultStoreEvent};

/// 窗口壳用到的外部服务。生产代码用 `WorkspaceServices::new()` 建真实的，
/// 测试用 `tests/ui/support` 的 `fake_services()` 建假的。
///
/// 后续工单每加一项服务（主机数据库、保险库、SFTP……），就在这里加一个字段，
/// 同时在 `fake_services()` 里给它一个假实现。
pub struct WorkspaceServices {
    /// 本地终端的传输：生产是本机 PTY，测试注入假的。
    pub local_terminals: SharedLocalTerminalTransportProvider,
    /// 主机、凭据、命令片段：生产是数据目录里的 `qishell.db`，测试是内存里的（`HostData::empty`、`seed`）。
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
}

impl WorkspaceServices {
    /// 真实的服务。
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
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
        let source = qishell_hosts::secrets::StoreSource::shared(secrets.clone());
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
        }
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
    /// 开始卡片的焦点：没有标签时由它持有焦点，快捷键才有焦点路径可走。
    pub(super) start_card_focus: FocusHandle,
    pub(super) settings: Entity<SettingsStore>,
    pub(super) services: WorkspaceServices,
    /// 主机栏是否显示。
    pub(super) host_sidebar_open: bool,
    /// 终端标签，从左到右，和在前的那个。
    pub(super) tabs: TabList<TerminalTab>,
    /// 下一张卡片的 id：从 1 起，不复用。
    pub(super) next_card_id: u64,
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
    /// 命令片段面板（工单 19）。
    pub(super) snippet_panel: SnippetPanel,
    /// 连接表的问题，一次问一个（工单 12）。
    pub(super) prompts: PromptQueue,
    /// 保险库（工单 13），也装成了全局量，对话框和设置页经 `VaultStore::global` 找到它。
    pub(super) vault: Entity<VaultStore>,
    /// 一直收连接表的事件（工单 12）。
    _remote_events: Task<()>,
    /// 开着的快速连接弹层（工单 18）。
    pub(super) quick_connect: Option<QuickConnectPopup>,
    /// 「+ ⌄」画在哪里，快速连接弹层挂在它下面（工单 18）。
    pub(super) quick_connect_anchor: std::rc::Rc<QuickConnectAnchor>,
    /// 开始卡片的最近连接列表高亮第几行（工单 18）。
    pub(super) start_highlight: usize,
    _subscriptions: Vec<Subscription>,
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
        let (hosts, host_list, host_subscriptions) =
            Self::open_hosts(&mut services, &focus_handle, window, cx);
        let snippet_panel = SnippetPanel::new(window, cx);
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
                cx.notify();
            }),
            // 切标签、放大还原、关卡片……窗口壳一变，就看看哪些卡片又看得见了。
            cx.observe_self(|this, cx| this.see_cards(cx)),
        ];
        subscriptions.extend(host_subscriptions);
        let (vault, vault_subscriptions) = Self::open_vault(&mut services, window, cx);
        subscriptions.extend(vault_subscriptions);
        subscriptions.push(
            // 命令片段面板的搜索框。
            cx.subscribe(
                snippet_panel.search(),
                |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        let query = input.read(cx).value().to_string();
                        this.snippet_panel.set_query(query);
                        cx.notify();
                    }
                },
            ),
        );
        apply(&settings.read(cx).settings(), window, cx);
        // 点终端的系统通知，回到那张卡片。
        Self::listen_for_notice_clicks(window, cx);
        let start_card_focus = cx.focus_handle();
        // 启动时没有标签：开始卡片持有焦点。
        start_card_focus.focus(window, cx);
        let remote_events = Self::listen_for_remote_events(&services, window, cx);
        Self {
            focus_handle,
            start_card_focus,
            settings,
            services,
            host_sidebar_open: true,
            tabs: TabList::default(),
            next_card_id: 1,
            notice_times: NoticeTimes::default(),
            tab_rail: TabRail::default(),
            window_active: window.is_window_active(),
            hosts,
            host_list,
            right_panel: None,
            snippet_panel,
            prompts: PromptQueue::default(),
            vault,
            _remote_events: remote_events,
            quick_connect: None,
            quick_connect_anchor: Default::default(),
            start_highlight: 0,
            _subscriptions: subscriptions,
        }
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
        if let Some(problem) = services.vault_problem.take() {
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
    pub fn selected_host(&self, cx: &App) -> Option<qishell_hosts::HostId> {
        self.host_list.read(cx).selected_host()
    }

    /// 主机栏搜索时高亮的主机。
    pub fn highlighted_host(&self, cx: &App) -> Option<qishell_hosts::HostId> {
        self.host_list.read(cx).highlighted_host(cx)
    }

    /// 主机栏列出的主机，从上到下（搜索时只有匹配的）。
    pub fn listed_hosts(&self, cx: &App) -> Vec<qishell_hosts::HostId> {
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

    /// 一个标签里的卡片。
    pub fn tab_cards(&self, tab: TabId) -> Vec<Entity<TerminalCard>> {
        self.tabs
            .get(tab)
            .map(|tab| tab.cards().cloned().collect())
            .unwrap_or_default()
    }

    /// id 为 `card` 的卡片，在哪个标签里都找。
    pub fn card(&self, card: CardId, cx: &App) -> Option<Entity<TerminalCard>> {
        self.tabs
            .iter()
            .flat_map(|(_, tab)| tab.cards())
            .find(|entity| entity.read(cx).id() == card)
            .cloned()
    }

    /// 卡片终端屏幕上的文字，测试用。
    pub fn card_screen_text(&self, card: CardId, cx: &App) -> Option<String> {
        self.card(card, cx)
            .map(|entity| entity.read(cx).screen_text(cx))
    }

    /// 在前那个标签的焦点卡片。
    pub fn focused_card(&self, cx: &App) -> Option<CardId> {
        self.tabs
            .active_content()
            .map(|tab| tab.focused_card().read(cx).id())
    }

    /// 在前那个标签里放大着的卡片。
    pub fn zoomed_card(&self) -> Option<CardId> {
        self.tabs
            .active_content()
            .and_then(|tab| tab.tree().zoomed())
    }

    /// 在前那个标签里分隔线 `node` 的比例（左边或上边占的份额）。
    pub fn divider_ratio(&self, node: NodeId) -> Option<f32> {
        self.tabs
            .active_content()
            .and_then(|tab| tab.tree().ratio(node))
    }

    /// 卡片终端内容的不透明度：焦点卡片 1，其余 0.85，窗口失活时 0.7。
    pub fn card_opacity(&self, card: CardId, window: &Window, cx: &App) -> Option<f32> {
        self.card(card, cx)
            .map(|entity| entity.read(cx).content_opacity(window))
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

    /// 标签上叠放的图块的程序名，从前到后。
    pub fn tab_tiles(&self, tab: TabId, cx: &App) -> Vec<String> {
        self.tabs
            .get(tab)
            .map(|tab| {
                tab.tiles(cx)
                    .into_iter()
                    .map(|program| program.name)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 卡片头部的状态点。
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
            .map(|(id, tab)| {
                let card = tab.focused_card().read(cx);
                TabSnapshot {
                    id,
                    name: card.name(),
                    directory: card.directory_label().into(),
                    active: active == Some(id),
                    tiles: tab.tiles(cx),
                    dot: tab.dot(cx),
                }
            })
            .collect()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let target = self.focus_handle.clone();
        // 主机栏右侧的绿色数字（工单 16）：开卡片、关卡片、关标签都会让窗口壳重画，这里统一告诉主机列表。
        let open_cards = self.open_cards_by_host(cx);
        self.host_list
            .update(cx, |list, cx| list.set_open_cards(open_cards, cx));
        // 有标签时是在前那个标签的内容，没有时是开始卡片。
        let content = div()
            .flex()
            .flex_1()
            .min_w_0()
            .when(!self.host_sidebar_open, |content| content.pl(GAP))
            .map(|content| match self.tabs.active_content() {
                Some(tab) => content.child(tab.content()),
                None => content.child(self.render_start_card(&target, &tokens, window, cx)),
            });
        let main = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(render_title_bar(
                self.host_sidebar_open,
                self.quick_connect.is_some(),
                &self.quick_connect_anchor(),
                self.tab_rail
                    .render(self.tab_snapshots(cx), &target, &tokens),
                &target,
                &tokens,
                window,
            ))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .pb(GAP)
                    .child(content)
                    .children(self.right_panel.map(|panel| match panel {
                        RightPanel::Snippets => self.render_snippet_panel(&tokens, cx),
                        // 凭据面板在工单 14 接上。
                        RightPanel::Credentials => div().into_any_element(),
                    }))
                    .child(render_right_sidebar(self.right_panel, &target, &tokens)),
            );
        div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus_handle)
            .map(|root| handlers::bind_window_actions(root, cx))
            .map(|root| handlers::bind_tabs_actions(root, cx))
            .map(|root| handlers::bind_cards_actions(root, cx))
            .map(|root| handlers::bind_terminal_actions(root, cx))
            .map(|root| handlers::bind_settings_actions(root, cx))
            .map(|root| handlers::bind_hosts_actions(root, cx))
            .map(|root| handlers::bind_snippets_actions(root, cx))
            .map(|root| handlers::bind_remote_actions(root, cx))
            .map(|root| handlers::bind_quick_connect_actions(root, cx))
            .map(|root| handlers::bind_vault_actions(root, cx))
            .relative()
            .flex()
            .size_full()
            .overflow_hidden()
            .text_color(tokens.text)
            .bg(linear_gradient(
                180.,
                linear_color_stop(tokens.bg_top, 0.),
                linear_color_stop(tokens.bg_bottom, 1.),
            ))
            .when(self.host_sidebar_open, |root| {
                root.child(render_host_sidebar(
                    self.hosts.read(cx).hosts().len(),
                    self.host_list.clone(),
                    self.vault.read(cx).status(),
                    &target,
                    &tokens,
                ))
            })
            .child(main)
            .children(self.render_quick_connect())
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

/// 主窗口的选项：无边框（标题栏自己画），背景不透明（渐变由根节点画）。
pub fn window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(PRODUCT_NAME.into()),
            // 隐藏系统标题栏；Windows 上保留缩放边框和贴靠，四边缩放交给平台层。
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(14.))),
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1280.), px(800.)),
            cx,
        ))),
        window_min_size: Some(size(px(720.), px(480.))),
        window_background: WindowBackgroundAppearance::Opaque,
        app_id: Some(crate::app::APP_IDENTITY.into()),
        ..Default::default()
    }
}
