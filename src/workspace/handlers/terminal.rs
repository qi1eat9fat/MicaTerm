//! 终端的处理器（工单 03）：找到在前那个标签的焦点卡片里的终端，转发复制、粘贴、全选、查找、
//! 清屏；字号改的是设置里的终端字号，写穿到文件，再经 `apply_terminal` 让开着的终端都跟着变。
// 参考 since2006/shell-rs @ b2a307a: src/workspace/workspace_view.rs（终端处理器、终端字号）

use gpui_kit::*;
use qishell_terminal::actions::{
    ClearTerminal, CopyTerminal, DismissTerminalFind, FindInTerminal, FindNextInTerminal,
    FindPreviousInTerminal, PasteTerminal, SelectAllTerminal, ZoomTerminalIn, ZoomTerminalOut,
    ZoomTerminalReset,
};
use qishell_terminal::{DEFAULT_FONT_SIZE, SearchDirection, TerminalView};

use crate::workspace::Workspace;

/// 终端字号一次变多少像素。
const TERMINAL_ZOOM_STEP: f32 = 1.;

pub(in crate::workspace) fn bind_terminal_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_copy_terminal))
        .on_action(cx.listener(Workspace::on_paste_terminal))
        .on_action(cx.listener(Workspace::on_select_all_terminal))
        .on_action(cx.listener(Workspace::on_find_in_terminal))
        .on_action(cx.listener(Workspace::on_find_next_in_terminal))
        .on_action(cx.listener(Workspace::on_find_previous_in_terminal))
        .on_action(cx.listener(Workspace::on_dismiss_terminal_find))
        .on_action(cx.listener(Workspace::on_clear_terminal))
        .on_action(cx.listener(Workspace::on_zoom_terminal_in))
        .on_action(cx.listener(Workspace::on_zoom_terminal_out))
        .on_action(cx.listener(Workspace::on_zoom_terminal_reset))
}

impl Workspace {
    /// 在终端动作的对象上做 `action`：对象是在前那个标签的焦点卡片里的终端，焦点在查找条里时也是它
    /// （查找条是终端的兄弟节点，不在终端的焦点里）。没有标签时什么都不做。
    fn with_active_terminal(
        &self,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut TerminalView, &mut Context<TerminalView>),
    ) {
        if let Some(terminal) = self
            .active_terminal_tab()
            .map(|tab| tab.focused_terminal(cx))
        {
            terminal.update(cx, action);
        }
    }

    fn on_copy_terminal(&mut self, _: &CopyTerminal, _: &mut Window, cx: &mut Context<Self>) {
        self.with_active_terminal(cx, |terminal, cx| terminal.copy_selection(cx));
    }

    fn on_paste_terminal(&mut self, _: &PasteTerminal, _: &mut Window, cx: &mut Context<Self>) {
        self.with_active_terminal(cx, |terminal, cx| terminal.paste_clipboard(cx));
    }

    fn on_select_all_terminal(
        &mut self,
        _: &SelectAllTerminal,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with_active_terminal(cx, |terminal, cx| terminal.select_all(cx));
    }

    fn on_find_in_terminal(
        &mut self,
        _: &FindInTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with_active_terminal(cx, |terminal, cx| terminal.open_find(window, cx));
    }

    fn on_find_next_in_terminal(
        &mut self,
        _: &FindNextInTerminal,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with_active_terminal(cx, |terminal, cx| {
            terminal.step_find(SearchDirection::Down, cx)
        });
    }

    fn on_find_previous_in_terminal(
        &mut self,
        _: &FindPreviousInTerminal,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with_active_terminal(cx, |terminal, cx| {
            terminal.step_find(SearchDirection::Up, cx)
        });
    }

    fn on_dismiss_terminal_find(
        &mut self,
        _: &DismissTerminalFind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with_active_terminal(cx, |terminal, cx| terminal.dismiss_find(window, cx));
    }

    fn on_clear_terminal(&mut self, _: &ClearTerminal, _: &mut Window, cx: &mut Context<Self>) {
        self.with_active_terminal(cx, |terminal, cx| terminal.clear(cx));
    }

    /// 终端字号经设置改：设置把它收进范围、存进文件，下次启动还是这个字号。
    fn change_terminal_font_size(&mut self, size: impl FnOnce(f32) -> f32, cx: &mut App) {
        self.settings.update(cx, |store, cx| {
            store.update(
                |settings| settings.terminal_font.size = size(settings.terminal_font.size),
                cx,
            )
        });
    }

    fn on_zoom_terminal_in(&mut self, _: &ZoomTerminalIn, _: &mut Window, cx: &mut Context<Self>) {
        self.change_terminal_font_size(|size| size + TERMINAL_ZOOM_STEP, cx);
    }

    fn on_zoom_terminal_out(
        &mut self,
        _: &ZoomTerminalOut,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_terminal_font_size(|size| size - TERMINAL_ZOOM_STEP, cx);
    }

    fn on_zoom_terminal_reset(
        &mut self,
        _: &ZoomTerminalReset,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_terminal_font_size(|_| DEFAULT_FONT_SIZE, cx);
    }
}
