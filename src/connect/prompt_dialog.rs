// 参考 since2006/shell-rs @ b2a307a: src/workspace/workspace_view.rs 的 `open_next_prompt`、
// `AuthenticationPromptForm` 和 src/host/host_dialog.rs 的 `ask_to_trust` 重写：
// 样式按全部界面原型的「第一次连接：确认主机密钥」「主机密钥变了：告警」「连接时询问密码 / 验证码」。

//! 连接时要问用户的事：第一次见到的主机密钥（信任并连接）、变了的主机密钥（更新并连接，默认取消）、
//! 密码和服务器的键盘交互问题（如验证码）。远程卡片和主机对话框的「测试连接」共用。
//!
//! 对话框不点遮罩关闭、没有右上角的 ×，只能经下面的按钮或 Esc（= 取消）离开，所以每个问题一定有回答。

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::backend::{
    ConnectionPromptField, ConnectionPromptKind, ConnectionPromptReply, ConnectionSecret,
    authentication_fields, authentication_instructions, authentication_title,
    host_key_changed_lead, unknown_host_lead,
};
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::DialogLayer;
use crate::ui::form::typed_secret;

/// 对话框的宽（原型 `.dlg.small`）。
const WIDTH: Pixels = px(440.);

/// 回答交给谁：远程卡片的连接，或者测试连接。
type Answer = Box<dyn FnOnce(ConnectionPromptReply, &mut Window, &mut App)>;

pub struct PromptDialog {
    kind: ConnectionPromptKind,
    /// 标题后面的灰字：主机名（问密码时还有 `user@地址`）。
    subtitle: SharedString,
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
        subtitle: SharedString,
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
            subtitle,
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

    fn render_fingerprint(
        label: SharedString,
        algorithm: &str,
        fingerprint: &str,
        old: bool,
        tokens: &Tokens,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .mb(px(8.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(tokens.field)
            .font_family("JetBrains Mono")
            .text_size(px(11.5))
            .when(old, |box_| box_.text_color(tokens.faint).line_through())
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().text_color(tokens.faint).child(label))
                    .child(algorithm.to_string()),
            )
            .child(div().child(fingerprint.to_string()))
    }

    fn render_body(&self, tokens: &Tokens) -> AnyElement {
        let lead = div()
            .mb(px(10.))
            .text_size(px(12.5))
            .line_height(relative(1.6))
            .text_color(tokens.muted)
            .child(self.lead());
        match &self.kind {
            ConnectionPromptKind::UnknownHost(prompt) => div()
                .child(lead)
                .child(Self::render_fingerprint(
                    t!("connect.fingerprint.algorithm"),
                    prompt.algorithm(),
                    prompt.fingerprint(),
                    false,
                    tokens,
                ))
                .child(
                    div()
                        .text_size(px(11.5))
                        .line_height(relative(1.6))
                        .text_color(tokens.faint)
                        .child(t!("connect.unknown_host.note")),
                )
                .into_any_element(),
            ConnectionPromptKind::HostKeyChanged(prompt) => div()
                .child(lead.text_color(tokens.bad))
                .children(prompt.old_fingerprints().iter().map(|old| {
                    Self::render_fingerprint(
                        t!("connect.key_changed.saved_label"),
                        prompt.algorithm(),
                        old,
                        true,
                        tokens,
                    )
                }))
                .child(Self::render_fingerprint(
                    t!("connect.key_changed.now_label"),
                    prompt.algorithm(),
                    prompt.fingerprint(),
                    false,
                    tokens,
                ))
                .into_any_element(),
            ConnectionPromptKind::Authentication(_) => div()
                .child(lead)
                .children(
                    self.fields
                        .iter()
                        .enumerate()
                        .map(|(index, (field, input))| {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .mb(px(8.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(tokens.muted)
                                        .child(field.label().to_string()),
                                )
                                .child(
                                    Input::new(input)
                                        .id(("prompt-field", index))
                                        .aria_label(SharedString::from(field.label().to_string()))
                                        .small(),
                                )
                        }),
                )
                .into_any_element(),
        }
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cancel = Button::new("prompt-cancel")
            .small()
            .label(t!("connect.cancel"))
            .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx)));
        let confirm = Button::new("prompt-confirm")
            .small()
            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)));
        let (cancel, confirm) = match &self.kind {
            ConnectionPromptKind::UnknownHost(_) => (
                cancel,
                confirm.primary().label(t!("connect.unknown_host.trust")),
            ),
            // 默认按钮是「取消」。
            ConnectionPromptKind::HostKeyChanged(_) => (
                cancel.primary(),
                confirm.danger().label(t!("connect.key_changed.replace")),
            ),
            ConnectionPromptKind::Authentication(_) => {
                (cancel, confirm.primary().label(t!("connect.ok")))
            }
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pt(px(12.))
            .pb(px(14.))
            .pl(px(18.))
            .pr(px(14.))
            .child(div().flex_1())
            .child(cancel)
            .child(confirm)
    }
}

impl Focusable for PromptDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PromptDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        div()
            .id("connection-prompt")
            .test_support()
            .aria_label(self.description())
            .track_focus(&self.focus_handle)
            // Esc 就是取消：问题一定有回答。
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.cancel(window, cx);
                }
            }))
            .flex()
            .flex_col()
            .text_color(tokens.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(46.))
                    .px(px(18.))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(
                                matches!(self.kind, ConnectionPromptKind::HostKeyChanged(_)),
                                |title| title.text_color(tokens.bad),
                            )
                            .child(self.title()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(tokens.faint)
                            .truncate()
                            .child(self.subtitle.clone()),
                    ),
            )
            .child(div().px(px(18.)).child(self.render_body(&tokens)))
            .child(self.render_footer(cx))
    }
}

/// 问用户 `kind`，回答交给 `answer`（取消、Esc 也是回答）。`subtitle` 写在标题后面：主机名。
/// 有输入框时焦点给第一个，否则给对话框（Esc 能用）。返回问答框那一层：问题作废时
/// （等它的卡片都关了）调用方拿它关掉问答框，不交回答。
pub fn open_prompt_dialog(
    subtitle: SharedString,
    kind: ConnectionPromptKind,
    answer: impl FnOnce(ConnectionPromptReply, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> DialogLayer {
    let layer = DialogLayer::new();
    let view =
        cx.new(|cx| PromptDialog::new(subtitle, kind, Box::new(answer), layer.clone(), window, cx));
    let first_field = view.read(cx).fields.first().map(|(_, input)| input.clone());
    let focus = view.read(cx).focus_handle.clone();
    layer.open(
        move |dialog, _, _| {
            dialog
                .w(WIDTH)
                .p_0()
                .rounded(px(14.))
                .close_button(false)
                .overlay_closable(false)
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
