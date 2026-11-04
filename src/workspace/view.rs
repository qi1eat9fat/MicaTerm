//! `Workspace`：窗口壳实体。
//!
//! 唯一的 UI 级 seam 是注入式构造 `Workspace::new_with_services`：生产代码传真实服务，
//! 无头 UI 测试传假实现（`tests/ui/support/mod.rs` 的 `fake_services`）。

use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use qishell_terminal::{LocalPtyTransportProvider, SharedLocalTerminalTransportProvider};

use super::handlers;
use super::host_sidebar::render_host_sidebar;
use super::notices::NoticeTimes;
use super::right_sidebar::render_right_sidebar;
use super::start_card::render_start_card;
use super::title_bar::render_title_bar;
use crate::app::PRODUCT_NAME;
use crate::card::{CardId, StatusDot, TerminalCard};
use crate::settings::{SettingsStore, SettingsStoreEvent, apply};
use crate::split::NodeId;
use crate::tabs::{TabId, TabList, TabRail, TabSnapshot, TerminalTab};
use crate::theme::{GAP, Tokens};

/// 窗口壳用到的外部服务。生产代码用 `WorkspaceServices::new()` 建真实的，
/// 测试用 `tests/ui/support` 的 `fake_services()` 建假的。
///
/// 后续工单每加一项服务（主机数据库、保险库、SFTP……），就在这里加一个字段，
/// 同时在 `fake_services()` 里给它一个假实现。
pub struct WorkspaceServices {
    /// 本地终端的传输：生产是本机 PTY，测试注入假的。
    pub local_terminals: SharedLocalTerminalTransportProvider,
}

impl WorkspaceServices {
    /// 真实的服务。
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            local_terminals: std::sync::Arc::new(LocalPtyTransportProvider),
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
        services: WorkspaceServices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
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
        apply(&settings.read(cx).settings(), window, cx);
        // 点终端的系统通知，回到那张卡片。
        Self::listen_for_notice_clicks(window, cx);
        let start_card_focus = cx.focus_handle();
        // 启动时没有标签：开始卡片持有焦点。
        start_card_focus.focus(window, cx);
        Self {
            focus_handle: cx.focus_handle(),
            start_card_focus,
            settings,
            services,
            host_sidebar_open: true,
            tabs: TabList::default(),
            next_card_id: 1,
            notice_times: NoticeTimes::default(),
            tab_rail: TabRail::default(),
            window_active: window.is_window_active(),
            _subscriptions: subscriptions,
        }
    }

    /// 设置。
    pub fn settings(&self) -> &Entity<SettingsStore> {
        &self.settings
    }

    /// 主机栏是否显示。
    pub fn is_host_sidebar_open(&self) -> bool {
        self.host_sidebar_open
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
        // 有标签时是在前那个标签的内容，没有时是开始卡片。
        let content = div()
            .flex()
            .flex_1()
            .min_w_0()
            .when(!self.host_sidebar_open, |content| content.pl(GAP))
            .map(|content| match self.tabs.active_content() {
                Some(tab) => content.child(tab.content()),
                None => content.child(render_start_card(
                    &self.start_card_focus,
                    &target,
                    &tokens,
                    window,
                )),
            });
        let main = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(render_title_bar(
                self.host_sidebar_open,
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
                    .child(render_right_sidebar(&target, &tokens)),
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
                root.child(render_host_sidebar(&target, &tokens))
            })
            .child(main)
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
