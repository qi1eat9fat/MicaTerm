//! 编辑器标签的处理器（工单 23）：SFTP 标签读好的文件开成标签，同一个文件只开一个；保存、重新加载；
//! 保存后刷新文件卡片。
// 参考 since2006/shell-rs @ b2a307a: src/workspace/editors.rs

use std::rc::Rc;

use gpui_kit::*;

use crate::app::{ReloadEditor, SaveEditor};
use crate::backend::sftp::TextFile;
use crate::editor::{EditorLocation, EditorOptions, EditorTab, EditorTabEvent};
use crate::i18n::t;
use crate::sftp_tab::SftpTab;
use crate::tabs::{Tab, TabId};
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_editor_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_save_editor))
        .on_action(cx.listener(Workspace::on_reload_editor))
}

impl Workspace {
    /// 在前的编辑器标签，测试和处理器用。
    pub fn active_editor_tab(&self) -> Option<Entity<EditorTab>> {
        self.tabs.active_content().and_then(Tab::as_editor).cloned()
    }

    fn on_save_editor(&mut self, _: &SaveEditor, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor_tab() {
            editor.update(cx, |editor, cx| editor.save(window, cx));
        }
    }

    fn on_reload_editor(&mut self, _: &ReloadEditor, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor_tab() {
            editor.update(cx, |editor, cx| editor.request_reload(window, cx));
        }
    }

    /// 已经开着这个文件的标签：远程文件看是不是这个 SFTP 标签打开的（一台主机只有一个 SFTP 标签），
    /// 本地文件只看路径。
    fn editor_tab_of(
        &self,
        sftp: &Entity<SftpTab>,
        location: &EditorLocation,
        cx: &App,
    ) -> Option<TabId> {
        self.tabs.find(|tab| {
            tab.as_editor().is_some_and(|editor| {
                let editor = editor.read(cx);
                editor.location() == location
                    && (!location.is_remote()
                        || editor
                            .sftp()
                            .is_some_and(|owner| owner.entity_id() == sftp.entity_id()))
            })
        })
    }

    /// 用户要编辑一个文件：开着就切过去，不再读一遍；否则让 SFTP 标签去读。
    pub(in crate::workspace) fn on_edit_file(
        &mut self,
        sftp: &Entity<SftpTab>,
        location: &EditorLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.editor_tab_of(sftp, location, cx) {
            self.activate_tab(tab, window, cx);
            return;
        }
        let location = location.clone();
        sftp.update(cx, |sftp, cx| sftp.read_for_editor(location, window, cx));
    }

    /// SFTP 标签读好了文件：开一个编辑器标签（读的时候又打开了一次就切过去）。
    pub(in crate::workspace) fn on_file_read(
        &mut self,
        sftp: &Entity<SftpTab>,
        location: &EditorLocation,
        file: &Rc<TextFile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.editor_tab_of(sftp, location, cx) {
            self.activate_tab(tab, window, cx);
            return;
        }
        let (place, owner) = if location.is_remote() {
            (sftp.read(cx).rail_name(), Some(sftp.downgrade()))
        } else {
            (t!("sftp_tab.local_name"), None)
        };
        let options = EditorOptions {
            location: location.clone(),
            place,
            sftp: owner,
            local_files: self.services.local_files.clone(),
            file: (**file).clone(),
            dispatch: self.focus_handle.clone(),
        };
        let editor = cx.new(|cx| EditorTab::new(options, window, cx));
        self.open_editor_tab(editor, window, cx);
    }

    /// 窗口壳开始听这个编辑器标签。
    pub(in crate::workspace) fn watch_editor(
        &mut self,
        editor: &Entity<EditorTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(editor, window, Self::on_editor_event)
            .detach();
    }

    fn on_editor_event(
        &mut self,
        editor: &Entity<EditorTab>,
        event: &EditorTabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorTabEvent::Changed => cx.notify(),
            EditorTabEvent::Saved => self.refresh_cards_showing(editor, window, cx),
            EditorTabEvent::SaveEnded { saved } => self.on_save_ended(*saved, window, cx),
            EditorTabEvent::CloseRequested => {
                let tab = self.tabs.find(|tab| tab.as_editor() == Some(editor));
                if let Some(tab) = tab {
                    self.close_tab(tab, window, cx);
                }
            }
        }
    }

    /// 文件卡片显示新的大小和时间：远程文件是打开它的 SFTP 标签，本地文件是所有 SFTP 标签。
    fn refresh_cards_showing(
        &mut self,
        editor: &Entity<EditorTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (location, owner) = {
            let editor = editor.read(cx);
            (
                editor.location().clone(),
                editor.sftp().map(|sftp| sftp.entity_id()),
            )
        };
        let tabs: Vec<Entity<SftpTab>> = self
            .tabs
            .iter()
            .filter_map(|(_, tab)| tab.as_sftp())
            .filter(|tab| owner.is_none_or(|owner| tab.entity_id() == owner))
            .cloned()
            .collect();
        for tab in tabs {
            tab.update(cx, |tab, cx| tab.reload_if_showing(&location, window, cx));
        }
    }
}
