//! 设置标签（26 号「设置」，照 Zed settings_ui）：左边导航 200 宽，右边一列设置，列最宽 680、居中、可以滚动。
//! 改动立即写进设置存储（`SettingsStore`），窗口壳观察它、马上应用。
//! 一个窗口只开一个设置标签：再按 Ctrl+, 或标题栏齿轮就切过去。
//!
//! 分类是一张表（`CATEGORIES`）：加一类只要往表里加一项、加一个页面函数。

use std::path::PathBuf;

use gpui_kit::component::{Icon, scroll::ScrollableElement as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use micaterm_terminal::monospace_font_families;

use super::SettingsStore;
use super::controls::page_title;
use super::local_shell_editor::LocalShellEditor;
use super::pages;
use super::sftp::LocalDirectoryEditor;
use crate::app::CatalogIcon;
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale, size};

/// 左栏的一类设置。
struct Category {
    /// 名称的文案键。
    title: &'static str,
    icon: CatalogIcon,
    /// 这一类的内容：分组标题和设置行，从上到下。
    page: Page,
}

/// 画一类设置的页面。
type Page = fn(&SettingsTab, &Tokens, &mut Window, &mut Context<SettingsTab>) -> Vec<AnyElement>;

/// 左栏的分类，按显示的顺序（26 号 `STG_NAV`）：外观、终端、SFTP、安全、应用。
const CATEGORIES: &[Category] = &[
    Category {
        title: "settings.category.appearance",
        icon: CatalogIcon::Palette,
        page: pages::appearance,
    },
    Category {
        title: "settings.category.terminal",
        icon: CatalogIcon::SquareTerminal,
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

/// 右边那一列最宽多少（26 号 `.stg-col`：680 加左右各 32）。
const COLUMN_WIDTH: Pixels = px(744.);

/// 设置页要显示的几个文件夹。
#[derive(Clone, Debug, Default)]
pub struct SettingsPaths {
    /// 「下载」文件夹：本地默认打开目录留空时用它，这里只用来显示。
    pub downloads: PathBuf,
    /// 数据目录（设置 › 应用显示它）；`None` 是这次运行没有数据目录。
    pub data_dir: Option<PathBuf>,
}

/// 设置标签的内容。
pub struct SettingsTab {
    pub(super) store: Entity<SettingsStore>,
    /// 显示的是 `CATEGORIES` 里的第几类。
    category: usize,
    /// 可以选的终端字体（本机装了的等宽字体）；后台扫描回来之前是空的。
    pub(super) font_families: &'static [SharedString],
    pub(super) local_shell: Entity<LocalShellEditor>,
    pub(super) local_directory: Entity<LocalDirectoryEditor>,
    /// 数据目录，设置 › 应用显示它。
    pub(super) data_dir: Option<PathBuf>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _font_scan: Task<()>,
}

impl SettingsTab {
    pub fn new(
        store: Entity<SettingsStore>,
        paths: SettingsPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let SettingsPaths {
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
            local_shell,
            local_directory,
            data_dir,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
            _font_scan: font_scan,
        }
    }

    /// 显示第几类。
    pub fn category(&self) -> usize {
        self.category
    }

    /// 标签轨道上的名字：「设置」。
    pub fn tab_name(&self) -> SharedString {
        t!("settings.tab.title")
    }

    /// 标签到前面来时焦点给谁。
    pub fn focus_target(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    fn show_category(&mut self, category: usize, cx: &mut Context<Self>) {
        if self.category != category {
            self.category = category;
            cx.notify();
        }
    }
}

impl Focusable for SettingsTab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let category = &CATEGORIES[self.category];
        let page = (category.page)(self, &tokens, window, cx);
        div()
            .id("settings")
            .track_focus(&self.focus_handle)
            .flex()
            .flex_1()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(tokens.content)
            .text_color(tokens.text)
            .child(navigation(self.category, &tokens, cx))
            .child(
                div()
                    .id("settings-body")
                    .flex()
                    .flex_col()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w_full()
                            .max_w(COLUMN_WIDTH)
                            .px(px(32.))
                            .pt(px(20.))
                            .pb(px(40.))
                            .child(page_title(t!(category.title), scale, &tokens))
                            .children(page),
                    ),
            )
            .test_support()
    }
}

/// 左栏（26 号 `.stg-nav`）：宽 200、标签栏底色、右边一条线；条目和主机栏同一套行（高 28、圆角 8、
/// 选中 `--sel`）；底部一行淡字「改动立即生效」，和主机栏左下角保险库那一行同高、同样贴底。
fn navigation(selected: usize, tokens: &Tokens, cx: &mut Context<SettingsTab>) -> impl IntoElement {
    let hover = tokens.hover;
    div()
        .id("settings-categories")
        .test_support()
        .flex()
        .flex_col()
        .flex_none()
        .w(size::SETTINGS_NAV_WIDTH)
        .h_full()
        .pt(px(10.))
        .px(px(8.))
        .bg(tokens.tabbar)
        .border_r(size::LINE)
        .border_color(tokens.line)
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
                .flex_none()
                .items_center()
                .gap(px(10.))
                .h(px(28.))
                .px(px(8.))
                .rounded(size::RADIUS_LG)
                .cursor_pointer()
                .map(|item| {
                    if chosen {
                        item.bg(tokens.sel).text_color(tokens.text)
                    } else {
                        item.text_color(tokens.label)
                            .hover(move |style| style.bg(hover))
                    }
                })
                .on_click(cx.listener(move |this, _, _, cx| this.show_category(index, cx)))
                .child(
                    Icon::new(category.icon)
                        .size(px(14.))
                        .flex_none()
                        .text_color(tokens.icon),
                )
                .child(div().line_height(px(28.)).child(title))
        }))
        .child(
            div()
                .id("settings-applies-at-once")
                .test_support()
                .flex()
                .flex_none()
                .items_center()
                .mt_auto()
                .h(size::STATUS_BAR)
                .border_t(size::LINE)
                .border_color(transparent_black())
                .px(px(8.))
                .text_color(tokens.st_dim)
                .child(t!("settings.tab.applies_at_once")),
        )
}
