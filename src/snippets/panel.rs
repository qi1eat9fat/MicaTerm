//! 命令片段面板（停靠面板 `ui::dock`，行样式对标 Muxy `agents_panel` / `agent_sessions` 双行圆角列表）：
//! 标题行是图标 +「Snippets」，后面写「→ 焦点卡片名」，右边是「+」和「收起」；列表按名称排序，左右内缩圆角悬浮行、
//! 无横分割线；每条两行：第一行 12px 正文色名称，第二行淡色 `$ ` 提示符 + 11px 等宽命令预览。
//! 单击一律只将命令输入到焦点终端提示符后面（不自动执行）。右键菜单四项。
//!
//! 和标签轨道一样，面板是窗口壳持有的普通结构：渲染不读实体，只拿窗口壳给的片段和焦点卡片名；
//! 点击、菜单都经窗口壳的焦点派发 action，由窗口壳把命令发给焦点卡片。
// 参考 since2006/shell-rs @ b2a307a: src/snippets/snippet_panel.rs 重写（去掉分类、折叠；
// 右键菜单不用 gpui-component 的 `context_menu`，照 `TabRail` 自己持有菜单实体）。

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use micaterm_ui::{Menu, WINDOW_MARGIN};

use crate::app::{
    CatalogIcon, CopySnippet, DeleteSnippet, DismissSnippetNotice, EditSnippet, EnterSnippet,
    NewSnippet, OpenSnippetMenu, ShowSnippets,
};
use crate::host::{Snippet, SnippetId};
use crate::i18n::t;
use crate::theme::{MONO_FONT_FAMILY, Tokens, TypeScale, WEIGHT_NORMAL, size, space};
use crate::ui::dock::{DockClose, DockNew, DockTarget, dock_empty, dock_header, dock_panel};
use crate::ui::icon_button::TextTooltip;

/// 面板跨帧要记住的东西：开着的右键菜单、底部「没有发出去」的提示与滚动位置。
#[derive(Default)]
pub struct SnippetPanel {
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
    pub fn new() -> Self {
        Self::default()
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

    /// 面板（停靠面板 `ui::dock`）。`dispatch` 是窗口壳的焦点：点击和菜单都经它派发。
    pub fn render(
        &self,
        props: SnippetPanelProps,
        dispatch: &FocusHandle,
        tokens: &Tokens,
        cx: &App,
    ) -> impl IntoElement + use<> {
        let SnippetPanelProps { snippets, target } = props;
        let has_target = target.is_some();
        let body = self.render_list(&snippets, dispatch, tokens, cx);
        let to = target.map(|name| DockTarget {
            id: "snippets-target",
            label: t!("snippets.panel.target", card = name.clone()),
            name,
        });
        dock_panel("snippets", tokens)
            .child(dock_header(
                CatalogIcon::CodeXml,
                t!("snippets.panel.title"),
                to,
                DockNew {
                    id: "snippets-new",
                    label: t!("snippets.panel.new"),
                    action: Box::new(NewSnippet),
                },
                DockClose {
                    id: "snippets-close",
                    label: t!("snippets.panel.collapse"),
                    action: Box::new(ShowSnippets),
                },
                dispatch,
                tokens,
                cx,
            ))
            .child(body)
            .children(
                self.notice
                    .clone()
                    .filter(|notice| !(has_target && *notice == t!("snippets.panel.no_terminal")))
                    .map(|notice| render_notice(notice, dispatch, tokens, cx)),
            )
            .children(self.menu.as_ref().map(|menu| {
                deferred(
                    anchored()
                        .position(menu.position)
                        .snap_to_window_with_margin(WINDOW_MARGIN)
                        .child(menu.view.clone()),
                )
                .with_priority(POPUP_PRIORITY)
            }))
            .test_support()
    }

    /// 片段列表：外层内缩 6px、行距 2px 的圆角双行列表；一个片段都没有时写一句话。
    fn render_list(
        &self,
        snippets: &[&Snippet],
        dispatch: &FocusHandle,
        tokens: &Tokens,
        cx: &App,
    ) -> AnyElement {
        let menu_snippet = self.menu_snippet();
        let rows: Vec<AnyElement> = snippets
            .iter()
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
        let empty = snippets.is_empty().then(|| {
            dock_empty(
                "snippets-empty",
                t!("snippets.panel.empty"),
                Some(t!("snippets.panel.empty_hint")),
                tokens,
                cx,
            )
        });
        div()
            .id("snippets-list")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(space::SPACE_2)
            .px(space::SPACE_6)
            .pt(space::SPACE_6)
            .pb(space::SPACE_8)
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .children(rows)
            .children(empty)
            .into_any_element()
    }
}

/// 一条片段（对标 Muxy 双行圆角侧栏列表）：6px 圆角悬浮行，第一行 12px 正文色名称，
/// 第二行淡色 `$` 提示符 + 11px 等宽命令预览；单击输入到焦点终端但不执行。`pressed`：它的右键菜单开着。
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
    let hover = tokens.hover;
    let command = SharedString::from(snippet.command.clone());
    let (click, menu) = (dispatch.clone(), dispatch.clone());
    div()
        .id(("snippet", id.0))
        .test_support()
        .aria_label(SharedString::from(format!("{} · {line}", snippet.name)))
        .flex()
        .flex_none()
        .items_center()
        .h(size::SNIPPET_ROW)
        .px(space::SPACE_8)
        .rounded(size::RADIUS_MD)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        // 被右键的那一行描一圈细边并保留淡悬停底。
        .when(pressed, |row| {
            row.bg(hover).border_1().border_color(tokens.line_strong)
        })
        .when(!pressed, |row| {
            row.tooltip(move |_, cx| TextTooltip::view(command.clone(), cx))
        })
        // 单击一律只把命令输入到提示符后面，不自动回车执行。
        .on_click(move |_, window, cx| click.dispatch_action(&EnterSnippet(id), window, cx))
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
                .gap(px(1.))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(scale.body)
                        .line_height(px(16.))
                        .font_weight(WEIGHT_NORMAL)
                        .text_color(tokens.text)
                        .child(snippet.name.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::SPACE_4)
                        .min_w_0()
                        .font_family(MONO_FONT_FAMILY)
                        .text_size(scale.sm)
                        .line_height(px(14.))
                        .child(div().flex_none().text_color(tokens.faint).child("$"))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_color(tokens.st_dim)
                                .child(line),
                        ),
                ),
        )
}

/// 面板底部的提示条：片段为什么没有发出去。点一下收起。
fn render_notice(
    notice: SharedString,
    dispatch: &FocusHandle,
    tokens: &Tokens,
    cx: &App,
) -> impl IntoElement {
    let scale = TypeScale::current(cx);
    let dismiss = dispatch.clone();
    div()
        .id("snippets-notice")
        .test_support()
        .aria_label(notice.clone())
        .absolute()
        .left(px(8.))
        .right(px(8.))
        .bottom(px(8.))
        .px(space::SPACE_10)
        .py(space::SPACE_6)
        .rounded(size::RADIUS_MD)
        // 浮在列表上面，要不透明：内容底上叠 14% 的警示黄。
        .bg(tokens.content.blend(tokens.warn_soft))
        .border_1()
        .border_color(tokens.warn_line)
        .text_size(scale.sm)
        .line_height(px(16.))
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
            .menu(t!("snippets.menu.enter"), Box::new(EnterSnippet(snippet)))
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
