//! 命令片段面板（26 号「片段」，停靠面板 `ui::dock`）：标题后面写「→ 焦点卡片名」，顶部按名称或命令
//! 搜索，列表按名称排序、不能拖动，没有分组；每条两行：名称，下一行淡色等宽的命令；悬停时右边出现
//! ⚡（点击就执行）或 ▶（点击只输入），点它总是直接执行。右键菜单四项。
//!
//! 和标签轨道一样，面板是窗口壳持有的普通结构：渲染不读实体，只拿窗口壳给的片段和焦点卡片名；
//! 点击、菜单都经窗口壳的焦点派发 action，由窗口壳把命令发给焦点卡片。
// 参考 since2006/shell-rs @ b2a307a: src/snippets/snippet_panel.rs 重写（去掉分类、折叠；
// 右键菜单不用 gpui-component 的 `context_menu`，照 `TabRail` 自己持有菜单实体）。

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::Icon;
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use micaterm_ui::Menu;

use crate::app::{
    CatalogIcon, CopySnippet, DeleteSnippet, DismissSnippetNotice, EditSnippet, EnterSnippet,
    NewSnippet, OpenSnippetMenu,
};
use crate::host::{Snippet, SnippetId};
use crate::i18n::t;
use crate::theme::{MONO_FONT_FAMILY, Tokens, TypeScale, size};
use crate::ui::dock::{DockNew, DockTarget, dock_empty, dock_header, dock_panel, dock_search};
use crate::ui::icon_button::TextTooltip;

/// 行右边 ⚡ / ▶ 的边长（`.sni .ib`）。
const RUN_BUTTON: Pixels = px(22.);

/// 面板跨帧要记住的东西：搜索框和搜的词、开着的右键菜单、底部「没有发出去」的提示。
pub struct SnippetPanel {
    search: Entity<InputState>,
    query: String,
    menu: Option<OpenMenu>,
    notice: Option<SharedString>,
    scroll: ScrollHandle,
}

/// 开着的片段右键菜单。菜单实体由面板持有，关菜单时一起放掉。
struct OpenMenu {
    snippet: SnippetId,
    position: Point<Pixels>,
    view: Entity<Menu>,
    _dismiss: Subscription,
}

/// 面板要画的东西，由窗口壳取好。
pub struct SnippetPanelProps<'a> {
    /// 全部片段，按名称排好（`HostData::snippets_by_name`）。
    pub snippets: Vec<&'a Snippet>,
    /// 片段发给谁：焦点卡片的名称；当前标签没有终端时是 `None`。
    pub target: Option<SharedString>,
}

impl SnippetPanel {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("snippets.panel.search"))
                .clean_on_escape()
        });
        Self {
            search,
            query: String::new(),
            menu: None,
            notice: None,
            scroll: ScrollHandle::new(),
        }
    }

    /// 搜索框：窗口壳订阅它的改动，再调 `set_query`。
    pub fn search(&self) -> &Entity<InputState> {
        &self.search
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
    }

    /// 在 `position`（窗口坐标）为 `snippet` 开右键菜单；之前开着的那个放掉。
    pub fn open_menu(
        &mut self,
        snippet: SnippetId,
        position: Point<Pixels>,
        view: Entity<Menu>,
        dismiss: Subscription,
    ) {
        self.menu = Some(OpenMenu {
            snippet,
            position,
            view,
            _dismiss: dismiss,
        });
    }

    /// 关掉右键菜单，只在开着的正是 `view` 时（右键另一行时，旧菜单的关闭事件晚到）。
    pub fn close_menu(&mut self, view: &Entity<Menu>) {
        if self.menu.as_ref().is_some_and(|menu| &menu.view == view) {
            self.menu = None;
        }
    }

    /// 开着右键菜单的片段。
    pub fn menu_snippet(&self) -> Option<SnippetId> {
        self.menu.as_ref().map(|menu| menu.snippet)
    }

    /// 底部的提示：片段为什么没有发出去。`None` 收起。
    pub fn set_notice(&mut self, notice: Option<SharedString>) {
        self.notice = notice;
    }

    pub fn notice(&self) -> Option<&SharedString> {
        self.notice.as_ref()
    }

    /// 面板（停靠面板 `ui::dock`，宽 280）。`dispatch` 是窗口壳的焦点：点击和菜单都经它派发。
    pub fn render(
        &self,
        props: SnippetPanelProps,
        dispatch: &FocusHandle,
        tokens: &Tokens,
        window: &Window,
        cx: &App,
    ) -> impl IntoElement + use<> {
        let SnippetPanelProps { snippets, target } = props;
        let body = match &target {
            None => dock_empty(
                "snippets-no-terminal",
                t!("snippets.panel.no_terminal"),
                Some(t!("snippets.panel.no_terminal_hint")),
                tokens,
                cx,
            ),
            Some(_) => self.render_list(&snippets, dispatch, tokens, window, cx),
        };
        let to = target.map(|name| DockTarget {
            id: "snippets-target",
            label: t!("snippets.panel.target", card = name.clone()),
            name,
        });
        dock_panel("snippets", tokens)
            .child(dock_header(
                t!("snippets.panel.title"),
                to,
                DockNew {
                    id: "snippets-new",
                    label: t!("snippets.panel.new"),
                    action: Box::new(NewSnippet),
                },
                dispatch,
                tokens,
                cx,
            ))
            .child(body)
            .children(
                self.notice
                    .clone()
                    .map(|notice| render_notice(notice, dispatch, tokens)),
            )
            .children(self.menu.as_ref().map(|menu| {
                deferred(
                    anchored()
                        .position(menu.position)
                        .snap_to_window_with_margin(px(8.))
                        .child(menu.view.clone()),
                )
                .with_priority(POPUP_PRIORITY)
            }))
            .test_support()
    }

    /// 搜索框和片段列表；一个片段都没有、搜不到时写一句话。
    fn render_list(
        &self,
        snippets: &[&Snippet],
        dispatch: &FocusHandle,
        tokens: &Tokens,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let menu_snippet = self.menu_snippet();
        let rows: Vec<AnyElement> = snippets
            .iter()
            .filter(|snippet| snippet.matches(&self.query))
            .map(|snippet| {
                render_row(
                    snippet,
                    menu_snippet == Some(snippet.id),
                    dispatch,
                    tokens,
                    cx,
                )
                .into_any_element()
            })
            .collect();
        let empty = if snippets.is_empty() {
            Some(dock_empty(
                "snippets-empty",
                t!("snippets.panel.empty"),
                Some(t!("snippets.panel.empty_hint")),
                tokens,
                cx,
            ))
        } else if rows.is_empty() {
            Some(dock_empty(
                "snippets-no-match",
                t!("snippets.panel.no_match"),
                None,
                tokens,
                cx,
            ))
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(dock_search(
                "snippets-search",
                t!("snippets.panel.search"),
                &self.search,
                tokens,
                window,
                cx,
            ))
            .child(
                div()
                    .id("snippets-list")
                    .test_support()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .pb(px(8.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows)
                    .children(empty),
            )
            .into_any_element()
    }
}

/// 一个片段（26 号 `.sni`）：名称一行（13，界面字），下面淡色等宽的命令一行（11.5），放不下就省略，
/// 悬停看全文；悬停时右边出现 ⚡（点一下这一行会执行）或 ▶（点一下只输入），点它总是直接执行。
/// `pressed`：它的右键菜单开着（描一圈细边）。
fn render_row(
    snippet: &Snippet,
    pressed: bool,
    dispatch: &FocusHandle,
    tokens: &Tokens,
    cx: &App,
) -> impl IntoElement + use<> {
    let scale = TypeScale::current(cx);
    let id = snippet.id;
    let line = one_line(&snippet.command);
    let runs = snippet.run_on_click;
    let group = SharedString::from(format!("snippet-row-{}", id.0));
    let hover = tokens.hover;
    let text = tokens.text;
    let command = SharedString::from(snippet.command.clone());
    let run_hint = if runs {
        t!("snippets.panel.runs_on_click")
    } else {
        t!("snippets.panel.types_on_click")
    };
    let (click, menu, run) = (dispatch.clone(), dispatch.clone(), dispatch.clone());
    div()
        .id(("snippet", id.0))
        .test_support()
        .group(group.clone())
        .aria_label(SharedString::from(format!("{} · {line}", snippet.name)))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .pl(px(8.))
        .pr(px(4.))
        .py(px(5.))
        .rounded(size::RADIUS_LG)
        .text_size(scale.side)
        .text_color(tokens.label)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |style| style.bg(hover).text_color(text))
        // 被右键的那一行描一圈细边（26 号 `.sni.ctx`）。
        .when(pressed, |row| {
            row.border_1().border_color(tokens.line_strong)
        })
        .when(!pressed, |row| {
            row.tooltip(move |_, cx| TextTooltip::view(command.clone(), cx))
        })
        // 单击按片段自己的「点击时自动执行」：执行，或者只输入。
        .on_click(move |_, window, cx| {
            click.dispatch_action(
                &EnterSnippet {
                    snippet: id,
                    run: runs,
                },
                window,
                cx,
            )
        })
        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
            cx.stop_propagation();
            let position = event.position;
            menu.dispatch_action(
                &OpenSnippetMenu {
                    snippet: id,
                    position,
                },
                window,
                cx,
            );
        })
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .line_height(px(20.))
                        .truncate()
                        .child(snippet.name.clone()),
                )
                .child(
                    div()
                        .line_height(px(18.))
                        .truncate()
                        .font_family(MONO_FONT_FAMILY)
                        .text_size(scale.mono_small)
                        .text_color(tokens.st_dim)
                        .child(line),
                ),
        )
        .child(
            // 点它总是直接执行；不要再触发整行的单击（那会按片段的设置再发一次）。
            div()
                .id(("snippet-run", id.0))
                .test_support()
                .aria_label(run_hint.clone())
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(RUN_BUTTON)
                .rounded(size::RADIUS)
                .invisible()
                .group_hover(group, |style| style.visible())
                .when(pressed, |button| button.visible())
                .text_color(tokens.icon)
                .hover(move |style| style.bg(hover).text_color(text))
                .tooltip(move |_, cx| TextTooltip::view(run_hint.clone(), cx))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    run.dispatch_action(
                        &EnterSnippet {
                            snippet: id,
                            run: true,
                        },
                        window,
                        cx,
                    );
                })
                .child(
                    Icon::new(if runs {
                        CatalogIcon::Zap
                    } else {
                        CatalogIcon::Play
                    })
                    .size(px(13.)),
                ),
        )
}

/// 面板底部的提示条：片段为什么没有发出去。点一下收起。
fn render_notice(
    notice: SharedString,
    dispatch: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
    let dismiss = dispatch.clone();
    div()
        .id("snippets-notice")
        .test_support()
        .aria_label(notice.clone())
        .absolute()
        .left(px(8.))
        .right(px(8.))
        .bottom(px(8.))
        .px(px(10.))
        .py(px(8.))
        .rounded(size::RADIUS_LG)
        // 浮在列表上面，要不透明：内容底上叠 14% 的警示黄。
        .bg(tokens.content.blend(tokens.warn.opacity(0.14)))
        .border_1()
        .border_color(tokens.warn.opacity(0.45))
        .text_color(tokens.text)
        .cursor_pointer()
        .on_click(move |_, window, cx| dismiss.dispatch_action(&DismissSnippetNotice, window, cx))
        .child(notice)
}

/// 命令放进一行：多行命令的换行显示成 ↵。
fn one_line(command: &str) -> SharedString {
    command
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{21b5} ")
        .into()
}

/// 片段的右键菜单：输入到终端 / 复制命令 / 编辑… / 删除。菜单派发的 action 送到 `target`（窗口壳的焦点）。
pub fn build_snippet_menu(
    snippet: SnippetId,
    target: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Menu> {
    let target = target.clone();
    Menu::build(window, cx, move |menu, _, _| {
        menu.action_context(target)
            .menu(
                t!("snippets.menu.enter"),
                Box::new(EnterSnippet {
                    snippet,
                    run: false,
                }),
            )
            .menu(t!("snippets.menu.copy"), Box::new(CopySnippet(snippet)))
            .separator()
            .menu(t!("snippets.menu.edit"), Box::new(EditSnippet(snippet)))
            .danger(
                t!("snippets.menu.delete"),
                Box::new(DeleteSnippet(snippet)),
                false,
            )
    })
}

#[cfg(test)]
mod tests {
    use super::one_line;

    #[test]
    fn a_command_of_several_lines_shows_on_one() {
        assert_eq!(one_line("df -h"), "df -h");
        assert_eq!(
            one_line("cd /srv\n\n  git pull\n"),
            "cd /srv \u{21b5} git pull"
        );
    }
}
