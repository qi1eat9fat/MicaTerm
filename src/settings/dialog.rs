//! 设置对话框（spec 第 9 节）：一个大对话框，左边分类导航，右边是选中那一类的设置，正文可以滚动。
//! 改动立即写进设置存储（`SettingsStore`），窗口壳观察它、马上应用。
//!
//! 分类是一张表（`CATEGORIES`）：加一类只要往表里加一项、加一个页面函数。

use std::path::PathBuf;

use gpui_kit::component::{Icon, IconName, input::InputState, scroll::ScrollableElement as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use qishell_terminal::monospace_font_families;

use super::SettingsStore;
use super::local_shell_editor::LocalShellEditor;
use super::pages;
use super::sftp::LocalDirectoryEditor;
use crate::app::CatalogIcon;
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::DialogLayer;

/// 对话框的键上下文。
pub const SETTINGS_DIALOG_KEY_CONTEXT: &str = "SettingsDialog";

/// 左栏的一类设置。
struct Category {
    /// 名称的文案键。
    title: &'static str,
    icon: CatalogIcon,
    /// 这一类的内容：分组标题和分组，从上到下。
    page: Page,
}

/// 画一类设置的页面。
type Page =
    fn(&SettingsDialog, &Tokens, &mut Window, &mut Context<SettingsDialog>) -> Vec<AnyElement>;

/// 左栏的分类，按显示的顺序（原型 `SETNAV`）：外观、终端、SFTP、安全、应用（spec 第 9 节的五类）。
const CATEGORIES: &[Category] = &[
    Category {
        title: "settings.category.appearance",
        icon: CatalogIcon::Palette,
        page: pages::appearance,
    },
    Category {
        title: "settings.category.terminal",
        icon: CatalogIcon::Terminal,
        page: pages::terminal,
    },
    Category {
        title: "settings.category.sftp",
        icon: CatalogIcon::Folder,
        page: super::sftp::sftp,
    },
    Category {
        title: "settings.category.security",
        icon: CatalogIcon::Shield,
        page: super::security::security,
    },
    Category {
        title: "settings.category.app",
        icon: CatalogIcon::AppWindow,
        page: super::app_page::app,
    },
];

/// 设置对话框要显示的几个文件夹。
#[derive(Clone, Debug, Default)]
pub struct SettingsDialogPaths {
    /// 「下载」文件夹：本地默认打开目录留空时用它，这里只用来显示。
    pub downloads: PathBuf,
    /// 数据目录（设置 › 应用显示它）；`None` 是这次运行没有数据目录。
    pub data_dir: Option<PathBuf>,
}

/// 设置对话框的内容。每次打开新建一个。
pub struct SettingsDialog {
    pub(super) store: Entity<SettingsStore>,
    /// 显示的是 `CATEGORIES` 里的第几类。
    category: usize,
    /// 可以选的终端字体（本机装了的等宽字体）；后台扫描回来之前是空的。
    pub(super) font_families: &'static [SharedString],
    /// 终端字号、行高的输入框：输入时保住光标，所以留在实体里。
    pub(super) terminal_font_size: Entity<InputState>,
    pub(super) terminal_line_height: Entity<InputState>,
    pub(super) local_shell: Entity<LocalShellEditor>,
    pub(super) local_directory: Entity<LocalDirectoryEditor>,
    /// 数据目录，设置 › 应用显示它。
    pub(super) data_dir: Option<PathBuf>,
    /// 对话框是哪一层：关闭按钮只关自己。
    layer: DialogLayer,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _font_scan: Task<()>,
}

impl SettingsDialog {
    pub fn new(
        store: Entity<SettingsStore>,
        paths: SettingsDialogPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let SettingsDialogPaths {
            downloads,
            data_dir,
        } = paths;
        let local_shell = cx.new(|cx| LocalShellEditor::new(store.clone(), window, cx));
        let local_directory =
            cx.new(|cx| LocalDirectoryEditor::new(store.clone(), downloads, window, cx));
        let mut subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        // 设置 › 安全跟着保险库的状态变（工单 13）。
        if let Some(vault) = crate::vault::VaultStore::global(cx) {
            subscriptions.push(cx.observe(&vault, |_, _, cx| cx.notify()));
        }
        let (terminal_font_size, font_size_subscriptions) =
            pages::terminal_font_size_field(&store, window, cx);
        let (terminal_line_height, line_height_subscriptions) =
            pages::terminal_line_height_field(&store, window, cx);
        subscriptions.extend(font_size_subscriptions);
        subscriptions.extend(line_height_subscriptions);
        // 第一次列出全部字体要一会儿；进程会记住结果，之后再打开就是现成的。
        let text_system = cx.text_system().clone();
        let font_scan = cx.spawn(async move |this, cx| {
            let families = cx
                .background_spawn(async move { monospace_font_families(&text_system) })
                .await;
            this.update(cx, |this, cx| {
                this.font_families = families;
                cx.notify();
            })
            .ok();
        });
        Self {
            store,
            category: 0,
            font_families: &[],
            terminal_font_size,
            terminal_line_height,
            local_shell,
            local_directory,
            data_dir,
            layer: DialogLayer::new(),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
            _font_scan: font_scan,
        }
    }

    /// 显示第几类。
    pub fn category(&self) -> usize {
        self.category
    }

    fn show_category(&mut self, category: usize, cx: &mut Context<Self>) {
        if self.category != category {
            self.category = category;
            cx.notify();
        }
    }
}

impl Focusable for SettingsDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let page = (CATEGORIES[self.category].page)(self, &tokens, window, cx);
        div()
            .id("settings")
            .test_support()
            .key_context(SETTINGS_DIALOG_KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .size_full()
            .text_color(tokens.text)
            .child(header(&self.layer, &tokens))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(tokens.line)
                    .child(navigation(self.category, &tokens, cx))
                    .child(
                        div()
                            .id("settings-body")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .px(px(22.))
                            .py(px(16.))
                            .overflow_y_scrollbar()
                            .children(page),
                    ),
            )
    }
}

/// 标题行：「设置」、一句说明、关闭按钮。
fn header(layer: &DialogLayer, tokens: &Tokens) -> impl IntoElement {
    let layer = layer.clone();
    let hover = tokens.hover;
    let text = tokens.text;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(px(46.))
        .pl(px(18.))
        .pr(px(8.))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(t!("settings.dialog.title")),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(12.))
                .text_color(tokens.faint)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(t!("settings.dialog.subtitle")),
        )
        .child(
            div()
                .id("settings-close")
                .test_support()
                .aria_label(t!("settings.dialog.close"))
                .flex()
                .items_center()
                .justify_center()
                .size(px(26.))
                .rounded(px(10.))
                .text_color(tokens.icon)
                .cursor_pointer()
                .hover(move |style| style.bg(hover).text_color(text))
                .on_click(move |_, window, cx| layer.close(window, cx))
                .child(Icon::new(IconName::Close).size(px(16.))),
        )
}

/// 左栏：分类，选中的那一项浮起来。
fn navigation(
    selected: usize,
    tokens: &Tokens,
    cx: &mut Context<SettingsDialog>,
) -> impl IntoElement {
    let on = tokens.tab_active;
    let shadow = tokens.shadow;
    let hover = tokens.hover;
    div()
        .id("settings-categories")
        .test_support()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(2.))
        .w(px(170.))
        .px(px(8.))
        .py(px(10.))
        .border_r_1()
        .border_color(tokens.line)
        .bg(tokens.group)
        .rounded_bl(px(14.))
        .children(CATEGORIES.iter().enumerate().map(|(index, category)| {
            let title = t!(category.title);
            let chosen = index == selected;
            div()
                .id(("settings-category", index))
                .test_support()
                .role(Role::Tab)
                .aria_label(title.clone())
                .aria_selected(chosen)
                .flex()
                .items_center()
                .gap(px(9.))
                .h(px(32.))
                .px(px(12.))
                .rounded_full()
                .text_size(px(12.5))
                .cursor_pointer()
                .map(|item| {
                    if chosen {
                        item.bg(on).text_color(tokens.text).shadow(vec![BoxShadow {
                            color: shadow,
                            offset: point(px(0.), px(2.)),
                            blur_radius: px(8.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                    } else {
                        item.text_color(tokens.muted)
                            .hover(move |style| style.bg(hover))
                    }
                })
                .on_click(cx.listener(move |this, _, _, cx| this.show_category(index, cx)))
                .child(Icon::new(category.icon).size(px(15.)))
                .child(title)
        }))
}

/// 打开设置对话框。对话框不点遮罩关闭，用右上角的按钮或 Esc 关。
pub fn open_settings_dialog(
    store: Entity<SettingsStore>,
    paths: SettingsDialogPaths,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| SettingsDialog::new(store, paths, window, cx));
    let layer = view.read(cx).layer.clone();
    layer.open(
        move |dialog, window, _| {
            let viewport = window.viewport_size();
            let width = px(860.).min(viewport.width - px(40.));
            let height = (viewport.height - px(60.)).max(px(320.));
            dialog
                .w(width)
                .h(height)
                .margin_top(px(30.))
                .p_0()
                .rounded(px(14.))
                .close_button(false)
                .overlay_closable(false)
                .child(view.clone())
        },
        window,
        cx,
    );
}
