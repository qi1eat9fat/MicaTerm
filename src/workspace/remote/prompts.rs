// 参考 since2006/shell-rs @ b2a307a: src/workspace/workspace_view.rs 的 `enqueue_prompt`、
// `open_next_prompt`、`finish_prompt`、`prompt_owner_is_live` 重写：
// 不再轮询：一次只开一个问答框，回答了再开下一个；问题属于连接，不属于某张卡片。

//! 连接的问题排队，一次问一个：同时有几台主机在连时，一个答完再问下一个。
//! 问题有两个来源（`PromptSource`）：连接表（卡片共用的连接）和 SFTP 标签（它自己的连接）。
//! 问的时候已经没有人在等这条连接了（卡片都关了、SFTP 标签关了或者重连了），就不问，直接取消。

use std::collections::VecDeque;

use gpui_kit::*;

use crate::app::CatalogIcon;
use crate::backend::{
    ConnectionId, ConnectionPrompt, ConnectionPromptKind, ConnectionPromptReply, HostLogin,
    JumpLogin, LoginRoute, PromptId, RemotePrompt,
};
use crate::card::CardId;
use crate::connect::open_prompt_dialog;
use crate::sftp_tab::SftpTabId;
use crate::ui::DialogLayer;
use crate::ui::modal::ModalHead;
use crate::workspace::Workspace;

/// 问题从哪条连接来，回答交回哪里。
enum PromptSource {
    /// 连接表：卡片共用的连接。
    Remote(RemotePrompt),
    /// SFTP 标签自己的连接。`generation` 是问的时候连接的代号：标签重连过，这个问题就作废了。
    Sftp {
        tab: SftpTabId,
        generation: u64,
        prompt: ConnectionPrompt,
    },
}

/// 正在问的问题是谁的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    Remote(PromptId),
    Sftp { tab: SftpTabId, request: u64 },
}

/// 排着的问题，和正在问的那个（连同它的问答框那一层）。
#[derive(Default)]
pub(in crate::workspace) struct PromptQueue {
    waiting: VecDeque<PromptSource>,
    shown: Option<(Asked, DialogLayer)>,
}

impl PromptQueue {
    pub(super) fn push(&mut self, prompt: RemotePrompt) {
        self.waiting.push_back(PromptSource::Remote(prompt));
    }
}

impl Workspace {
    /// 连接 `connection` 的问题都作废了（等它的卡片都关了）：排着的不再问，正在问的关掉问答框
    /// （不交回答），接着问下一个。
    pub(super) fn withdraw_prompts(
        &mut self,
        connection: ConnectionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompts.waiting.retain(|prompt| match prompt {
            PromptSource::Remote(prompt) => prompt.id.connection != connection,
            PromptSource::Sftp { .. } => true,
        });
        let shown_here = matches!(
            &self.prompts.shown,
            Some((Asked::Remote(id), _)) if id.connection == connection
        );
        if shown_here {
            self.close_shown_prompt(window, cx);
        }
    }

    /// SFTP 标签 `tab` 关了：它的问题作废，排着的不再问，正在问的关掉问答框。
    pub(in crate::workspace) fn withdraw_sftp_prompts(
        &mut self,
        tab: SftpTabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompts.waiting.retain(|prompt| match prompt {
            PromptSource::Sftp { tab: owner, .. } => *owner != tab,
            PromptSource::Remote(_) => true,
        });
        let shown_here = matches!(
            &self.prompts.shown,
            Some((Asked::Sftp { tab: owner, .. }, _)) if *owner == tab
        );
        if shown_here {
            self.close_shown_prompt(window, cx);
        }
    }

    fn close_shown_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, layer)) = self.prompts.shown.take() {
            layer.close(window, cx);
        }
        self.show_next_prompt(window, cx);
    }

    /// SFTP 标签 `tab` 的连接要问用户：进同一个队列。
    pub(in crate::workspace) fn ask_for_sftp_tab(
        &mut self,
        tab: SftpTabId,
        generation: u64,
        prompt: ConnectionPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompts.waiting.push_back(PromptSource::Sftp {
            tab,
            generation,
            prompt,
        });
        self.show_next_prompt(window, cx);
    }

    /// 没在问别的，就把排着的下一个问题打开。
    pub(super) fn show_next_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompts.shown.is_some() {
            return;
        }
        while let Some(prompt) = self.prompts.waiting.pop_front() {
            let shown = match prompt {
                PromptSource::Remote(prompt) => self.show_remote_prompt(prompt, window, cx),
                PromptSource::Sftp {
                    tab,
                    generation,
                    prompt,
                } => self.show_sftp_prompt(tab, generation, prompt, window, cx),
            };
            if shown {
                return;
            }
        }
    }

    /// 打开连接表的问题；没有卡片在等了就取消它，返回 `false`。
    fn show_remote_prompt(
        &mut self,
        prompt: RemotePrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(card) = prompt
            .terminals
            .iter()
            .find_map(|terminal| self.card(CardId(terminal.0), cx))
        else {
            // 等它的卡片都关了：连接表也会作废这个问题，这里只是不问。
            self.services
                .remote_terminals
                .respond(prompt.id, ConnectionPromptReply::Cancel);
            return false;
        };
        let head = {
            let card = card.read(cx);
            prompt_head(
                card.name(),
                &prompt.kind,
                card.remote_target().map(|target| &target.login),
            )
        };
        let id = prompt.id;
        let remote = self.services.remote_terminals.clone();
        let workspace = cx.weak_entity();
        let layer = open_prompt_dialog(
            head,
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
        self.prompts.shown = Some((Asked::Remote(id), layer));
        true
    }

    /// 打开 SFTP 标签的问题；标签关了或者重连过就不问，返回 `false`。
    fn show_sftp_prompt(
        &mut self,
        id: SftpTabId,
        generation: u64,
        prompt: ConnectionPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.sftp_tab_by_id(id, cx) else {
            return false;
        };
        if tab.read(cx).generation() != generation {
            return false;
        }
        let head = {
            let tab = tab.read(cx);
            prompt_head(tab.host().name.clone(), prompt.kind(), Some(tab.login()))
        };
        let request = prompt.request_id();
        let workspace = cx.weak_entity();
        let layer = open_prompt_dialog(
            head,
            prompt.kind().clone(),
            move |reply, window, cx| {
                tab.read(cx).reply_to_prompt(request, reply);
                let _ = workspace.update(cx, |this, cx| {
                    this.prompts.shown = None;
                    this.show_next_prompt(window, cx);
                });
            },
            window,
            cx,
        );
        self.prompts.shown = Some((Asked::Sftp { tab: id, request }, layer));
        true
    }

    /// 正在问的连接表的问题，测试用。
    pub fn shown_prompt(&self) -> Option<PromptId> {
        match &self.prompts.shown {
            Some((Asked::Remote(id), _)) => Some(*id),
            _ => None,
        }
    }

    /// 正在问的 SFTP 标签的问题（请求号），测试用。
    pub fn shown_sftp_prompt(&self) -> Option<u64> {
        match &self.prompts.shown {
            Some((Asked::Sftp { request, .. }, _)) => Some(*request),
            _ => None,
        }
    }
}

/// 问答弹窗的头部（26 号 `.cp-h`）：卡片名，后面淡字写被问的是哪个登录。认证问题写那个登录的
/// `user@地址:端口`（问的是跳板主机时写跳板主机的）；主机密钥的问题写服务器的 `地址:端口`。
fn prompt_head(
    name: SharedString,
    kind: &ConnectionPromptKind,
    target: Option<&HostLogin>,
) -> ModalHead {
    let detail = match kind {
        ConnectionPromptKind::UnknownHost(prompt) => {
            Some(format!("{}:{}", prompt.host(), prompt.port()))
        }
        ConnectionPromptKind::HostKeyChanged(prompt) => {
            Some(format!("{}:{}", prompt.host(), prompt.port()))
        }
        ConnectionPromptKind::Authentication(question) => target
            .and_then(|target| match (question.jump_host(), &target.route) {
                (None, _) => Some(target),
                (Some(jump), LoginRoute::Jump(hops)) => hops.iter().find_map(|hop| match hop {
                    JumpLogin::Host { name, login } if name == jump => Some(&**login),
                    _ => None,
                }),
                (Some(_), _) => None,
            })
            .map(|login| format!("{}@{}:{}", login.user, login.host, login.port)),
    };
    ModalHead {
        icon: CatalogIcon::Server,
        title: name,
        detail: detail.map(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::prompt_head;
    use crate::backend::{
        AuthenticationRequest, ConnectionPromptKind, HostLogin, JumpLogin, LoginRoute,
    };
    use micaterm_ssh::LoginMethod;

    fn password(jump_host: Option<&str>) -> ConnectionPromptKind {
        ConnectionPromptKind::authentication(AuthenticationRequest::Password {
            saved_rejected: false,
        })
        .at_jump_host(jump_host)
    }

    #[test]
    fn the_head_names_the_login_being_asked_about() {
        let jump = HostLogin::new("203.0.113.7", 2222, "admin", LoginMethod::Password);
        let target = HostLogin::new("10.0.2.8", 22, "root", LoginMethod::Password).with_route(
            LoginRoute::Jump(vec![JumpLogin::Host {
                name: "jp-gateway".into(),
                login: Box::new(jump),
            }]),
        );
        let detail = |kind| {
            let head = prompt_head("db-master".into(), &kind, Some(&target));
            assert_eq!(head.title.as_ref(), "db-master");
            head.detail.map(|detail| detail.to_string())
        };

        assert_eq!(detail(password(None)).as_deref(), Some("root@10.0.2.8:22"));
        assert_eq!(
            detail(password(Some("jp-gateway"))).as_deref(),
            Some("admin@203.0.113.7:2222")
        );
        // 主机密钥的问题写服务器的地址和端口。
        assert_eq!(
            detail(ConnectionPromptKind::unknown_host(
                "203.0.113.7",
                2222,
                "ssh-ed25519",
                "SHA256:abc"
            ))
            .as_deref(),
            Some("203.0.113.7:2222")
        );
    }
}
