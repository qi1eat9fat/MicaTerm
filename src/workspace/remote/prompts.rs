// 参考 since2006/shell-rs @ b2a307a: src/workspace/workspace_view.rs 的 `enqueue_prompt`、
// `open_next_prompt`、`finish_prompt`、`prompt_owner_is_live` 重写：
// 不再轮询：一次只开一个问答框，回答了再开下一个；问题属于连接，不属于某张卡片。

//! 连接表的问题排队，一次问一个：同时有几台主机在连时，一个答完再问下一个。
//! 问的时候已经没有卡片在等这条连接了（都关了），就不问，直接取消。

use std::collections::VecDeque;

use gpui_kit::*;

use crate::backend::{ConnectionPromptKind, ConnectionPromptReply, PromptId, RemotePrompt};
use crate::card::CardId;
use crate::connect::open_prompt_dialog;
use crate::workspace::Workspace;

/// 排着的问题，和正在问的那个。
#[derive(Default)]
pub(in crate::workspace) struct PromptQueue {
    waiting: VecDeque<RemotePrompt>,
    shown: Option<PromptId>,
}

impl PromptQueue {
    pub(super) fn push(&mut self, prompt: RemotePrompt) {
        self.waiting.push_back(prompt);
    }
}

impl Workspace {
    /// 没在问别的，就把排着的下一个问题打开。
    pub(super) fn show_next_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompts.shown.is_some() {
            return;
        }
        while let Some(prompt) = self.prompts.waiting.pop_front() {
            let Some(card) = prompt
                .terminals
                .iter()
                .find_map(|terminal| self.card(CardId(terminal.0), cx))
            else {
                // 等它的卡片都关了：连接表也会作废这个问题，这里只是不问。
                self.services
                    .remote_terminals
                    .respond(prompt.id, ConnectionPromptReply::Cancel);
                continue;
            };
            let subtitle = {
                let card = card.read(cx);
                let name = card.name();
                match (&prompt.kind, card.remote_target()) {
                    (ConnectionPromptKind::Authentication(_), Some(target)) => {
                        let login = &target.login;
                        format!("{name} · {}@{}", login.user, login.host).into()
                    }
                    _ => name,
                }
            };
            let id = prompt.id;
            self.prompts.shown = Some(id);
            let remote = self.services.remote_terminals.clone();
            let workspace = cx.weak_entity();
            open_prompt_dialog(
                subtitle,
                prompt.kind,
                move |reply, window, cx| {
                    remote.respond(id, reply);
                    let _ = workspace.update(cx, |this, cx| {
                        this.prompts.shown = None;
                        this.show_next_prompt(window, cx);
                    });
                },
                window,
                cx,
            );
            return;
        }
    }

    /// 正在问的问题，测试用。
    pub fn shown_prompt(&self) -> Option<PromptId> {
        self.prompts.shown
    }
}
