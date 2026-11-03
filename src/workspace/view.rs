//! `Workspace`：窗口壳实体。
//!
//! 唯一的 UI 级 seam 是注入式构造 `Workspace::new_with_services`：生产代码传真实服务，
//! 无头 UI 测试传假实现（`tests/ui/support/mod.rs` 的 `fake_services`）。

use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::handlers;
use super::host_sidebar::render_host_sidebar;
use super::right_sidebar::render_right_sidebar;
use super::start_card::render_start_card;
use super::title_bar::render_title_bar;
use crate::app::PRODUCT_NAME;
use crate::settings::{SettingsStore, SettingsStoreEvent};
use crate::theme::{GAP, Tokens};

/// 窗口壳用到的外部服务。生产代码用 `WorkspaceServices::new()` 建真实的，
/// 测试用 `tests/ui/support` 的 `fake_services()` 建假的。
///
/// 后续工单每加一项服务（本地终端传输、主机数据库、保险库、SFTP……），就在这里加一个字段，
/// 同时在 `fake_services()` 里给它一个假实现。
pub struct WorkspaceServices {}

impl WorkspaceServices {
    /// 真实的服务。
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {}
    }
}

/// 窗口壳。
pub struct Workspace {
    /// 窗口壳根节点的焦点：按钮一律经它派发 action，焦点为空时也能送到这里的处理器。
    pub(super) focus_handle: FocusHandle,
    /// 开始卡片的焦点：没有标签时由它持有焦点，快捷键才有焦点路径可走。
    pub(super) start_card_focus: FocusHandle,
    pub(super) settings: Entity<SettingsStore>,
    #[allow(dead_code)]
    pub(super) services: WorkspaceServices,
    /// 主机栏是否显示。
    pub(super) host_sidebar_open: bool,
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
        ];
        let start_card_focus = cx.focus_handle();
        // 启动时没有标签：开始卡片持有焦点。
        start_card_focus.focus(window, cx);
        Self {
            focus_handle: cx.focus_handle(),
            start_card_focus,
            settings,
            services,
            host_sidebar_open: true,
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
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let target = self.focus_handle.clone();
        let content = div()
            .flex()
            .flex_1()
            .min_w_0()
            .when(!self.host_sidebar_open, |content| content.pl(GAP))
            .child(render_start_card(
                &self.start_card_focus,
                &target,
                &tokens,
                window,
            ));
        let main = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(render_title_bar(
                self.host_sidebar_open,
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
