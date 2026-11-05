// 参考 since2006/shell-rs @ b2a307a: src/workspace/workspace_view.rs 的 `enqueue_prompt`、
// `open_next_prompt`、`finish_prompt`、`prompt_owner_is_live` 重写：
// 不再轮询：一次只开一个问答框，回答了再开下一个；问题属于连接，不属于某张卡片。

//! 连接表的问题排队，一次问一个：同时有几台主机在连时，一个答完再问下一个。
//! 问的时候已经没有卡片在等这条连接了（都关了），就不问，直接取消。

use std::collections::VecDeque;

use gpui_kit::*;

use crate::backend::{
    ConnectionPromptKind, ConnectionPromptReply, JumpLogin, LoginRoute, PromptId, RemotePrompt,
    RemoteTarget,
};
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
                prompt_subtitle(card.name(), &prompt.kind, card.remote_target())
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

/// 问答框标题后面的灰字：卡片名；问密码时再加上是以谁登录哪台，跳板主机问的写跳板主机的
/// （工单 15：问题的正文已经写明是哪台跳板主机）。
fn prompt_subtitle(
    name: SharedString,
    kind: &ConnectionPromptKind,
    target: Option<&RemoteTarget>,
) -> SharedString {
    let (ConnectionPromptKind::Authentication(question), Some(target)) = (kind, target) else {
        return name;
    };
    let login = match (question.jump_host(), &target.login.route) {
        (None, _) => Some(&target.login),
        (Some(jump), LoginRoute::Jump(hops)) => hops.iter().find_map(|hop| match hop {
            JumpLogin::Host { name, login } if name == jump => Some(&**login),
            _ => None,
        }),
        (Some(_), _) => None,
    };
    match login {
        Some(login) => format!("{name} \u{b7} {}@{}", login.user, login.host).into(),
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::prompt_subtitle;
    use crate::backend::{
        AuthenticationRequest, ConnectionPromptKind, HostLogin, JumpLogin, LoginRoute, RemoteTarget,
    };
    use qishell_ssh::LoginMethod;

    fn password(jump_host: Option<&str>) -> ConnectionPromptKind {
        ConnectionPromptKind::authentication(AuthenticationRequest::Password {
            saved_rejected: false,
        })
        .at_jump_host(jump_host)
    }

    #[test]
    fn a_password_asked_at_a_jump_host_names_the_jump_hosts_login() {
        let jump = HostLogin::new("203.0.113.7", 2222, "admin", LoginMethod::Password);
        let target = RemoteTarget {
            host: 7,
            login: HostLogin::new("10.0.2.8", 22, "root", LoginMethod::Password).with_route(
                LoginRoute::Jump(vec![JumpLogin::Host {
                    name: "jp-gateway".into(),
                    login: Box::new(jump),
                }]),
            ),
        };
        let subtitle = |kind| prompt_subtitle("db-master".into(), &kind, Some(&target));

        assert_eq!(subtitle(password(None)), "db-master · root@10.0.2.8");
        assert_eq!(
            subtitle(password(Some("jp-gateway"))),
            "db-master · admin@203.0.113.7"
        );
        // 主机密钥的问题只写卡片名。
        assert_eq!(
            subtitle(ConnectionPromptKind::unknown_host(
                "203.0.113.7",
                2222,
                "ssh-ed25519",
                "SHA256:abc"
            )),
            "db-master"
        );
    }
}
