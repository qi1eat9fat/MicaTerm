// 参考 since2006/shell-rs @ b2a307a: src/host/host_dialog.rs 重写
// 裁剪：去掉分组、协议（Telnet、串口）、临时连接；版式改成 spec 的变体 B（宽约 500、圆角 14、
// 单页分组可以滚动）；校验出错不再弹通知，而是字段描红、下面写原因。

//! 主机对话框：新建和编辑主机共用。居中，宽约 500，圆角 14，背后的窗口压暗；单页分四组，
//! 可以滚动；底部左边是「测试连接」，右边是「取消」「保存」（spec 第 4 节「编辑表单」）。
//!
//! 每组一个文件、一个结构，`commit` 依次调它们的 `fill` 把值写进 `HostDraft`、记下出错的字段：
//! - `general`：名称（留空用地址）、地址、端口；
//! - `auth`：认证（本工单只有「密码」；13 改密码框的锁定显示，14 加「使用凭据」「无密码」）；
//! - `route`：连接方式（本工单只有「直接」；15 加跳板和代理）；
//! - `notes`：备注。
//!
//! 校验：出错的字段描红，下面写原因，保存失败时对话框不关闭。
//!
//! 「测试连接」（工单 12）用表单里现在的值真的登录一次（`ConnectionTester`），第一次见到的主机密钥
//! 叠一个确认框问；结果写在按钮旁边。

mod auth;
mod delete;
pub mod form;
mod general;
mod notes;
mod route;

use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, IconName, Sizable as _, WindowExt as _};
use gpui_kit::*;

use futures::FutureExt as _;
use qishell_hosts::{AuthKind, Host, HostDraft, HostDraftError, HostId, SSH_PORT};

pub use auth::{AUTH_CHOICE, FIELD_PASSWORD, FIELD_USER};
pub use delete::open_delete_host_dialog;
pub use general::{FIELD_ADDRESS, FIELD_NAME, FIELD_PORT};
pub use notes::FIELD_NOTES;
pub use route::ROUTE_CHOICE;

use self::auth::AuthSection;
use self::form::FieldErrors;
use self::general::GeneralSection;
use self::notes::NotesSection;
use self::route::RouteSection;
use super::describe::ErrorMessage as _;
use super::store::HostStore;
use crate::backend::{
    ConnectionPromptKind, ConnectionPromptReply, LoginTest, SharedConnectionTester, TrustAsker,
    UnknownHostPrompt,
};
use crate::connect::open_prompt_dialog;
use crate::i18n::t;
use crate::theme::Tokens;

/// 对话框的键上下文。
pub const HOST_DIALOG_KEY_CONTEXT: &str = "HostDialog";

/// 对话框的宽（原型 `.dlg.B`）和最高高度。
const WIDTH: Pixels = px(500.);
const MAX_HEIGHT: Pixels = px(640.);

/// 主机对话框的内容。每次打开新建一个。
pub struct HostDialog {
    store: Entity<HostStore>,
    /// 编辑的主机；新建时是 `None`。
    editing: Option<HostId>,
    /// 编辑时标题后面的灰字：主机原来的名字。
    editing_name: Option<SharedString>,
    general: GeneralSection,
    auth: AuthSection,
    route: RouteSection,
    notes: NotesSection,
    errors: FieldErrors,
    /// 「测试连接」用的测试器（工单 12）。
    tester: SharedConnectionTester,
    /// 正在测试；测试时按钮不能再点。
    testing: Option<Task<()>>,
    /// 上一次测试的结果：成功，或者给用户看的原因。
    tested: Option<Result<(), SharedString>>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// 测试连接要确认主机密钥时，问题和回答的去处。
type TrustQuestion = (UnknownHostPrompt, futures::channel::oneshot::Sender<bool>);

impl HostDialog {
    pub fn new(
        store: Entity<HostStore>,
        editing: Option<HostId>,
        tester: SharedConnectionTester,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let host = editing.and_then(|id| store.read(cx).host(id).cloned());
        let editing = host.as_ref().map(|host| host.id);
        let draft = host
            .as_ref()
            .map(|host| host.draft())
            .unwrap_or_else(|| HostDraft::new("", "", SSH_PORT, "", AuthKind::Password));
        let general = GeneralSection::new(&draft, window, cx);
        let auth = AuthSection::new(
            &draft,
            host.as_ref().map(|host| host.password_secret()),
            &store,
            window,
            cx,
        );
        let route = RouteSection::new(&draft);
        let notes = NotesSection::new(&draft, window, cx);
        // 改了哪个字段，就先收起它的错误。
        let subscriptions = general
            .inputs()
            .into_iter()
            .chain(auth.inputs())
            .map(|(field, input)| {
                cx.subscribe(
                    &input,
                    move |this, _: Entity<InputState>, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) && this.errors.clear(field) {
                            cx.notify();
                        }
                    },
                )
            })
            .collect();
        Self {
            store,
            editing,
            editing_name: host.map(|host| host.name),
            general,
            auth,
            route,
            notes,
            errors: FieldErrors::default(),
            tester,
            testing: None,
            tested: None,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// 出错的字段和原因，测试用。
    pub fn errors(&self) -> &FieldErrors {
        &self.errors
    }

    /// 上一次「测试连接」的结果，测试用。
    pub fn test_outcome(&self) -> Option<&Result<(), SharedString>> {
        self.tested.as_ref()
    }

    /// 表单里现在的值，校验过的；出错时记下出错的字段，返回 `None`。
    fn validated_draft(&mut self, cx: &mut Context<Self>) -> Option<HostDraft> {
        let mut errors = FieldErrors::default();
        let mut draft = HostDraft::new("", "", SSH_PORT, "", AuthKind::Password);
        self.general.fill(&mut draft, &mut errors, cx);
        self.auth.fill(&mut draft, &mut errors, cx);
        self.route.fill(&mut draft, &mut errors, cx);
        self.notes.fill(&mut draft, &mut errors, cx);
        let draft = match draft.validated() {
            Ok(draft) => Some(draft),
            Err(error) => {
                errors.set(field_of(error), error.message());
                None
            }
        };
        let draft = draft.filter(|_| errors.is_empty());
        if draft.is_none() {
            self.errors = errors;
            cx.notify();
        }
        draft
    }

    /// 测试连接：用表单里现在的值（没保存的也算）真的登录一次再挂断。第一次见到的主机密钥叠一个
    /// 确认框问；密钥变了只报错不问。结果写在按钮旁边。
    fn test_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.testing.is_some() {
            return;
        }
        let Some(draft) = self.validated_draft(cx) else {
            return;
        };
        let name = draft.name.clone();
        // 新建的主机还没有 id：密码条目随便哪个都行，测试只用表单里的密码。
        let host = Host::new(self.editing.unwrap_or(HostId(0)), draft);
        let login = self.store.read(cx).login_of(&host);
        let request = LoginTest::new(login, &self.auth.typed_password(cx));
        let (questions, asked) = async_channel::unbounded::<TrustQuestion>();
        let trust: TrustAsker = std::sync::Arc::new(move |prompt| {
            let (answer, answered) = futures::channel::oneshot::channel();
            let _ = questions.try_send((prompt, answer));
            async move { answered.await.unwrap_or(false) }.boxed()
        });
        let test = self.tester.test(request, trust);
        self.tested = None;
        // 问主机密钥：问题一来就叠一个确认框。测试结束、问的口子没了，这个循环也就结束。
        cx.spawn_in(window, async move |_, cx| {
            while let Ok((prompt, answer)) = asked.recv().await {
                let subtitle = name.clone();
                let _ = cx.update(|window, cx| {
                    open_prompt_dialog(
                        subtitle,
                        ConnectionPromptKind::UnknownHost(prompt),
                        move |reply, _, _| {
                            let _ =
                                answer.send(matches!(reply, ConnectionPromptReply::TrustAndSave));
                        },
                        window,
                        cx,
                    );
                });
            }
        })
        .detach();
        self.testing = Some(cx.spawn(async move |this, cx| {
            let result = test.await.map_err(SharedString::from);
            let _ = this.update(cx, |this, cx| {
                this.testing = None;
                this.tested = Some(result);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 校验并写进 store。返回对话框能不能关：出错时不关，出错的字段描红。
    pub fn commit(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(draft) = self.validated_draft(cx) else {
            return false;
        };
        let password = self.auth.password_change(cx);
        let editing = self.editing;
        self.store.update(cx, |store, cx| {
            let id = match editing {
                Some(id) => {
                    store.update(id, draft, cx);
                    id
                }
                None => store.insert(draft, cx),
            };
            if let Some(change) = password
                && let Some(secret) = store.host_password(id)
            {
                store.save_secret(secret, change, cx);
            }
        });
        true
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 要写密码而保险库还收不下（第一次保存、锁着、打不开）：先弹保险库的框，好了再保存，
        // 取消了对话框照旧开着（工单 13）。
        if self.auth.writes_password(cx)
            && crate::vault::asks_before_saving(cx)
            && self.validated_draft(cx).is_some()
        {
            let this = cx.entity().downgrade();
            crate::vault::before_saving_secret(window, cx, move |window, cx| {
                this.update(cx, |this, cx| this.save_now(window, cx)).ok();
            });
            return;
        }
        self.save_now(window, cx);
    }

    fn save_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.commit(cx) {
            window.close_dialog(cx);
        }
    }

    fn render_header(&self, tokens: &Tokens) -> impl IntoElement {
        let (title, subtitle) = match &self.editing_name {
            Some(name) => (t!("host.dialog.title_edit"), name.clone()),
            None => (t!("host.dialog.title_new"), t!("host.dialog.subtitle_new")),
        };
        let hover = tokens.hover;
        let text = tokens.text;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(46.))
            .pl(px(18.))
            .pr(px(8.))
            .child(
                div()
                    .id("host-dialog-title")
                    .test_support()
                    .aria_label(title.clone())
                    .flex_none()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .text_color(tokens.faint)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(subtitle),
            )
            .child(
                div()
                    .id("host-dialog-close")
                    .test_support()
                    .aria_label(t!("host.dialog.close"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.))
                    .rounded(px(10.))
                    .text_color(tokens.icon)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover).text_color(text))
                    .on_click(|_, window, cx| window.close_dialog(cx))
                    .child(Icon::new(IconName::Close).size(px(16.))),
            )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .pt(px(12.))
            .pb(px(14.))
            .pl(px(18.))
            .pr(px(14.))
            .child(
                Button::new("host-test-connection")
                    .small()
                    .label(if self.testing.is_some() {
                        t!("host.dialog.testing")
                    } else {
                        t!("host.dialog.test_connection")
                    })
                    .disabled(self.testing.is_some())
                    .on_click(cx.listener(|this, _, window, cx| this.test_connection(window, cx))),
            )
            .children(self.tested.as_ref().map(|tested| {
                let tokens = Tokens::current(cx);
                let (text, color) = match tested {
                    Ok(()) => (t!("host.dialog.test_ok"), tokens.busy),
                    Err(reason) => (t!("host.dialog.test_failed", reason = reason), tokens.bad),
                };
                div()
                    .id("host-test-result")
                    .test_support()
                    .aria_label(text.clone())
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(color)
                    .child(text)
            }))
            .child(div().flex_1())
            .child(
                Button::new("host-dialog-cancel")
                    .label(t!("host.dialog.cancel"))
                    .small()
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("host-dialog-save")
                    .primary()
                    .small()
                    .label(t!("host.dialog.save"))
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
    }
}

/// 主机草稿的错误落在哪个字段上。
fn field_of(error: HostDraftError) -> &'static str {
    match error {
        HostDraftError::Address => FIELD_ADDRESS,
        HostDraftError::Port => FIELD_PORT,
        HostDraftError::NoJumpHosts
        | HostDraftError::DeletedJumpHost
        | HostDraftError::ProxyAddress
        | HostDraftError::ProxyPort => ROUTE_CHOICE,
    }
}

impl Focusable for HostDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HostDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        div()
            .id("host-dialog")
            .test_support()
            .key_context(HOST_DIALOG_KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .size_full()
            .text_color(tokens.text)
            .child(self.render_header(&tokens))
            .child(
                div()
                    .id("host-dialog-body")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .px(px(18.))
                    .pt(px(12.))
                    .pb(px(10.))
                    .border_t_1()
                    .border_b_1()
                    .border_color(tokens.line)
                    .overflow_y_scrollbar()
                    .child(self.general.render(&self.errors, &tokens))
                    .child(self.auth.render(&self.errors, &tokens))
                    .child(self.route.render(&self.errors, &tokens))
                    .child(self.notes.render(&self.errors, &tokens)),
            )
            .child(self.render_footer(cx))
    }
}

/// 打开主机对话框：`editing` 是要编辑的主机，新建时是 `None`。对话框不点遮罩关闭，
/// 用右上角的按钮、「取消」或 Esc 关；焦点先给「名称」。
pub fn open_host_dialog(
    store: Entity<HostStore>,
    editing: Option<HostId>,
    tester: SharedConnectionTester,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| HostDialog::new(store, editing, tester, window, cx));
    let name = view.read(cx).general.name.clone();
    window.open_dialog(cx, move |dialog, window, _| {
        let height = MAX_HEIGHT.min(window.viewport_size().height - px(40.));
        dialog
            .w(WIDTH)
            .h(height)
            .p_0()
            .rounded(px(14.))
            .close_button(false)
            .overlay_closable(false)
            .child(view.clone())
    });
    name.update(cx, |name, cx| name.focus(window, cx));
}
