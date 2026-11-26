// 参考 since2006/shell-rs @ b2a307a: src/workspace/workspace_view.rs 的 `open_next_prompt`、
// `AuthenticationPromptForm` 和 src/host/host_dialog.rs 的 `ask_to_trust` 重写：
// 样式按 26 号逐屏原型的「连接提示」（主机密钥 / 密码 / 验证码）和「主机密钥变了」，外框是共用的 `ui::modal`。

//! 连接时要问用户的事：第一次见到的主机密钥（信任并连接）、变了的主机密钥（更新并连接，默认取消）、
//! 密码和服务器的键盘交互问题（如验证码）。远程卡片和主机对话框的「测试连接」共用。
//!
//! 对话框不点遮罩关闭、没有右上角的 ×，只能经下面的按钮或 Esc（= 取消）离开，所以每个问题一定有回答。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use crate::app::CatalogIcon;
use crate::backend::{
    AuthenticationRequest, ConnectionPromptField, ConnectionPromptKind, ConnectionPromptReply,
    ConnectionSecret, authentication_fields, authentication_instructions, authentication_title,
    host_key_changed_lead, unknown_host_lead,
};
use crate::i18n::t;
use crate::theme::{Tokens, space};
use crate::ui::DialogLayer;
use crate::ui::form::{form_field, typed_secret};
use crate::ui::modal::{
    FingerprintRow, ModalButtonTone, ModalHead, ModalInputKind, OnClick, modal_button,
    modal_dialog, modal_fingerprint, modal_input, modal_note, modal_question, modal_warn,
    render_modal,
};

/// 回答交给谁：远程卡片的连接，或者测试连接。
type Answer = Box<dyn FnOnce(ConnectionPromptReply, &mut Window, &mut App)>;

pub struct PromptDialog {
    kind: ConnectionPromptKind,
    /// 弹窗头部：哪台主机在问（主机名和 `user@地址:端口`）。
    head: ModalHead,
    /// 认证问题的输入框，和服务器给的字段一一对应。
    fields: Vec<(ConnectionPromptField, Entity<InputState>)>,
    answer: Option<Answer>,
    /// 对话框是哪一层：回答时只关自己。
    layer: DialogLayer,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl PromptDialog {
    fn new(
        head: ModalHead,
        kind: ConnectionPromptKind,
        answer: Answer,
        layer: DialogLayer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fields: Vec<_> = match &kind {
            ConnectionPromptKind::Authentication(prompt) => authentication_fields(prompt)
                .into_iter()
                .map(|field| {
                    let echo = field.echo();
                    let input = cx.new(|cx| InputState::new(window, cx).masked(!echo));
                    (field, input)
                })
                .collect(),
            _ => Vec::new(),
        };
        // 在输入框里按回车就是「确定」。
        let subscriptions = fields
            .iter()
            .map(|(_, input)| {
                cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.submit(window, cx);
                    }
                })
            })
            .collect();
        Self {
            kind,
            head,
            fields,
            answer: Some(answer),
            layer,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// 回答，关掉对话框。只回答一次。
    fn reply(&mut self, reply: ConnectionPromptReply, window: &mut Window, cx: &mut Context<Self>) {
        let Some(answer) = self.answer.take() else {
            return;
        };
        self.layer.close(window, cx);
        answer(reply, window, cx);
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reply(ConnectionPromptReply::Cancel, window, cx);
    }

    /// 信任主机密钥，或者交上输入框里的字。
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reply = match &self.kind {
            ConnectionPromptKind::Authentication(_) => ConnectionPromptReply::Answers(
                self.fields
                    .iter()
                    .map(|(_, input)| {
                        let typed = typed_secret(input, cx);
                        ConnectionSecret::new(typed.as_str())
                    })
                    .collect(),
            ),
            _ => ConnectionPromptReply::TrustAndSave,
        };
        self.reply(reply, window, cx);
    }

    fn title(&self) -> SharedString {
        match &self.kind {
            ConnectionPromptKind::UnknownHost(_) => t!("connect.unknown_host.title"),
            ConnectionPromptKind::HostKeyChanged(_) => t!("connect.key_changed.title"),
            ConnectionPromptKind::Authentication(prompt) => authentication_title(prompt).into(),
        }
    }

    /// 标题下面那段话。
    fn lead(&self) -> SharedString {
        match &self.kind {
            ConnectionPromptKind::UnknownHost(prompt) => unknown_host_lead(prompt).into(),
            ConnectionPromptKind::HostKeyChanged(prompt) => host_key_changed_lead(prompt).into(),
            ConnectionPromptKind::Authentication(prompt) => {
                authentication_instructions(prompt).into()
            }
        }
    }

    /// 对话框里的全部文字，一行一句，测试靠它断言。
    fn description(&self) -> SharedString {
        let mut lines = vec![self.title().to_string(), self.lead().to_string()];
        match &self.kind {
            ConnectionPromptKind::UnknownHost(prompt) => {
                lines.push(format!("{} {}", prompt.algorithm(), prompt.fingerprint()));
                lines.push(t!("connect.unknown_host.note").to_string());
            }
            ConnectionPromptKind::HostKeyChanged(prompt) => {
                for old in prompt.old_fingerprints() {
                    lines.push(t!("connect.key_changed.saved", fingerprint = old).to_string());
                }
                lines.push(
                    t!(
                        "connect.key_changed.now",
                        fingerprint = prompt.fingerprint()
                    )
                    .to_string(),
                );
            }
            ConnectionPromptKind::Authentication(_) => {
                lines.extend(
                    self.fields
                        .iter()
                        .map(|(field, _)| field.label().to_string()),
                );
            }
        }
        lines.join("\n").into()
    }

    /// 指纹框的行：类型，再是指纹（`SHA256:…` 拆成标签 SHA256 和后面那串）。
    fn fingerprint_rows(algorithm: &str, fingerprint: &str) -> Vec<FingerprintRow> {
        let (label, value) = match fingerprint.split_once(':') {
            Some((hash, value)) if !hash.is_empty() && !hash.contains(' ') => {
                (SharedString::from(hash.to_string()), value.to_string())
            }
            _ => (t!("connect.fingerprint.value"), fingerprint.to_string()),
        };
        vec![
            FingerprintRow {
                label: t!("connect.fingerprint.algorithm"),
                value: algorithm.to_string().into(),
                old: false,
            },
            FingerprintRow {
                label,
                value: value.into(),
                old: false,
            },
        ]
    }

    fn render_body(&self, tokens: &Tokens, window: &Window, cx: &App) -> AnyElement {
        match &self.kind {
            // 26 号「连接提示 · 主机密钥」：问话、指纹框、说明。
            ConnectionPromptKind::UnknownHost(prompt) => div()
                .flex()
                .flex_col()
                .gap(space::SPACE_8)
                .child(modal_question(self.lead(), tokens))
                .child(modal_fingerprint(
                    Self::fingerprint_rows(prompt.algorithm(), prompt.fingerprint()),
                    tokens,
                    cx,
                ))
                .child(modal_note(t!("connect.unknown_host.note"), tokens))
                .into_any_element(),
            // 26 号「主机密钥变了」：红色警示行、问话、类型 / 记下的（淡）/ 现在的。
            ConnectionPromptKind::HostKeyChanged(prompt) => {
                let mut rows = vec![FingerprintRow {
                    label: t!("connect.fingerprint.algorithm"),
                    value: prompt.algorithm().to_string().into(),
                    old: false,
                }];
                // 指纹去掉 `SHA256:` 前缀（26 号）：两行都是同一种，前缀只占地方，放不下整串。
                rows.extend(prompt.old_fingerprints().iter().map(|old| FingerprintRow {
                    label: t!("connect.key_changed.saved_label"),
                    value: without_hash_name(old).into(),
                    old: true,
                }));
                rows.push(FingerprintRow {
                    label: t!("connect.key_changed.now_label"),
                    value: without_hash_name(prompt.fingerprint()).into(),
                    old: false,
                });
                div()
                    .flex()
                    .flex_col()
                    .gap(space::SPACE_8)
                    .child(modal_warn(CatalogIcon::ShieldX, self.title(), true, tokens, cx))
                    .child(modal_question(self.lead(), tokens))
                    .child(modal_fingerprint(rows, tokens, cx))
                    .into_any_element()
            }
            // 26 号「连接提示 · 密码 / 验证码」：问话；服务器自己的问题（键盘交互）先照写它的原话
            // （等宽字），下面一个输入框；密码框右边有眼睛。
            ConnectionPromptKind::Authentication(prompt) => {
                let server_asks = matches!(
                    prompt.request(),
                    AuthenticationRequest::KeyboardInteractive { .. }
                );
                div()
                    .flex()
                    .flex_col()
                    .gap(space::SPACE_8)
                    .child(modal_question(self.lead(), tokens))
                    .children(
                        self.fields
                            .iter()
                            .enumerate()
                            .map(|(index, (field, input))| {
                                let label = SharedString::from(field.label().to_string());
                                let kind = if !field.echo() {
                                    ModalInputKind::Secret
                                } else if server_asks {
                                    ModalInputKind::Mono
                                } else {
                                    ModalInputKind::Text
                                };
                                form_field(
                                    server_asks.then(|| label.clone()),
                                    modal_input(
                                        ("prompt-field", index),
                                        label,
                                        input,
                                        kind,
                                        false,
                                        tokens,
                                        window,
                                        cx,
                                    ),
                                    Vec::new(),
                                    tokens,
                                )
                            }),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_footer(&self, tokens: &Tokens, window: &Window, cx: &mut Context<Self>) -> Div {
        let button = |id: &'static str,
                      label: SharedString,
                      keys: Option<&'static str>,
                      tone: ModalButtonTone,
                      on_click: OnClick,
                      cx: &App| {
            modal_button(id, label, keys, tone, Some(on_click), tokens, window, cx)
        };
        let cancel_tone = if matches!(self.kind, ConnectionPromptKind::HostKeyChanged(_)) {
            ModalButtonTone::Primary
        } else {
            ModalButtonTone::Plain
        };
        let cancel = button(
            "prompt-cancel",
            t!("connect.cancel"),
            Some("Esc"),
            cancel_tone,
            Box::new(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
            cx,
        );
        let submit =
            || -> OnClick { Box::new(cx.listener(|this, _, window, cx| this.submit(window, cx))) };
        let confirm = match &self.kind {
            ConnectionPromptKind::UnknownHost(_) => button(
                "prompt-confirm",
                t!("connect.unknown_host.trust"),
                Some("Enter"),
                ModalButtonTone::Primary,
                submit(),
                cx,
            ),
            // 默认是「取消」；「更新并连接」红字描边、不带快捷键（26 号，用户定）。
            ConnectionPromptKind::HostKeyChanged(_) => button(
                "prompt-confirm",
                t!("connect.key_changed.replace"),
                None,
                ModalButtonTone::Danger,
                submit(),
                cx,
            ),
            ConnectionPromptKind::Authentication(_) => button(
                "prompt-confirm",
                t!("connect.ok"),
                Some("Enter"),
                ModalButtonTone::Primary,
                submit(),
                cx,
            ),
        };
        // 「取消」在左、做事的在右；主机密钥变了也一样，「更新并连接」在最右（2026-11-13 用户定，
        // 改了 26 号的危险按钮在左），默认按钮仍是「取消」。
        div().flex().gap(space::SPACE_6).child(cancel).child(confirm)
    }
}

impl Focusable for PromptDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PromptDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let body = self.render_body(&tokens, window, cx);
        let footer = self.render_footer(&tokens, window, cx);
        render_modal(Some(self.head.clone()), body, footer, &tokens, window, cx)
            .id("connection-prompt")
            .test_support()
            .aria_label(self.description())
            .track_focus(&self.focus_handle)
            // Esc 就是取消：问题一定有回答。没有输入框时回车按默认按钮：确认主机密钥是「信任并连接」，
            // 主机密钥变了是「取消」（有输入框时回车由输入框交上去）。
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => {
                        cx.stop_propagation();
                        this.cancel(window, cx);
                    }
                    "enter" if this.fields.is_empty() => {
                        cx.stop_propagation();
                        if matches!(this.kind, ConnectionPromptKind::HostKeyChanged(_)) {
                            this.cancel(window, cx);
                        } else {
                            this.submit(window, cx);
                        }
                    }
                    _ => {}
                }
            }))
    }
}

/// 指纹去掉开头的哈希名（`SHA256:`）；没有哈希名时原样。
fn without_hash_name(fingerprint: &str) -> String {
    match fingerprint.split_once(':') {
        Some((hash, value)) if !hash.is_empty() && !hash.contains(' ') => value.to_string(),
        _ => fingerprint.to_string(),
    }
}

/// 问用户 `kind`，回答交给 `answer`（取消、Esc 也是回答）。`head` 是弹窗头部：哪台主机在问。
/// 有输入框时焦点给第一个，否则给对话框（Esc、回车能用）。返回问答框那一层：问题作废时
/// （等它的卡片都关了）调用方拿它关掉问答框，不交回答。
pub fn open_prompt_dialog(
    head: ModalHead,
    kind: ConnectionPromptKind,
    answer: impl FnOnce(ConnectionPromptReply, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> DialogLayer {
    let layer = DialogLayer::new();
    let view =
        cx.new(|cx| PromptDialog::new(head, kind, Box::new(answer), layer.clone(), window, cx));
    let first_field = view.read(cx).fields.first().map(|(_, input)| input.clone());
    let focus = view.read(cx).focus_handle.clone();
    layer.open(
        move |dialog, _, cx| {
            modal_dialog(dialog, &Tokens::current(cx))
                .keyboard(false)
                .child(view.clone())
        },
        window,
        cx,
    );
    match first_field {
        Some(input) => input.update(cx, |input, cx| input.focus(window, cx)),
        None => window.focus(&focus, cx),
    }
    layer
}
