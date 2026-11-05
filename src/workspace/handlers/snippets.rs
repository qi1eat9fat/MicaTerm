//! 命令片段的处理器（工单 19）：展开、收起面板，开对话框、删除前确认、复制，
//! 以及把片段发给焦点卡片的终端（本地卡片和远程卡片一样，都是 `TerminalView::enter_command`）。
// 参考 since2006/shell-rs @ b2a307a: src/workspace/snippets.rs、src/workspace/tools.rs 的 `on_enter_command`

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::*;

use crate::app::{
    CopySnippet, DeleteSnippet, DismissSnippetNotice, EditSnippet, EnterSnippet, NewSnippet,
    OpenSnippetMenu, ShowSnippets,
};
use crate::card::TerminalCard;
use crate::i18n::t;
use crate::snippets::{
    SnippetPanelProps, build_snippet_menu, confirm_delete_snippet, open_snippet_dialog,
};
use crate::theme::Tokens;
use crate::workspace::Workspace;
use crate::workspace::right_sidebar::RightPanel;

pub(in crate::workspace) fn bind_snippets_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_show_snippets))
        .on_action(cx.listener(Workspace::on_new_snippet))
        .on_action(cx.listener(Workspace::on_edit_snippet))
        .on_action(cx.listener(Workspace::on_delete_snippet))
        .on_action(cx.listener(Workspace::on_copy_snippet))
        .on_action(cx.listener(Workspace::on_enter_snippet))
        .on_action(cx.listener(Workspace::on_open_snippet_menu))
        .on_action(cx.listener(Workspace::on_dismiss_snippet_notice))
}

impl Workspace {
    /// 命令片段面板：要画的片段和「→ 焦点卡片名」由这里取好交给面板。
    pub(in crate::workspace) fn render_snippet_panel(
        &self,
        tokens: &Tokens,
        cx: &App,
    ) -> AnyElement {
        let props = SnippetPanelProps {
            snippets: self.hosts.read(cx).snippets_by_name(),
            target: self
                .active_terminal_tab()
                .map(|tab| tab.focused_card().read(cx).name()),
        };
        self.snippet_panel
            .render(props, &self.focus_handle, tokens, cx)
            .into_any_element()
    }

    /// 右侧栏「命令片段」、Ctrl+Shift+S：展开面板，展开着就收起。
    fn on_show_snippets(&mut self, _: &ShowSnippets, _: &mut Window, cx: &mut Context<Self>) {
        self.right_panel = match self.right_panel {
            Some(RightPanel::Snippets) => None,
            _ => Some(RightPanel::Snippets),
        };
        self.snippet_panel.set_notice(None);
        cx.notify();
    }

    fn on_new_snippet(&mut self, _: &NewSnippet, window: &mut Window, cx: &mut Context<Self>) {
        open_snippet_dialog(None, self.hosts.clone(), window, cx);
    }

    fn on_edit_snippet(
        &mut self,
        &EditSnippet(id): &EditSnippet,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.hosts.read(cx).snippet(id).is_some() {
            open_snippet_dialog(Some(id), self.hosts.clone(), window, cx);
        }
    }

    fn on_delete_snippet(
        &mut self,
        &DeleteSnippet(id): &DeleteSnippet,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        confirm_delete_snippet(id, self.hosts.clone(), window, cx);
    }

    fn on_copy_snippet(
        &mut self,
        &CopySnippet(id): &CopySnippet,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(snippet) = self.hosts.read(cx).snippet(id) {
            cx.write_to_clipboard(ClipboardItem::new_string(snippet.command.clone()));
        }
    }

    /// 把片段发给在前标签的焦点卡片：`run` 时执行，否则只输入；发出去了焦点给那张卡片，
    /// 好接着改或者回车。发不出去（全屏程序、多行命令只输入、没连上）时在面板底部说为什么。
    fn on_enter_snippet(
        &mut self,
        &EnterSnippet { snippet, run }: &EnterSnippet,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(snippet) = self.hosts.read(cx).snippet(snippet).cloned() else {
            return;
        };
        let Some(card) = self
            .active_terminal_tab()
            .map(|tab| tab.focused_card().clone())
        else {
            self.snippet_panel
                .set_notice(Some(t!("snippets.panel.no_terminal")));
            cx.notify();
            return;
        };
        let (name, terminal, replace) = {
            let card = card.read(cx);
            (
                card.name(),
                card.terminal().clone(),
                clears_typed_line(card),
            )
        };
        let result = if terminal.read(cx).is_full_screen(cx) {
            Err(t!(
                "snippets.notice.full_screen",
                card = name,
                snippet = snippet.name
            ))
        } else {
            terminal
                .update(cx, |terminal, cx| {
                    terminal.enter_command(&snippet.command, run, replace, cx)
                })
                .map_err(|why| {
                    t!(
                        "snippets.notice.failed",
                        card = name,
                        snippet = snippet.name,
                        error = why
                    )
                })
        };
        match result {
            Ok(()) => {
                self.snippet_panel.set_notice(None);
                let focus = card.read(cx).focus_handle();
                window.focus(&focus, cx);
            }
            Err(notice) => self.snippet_panel.set_notice(Some(notice)),
        }
        cx.notify();
    }

    /// 右键片段：开菜单，菜单拿焦点；关菜单时焦点回到在前标签的焦点卡片，选的那项随后由上面的处理器执行。
    fn on_open_snippet_menu(
        &mut self,
        &OpenSnippetMenu { snippet, position }: &OpenSnippetMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.hosts.read(cx).snippet(snippet).is_none() {
            return;
        }
        let menu = build_snippet_menu(snippet, &self.focus_handle, window, cx);
        let dismiss = cx.subscribe_in(
            &menu,
            window,
            |this, menu: &Entity<PopupMenu>, _: &DismissEvent, window, cx| {
                this.snippet_panel.close_menu(menu);
                // 选的是「编辑…」「删除」时对话框已经开了，焦点留在对话框里。
                if window.has_active_dialog(cx) {
                    cx.notify();
                    return;
                }
                let focus = this
                    .tabs
                    .active_content()
                    .map(|tab| tab.focus_handle(cx))
                    .unwrap_or_else(|| this.focus_handle.clone());
                window.focus(&focus, cx);
                cx.notify();
            },
        );
        window.focus(&menu.focus_handle(cx), cx);
        self.snippet_panel
            .open_menu(snippet, position, menu, dismiss);
        cx.notify();
    }

    fn on_dismiss_snippet_notice(
        &mut self,
        _: &DismissSnippetNotice,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.snippet_panel.set_notice(None);
        cx.notify();
    }
}

/// 输入前要不要先清掉提示符后面已经打的字（Ctrl-E Ctrl-U，照 shell-rs）。
/// Windows 上的 shell（PowerShell、cmd）不认这两个键，只能接着已有的字往后打。
/// 远程卡片按主机的系统判断（工单 12）：Windows 主机同样不清。
fn clears_typed_line(card: &TerminalCard) -> bool {
    match card.local_shell() {
        Some(_) => !cfg!(windows),
        None => card.remote_os() != Some(qishell_hosts::HostOs::Windows),
    }
}
