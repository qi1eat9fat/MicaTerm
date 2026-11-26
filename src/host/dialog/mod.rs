// 参考 since2006/shell-rs @ b2a307a: src/host/host_dialog.rs 重写
// 裁剪：去掉分组、协议（Telnet、串口）、临时连接；版式改成 spec 的变体 B（宽约 500、圆角 14、
// 单页分组可以滚动）；校验出错不再弹通知，而是字段描红、下面写原因。

//! 主机表单：新建和编辑主机共用，照空壳的「添加主机」做成标签页里居中的卡片（背后压暗）。
//! 宽 410、内边距 19、圆角 13：标题 + 说明，名称，地址 + 端口；认证方式（切换按钮）、用户名 + 密码；
//! 连接方式；备注（一行）；按钮在右下角：左边「测试连接」（描边），右边「取消 Esc」「保存 Enter」。
//! 保存、取消后发 [`HostFormEvent::Close`]，窗口壳关掉这个标签。
//!
//! 每组一个文件、一个结构，`commit` 依次调它们的 `fill` 把值写进 `HostDraft`、记下出错的字段：
//! - `general`：名称（留空用地址）、地址、端口；
//! - `auth`：认证：密码 / 使用凭据 / 无密码（13 改密码框的锁定显示，14 加「使用凭据」「无密码」）；
//! - `route`：连接方式：直接 / SSH 跳板 / 代理（工单 15），代理密码和主机密码一样单独写进保险库；
//! - `notes`：备注；
//! - `advanced`：高级：「登录后探测系统」开关（工单 34）。
//!
//! 校验：出错的字段描红，下面写原因，保存失败时标签不关。
//!
//! 「测试连接」（工单 12）用表单里现在的值真的登录一次（`ConnectionTester`），第一次见到的主机密钥
//! 叠一个确认框问；结果写在按钮旁边。

mod advanced;
mod auth;
mod delete;
mod general;
mod notes;
mod route;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use futures::FutureExt as _;
use micaterm_hosts::{AuthKind, Host, HostDraft, HostDraftError, HostId, SSH_PORT};

pub use advanced::FIELD_PROBE_OS;
pub use auth::{
    AUTH_CHOICE, AUTH_HINT, CREDENTIAL_NEW, CREDENTIAL_OPTION, FIELD_CREDENTIAL, FIELD_PASSWORD,
    FIELD_USER,
};
pub use delete::open_delete_host_dialog;
pub use general::{FIELD_ADDRESS, FIELD_NAME, FIELD_PORT};
pub use notes::FIELD_NOTES;
pub use route::{
    ADD_JUMP, FIELD_JUMPS, FIELD_PROXY_HOST, FIELD_PROXY_PASSWORD, FIELD_PROXY_PORT,
    FIELD_PROXY_USER, JUMP_CANDIDATE, JUMP_DOWN, JUMP_REMOVE, JUMP_ROW, JUMP_UP, PROXY_KIND,
    ROUTE_CHAIN, ROUTE_CHOICE,
};

use self::advanced::AdvancedSection;
use self::auth::AuthSection;
use self::general::GeneralSection;
use self::notes::NotesSection;
use self::route::RouteSection;
use super::describe::ErrorMessage as _;
use super::store::HostStore;
use crate::app::CatalogIcon;
use crate::backend::{
    ConnectionPromptKind, ConnectionPromptReply, LoginTest, SharedConnectionTester, TrustAsker,
    UnknownHostPrompt,
};
use crate::connect::open_prompt_dialog;
use crate::i18n::t;
use crate::theme::space;
use crate::theme::{Tokens, TypeScale, size};
use crate::ui::DialogLayer;
use crate::ui::form::FieldErrors;
use crate::ui::modal::{
    ModalButtonTone, ModalHead, OnClick, modal_button, modal_dialog_wide, render_modal,
};

/// 对话框的键上下文。
pub const HOST_DIALOG_KEY_CONTEXT: &str = "HostDialog";

/// 主机对话框的内容。每次打开新建一个，经 [`open_host_dialog`] 在窗口根层居中弹出。
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
    advanced: AdvancedSection,
    errors: FieldErrors,
    /// 「测试连接」用的测试器（工单 12）。
    tester: SharedConnectionTester,
    /// 正在测试；测试时按钮不能再点。
    testing: Option<Task<()>>,
    /// 上一次测试的结果：成功，或者给用户看的原因。
    tested: Option<Result<(), SharedString>>,
    /// 用户是否改过表单里的字（没改过且没在测试时允许点遮罩关闭）。
    dirty: bool,
    layer: DialogLayer,
    previous_focus: Option<FocusHandle>,
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
        layer: DialogLayer,
        previous_focus: Option<FocusHandle>,
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
        let route = RouteSection::new(&draft, editing, &store, window, cx);
        let notes = NotesSection::new(&draft, window, cx);
        let advanced = AdvancedSection::new(&draft);
        // 改了哪个字段，就先收起它的错误，并标记为已改动。
        let subscriptions = general
            .inputs()
            .into_iter()
            .chain(auth.inputs())
            .chain(route.inputs())
            .map(|(field, input)| {
                cx.subscribe(
                    &input,
                    move |this, _: Entity<InputState>, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.dirty = true;
                            this.errors.clear(field);
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
            advanced,
            errors: FieldErrors::default(),
            tester,
            testing: None,
            tested: None,
            dirty: false,
            layer,
            previous_focus,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// 编辑的是哪台主机；新建时是 `None`。
    pub fn editing(&self) -> Option<HostId> {
        self.editing
    }

    /// 标签上的名字：「新建主机」，编辑时是「编辑 web-01」这样。
    pub fn tab_name(&self) -> SharedString {
        match &self.editing_name {
            Some(name) => t!("host.dialog.title_edit", name = name),
            None => t!("host.dialog.title_new"),
        }
    }

    /// 标签到前面来时焦点给「名称」。
    pub fn focus_target(&self, cx: &App) -> FocusHandle {
        self.general.name.read(cx).focus_handle(cx)
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
        self.advanced.fill(&mut draft, &mut errors);
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
        let request = LoginTest::new(login, &self.auth.typed_password(cx))
            .with_proxy_password(&self.route.typed_proxy_password(cx));
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
                let head = ModalHead {
                    icon: CatalogIcon::Server,
                    title: name.clone(),
                    detail: Some(format!("{}:{}", prompt.host(), prompt.port()).into()),
                };
                let _ = cx.update(|window, cx| {
                    open_prompt_dialog(
                        head,
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
        let proxy_password = self.route.proxy_password_change(&draft.route, cx);
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
            if let Some((secret, change)) = proxy_password {
                store.save_secret(secret, change, cx);
            }
        });
        true
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 要写密码（主机的、代理的）而保险库还收不下（第一次保存、锁着、打不开）：先弹保险库的框，
        // 好了再保存，取消了对话框照旧开着（工单 13）。
        if crate::vault::asks_before_saving(cx) && self.writes_secret(cx) {
            let this = cx.entity().downgrade();
            crate::vault::before_saving_secret(window, cx, move |window, cx| {
                this.update(cx, |this, cx| this.save_now(window, cx)).ok();
            });
            return;
        }
        self.save_now(window, cx);
    }

    /// 表单校验得过、保存时要往保险库里写秘密（主机密码或代理密码）。
    fn writes_secret(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(draft) = self.validated_draft(cx) else {
            return false;
        };
        self.auth.writes_password(cx)
            || matches!(
                self.route.proxy_password_change(&draft.route, cx),
                Some((_, Some(_)))
            )
    }

    /// 「使用凭据」下拉里的「新建凭据…」：在上面叠一层凭据对话框，保存后自动选上新凭据
    /// （工单 14）。Esc 只关上面那一层。
    fn new_credential(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let on_saved: crate::credential::CredentialSaved = Box::new(move |id, _, cx| {
            this.update(cx, |dialog, cx| {
                dialog.auth.choose_credential(id, &mut dialog.errors);
                cx.notify();
            })
            .ok();
        });
        crate::credential::open_credential_dialog(
            self.store.clone(),
            None,
            Some(on_saved),
            window,
            cx,
        );
    }

    fn save_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.commit(cx) {
            self.close(window, cx);
        }
    }

    /// 关闭弹窗：立即清理密码明文并归还焦点。
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.forget_secrets(window, cx);
        let focus = self.previous_focus.clone();
        self.layer.close(window, cx);
        if let Some(focus) = focus {
            window.focus(&focus, cx);
        }
    }

    /// 表单要关了：密码框里的明文现在就放掉，不等实体回收。
    pub fn forget_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.auth.forget_secrets(window, cx);
        self.route.forget_secrets(window, cx);
    }

    fn head(&self) -> ModalHead {
        ModalHead {
            icon: CatalogIcon::Server,
            title: self.tab_name(),
            detail: None,
        }
    }

    fn render_actions(&self, tokens: &Tokens, window: &Window, cx: &mut Context<Self>) -> Div {
        let testing = self.testing.is_some();
        let button = |id: &'static str,
                      label: SharedString,
                      keys: Option<&'static str>,
                      tone: ModalButtonTone,
                      on_click: Option<OnClick>,
                      cx: &App| {
            modal_button(id, label, keys, tone, on_click, tokens, window, cx)
        };
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap(space::SPACE_8)
            .child(button(
                "host-test-connection",
                if testing {
                    t!("host.dialog.testing")
                } else {
                    t!("host.dialog.test_connection")
                },
                None,
                ModalButtonTone::Line,
                (!testing).then(|| -> OnClick {
                    Box::new(cx.listener(|this, _, window, cx| this.test_connection(window, cx)))
                }),
                cx,
            ))
            .children(self.tested.as_ref().map(|tested| {
                let (text, color) = match tested {
                    Ok(()) => (t!("host.dialog.test_ok"), tokens.ok_text),
                    Err(reason) => (t!("host.dialog.test_failed", reason = reason), tokens.err),
                };
                div()
                    .id("host-test-result")
                    .test_support()
                    .aria_label(text.clone())
                    .min_w_0()
                    .truncate()
                    .text_color(color)
                    .child(text)
            }))
            .child(div().flex_1())
            .child(button(
                "host-dialog-cancel",
                t!("host.dialog.cancel"),
                Some("Esc"),
                ModalButtonTone::Plain,
                Some(Box::new(
                    cx.listener(|this, _, window, cx| this.close(window, cx)),
                )),
                cx,
            ))
            .child(button(
                "host-dialog-save",
                t!("host.dialog.save"),
                Some("Enter"),
                ModalButtonTone::Primary,
                Some(Box::new(
                    cx.listener(|this, _, window, cx| this.save(window, cx)),
                )),
                cx,
            ))
    }
}

/// 主机草稿的错误落在哪个字段上。
fn field_of(error: HostDraftError) -> &'static str {
    match error {
        HostDraftError::Address => FIELD_ADDRESS,
        HostDraftError::Port => FIELD_PORT,
        HostDraftError::NoJumpHosts | HostDraftError::DeletedJumpHost => FIELD_JUMPS,
        HostDraftError::ProxyAddress => FIELD_PROXY_HOST,
        HostDraftError::ProxyPort => FIELD_PROXY_PORT,
    }
}

/// 弹窗正文的最大高度：窗口高度去掉顶部偏移、头部和底部，保留滚动余量。
fn max_body_height(window: &Window) -> Pixels {
    let available = window.viewport_size().height - size::MODAL_TOP - px(160.);
    available.max(px(240.))
}

impl Focusable for HostDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HostDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let body = div()
            .id("host-dialog-body")
            .test_support()
            .flex()
            .flex_col()
            .gap(space::SPACE_12)
            .max_h(max_body_height(window))
            // 用原生滚动：`overflow_y_scrollbar` 会把这里的 id 换成自己的，测试按
            // `host-dialog-body` 滚到看不见的字段就找不到了。
            .overflow_y_scroll()
            .child(
                div()
                    .text_size(scale.body)
                    .text_color(tokens.text2)
                    .child(t!("host.dialog.subtitle")),
            )
            .child(self.general.render(&self.errors, &tokens, window, cx))
            .child(self.auth.render(&self.errors, &tokens, window, cx))
            .child(self.route.render(
                &self.errors,
                &tokens,
                self.general.shown_name(cx),
                window,
                cx,
            ))
            .child(self.notes.render(&self.errors, &tokens, window, cx))
            .child(self.advanced.render(&tokens, cx));
        let footer = self.render_actions(&tokens, window, cx);
        render_modal(Some(self.head()), body, footer, &tokens, window, cx)
            .id("host-dialog")
            .test_support()
            .key_context(HOST_DIALOG_KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "enter" => {
                        cx.stop_propagation();
                        this.save(window, cx);
                    }
                    "escape" => {
                        cx.stop_propagation();
                        if this.auth.picking || this.route.picking {
                            this.auth.picking = false;
                            this.route.picking = false;
                            cx.notify();
                        } else {
                            this.close(window, cx);
                        }
                    }
                    _ => {}
                }
            }))
    }
}

/// 打开新建（`editing` 为 `None`）或编辑主机的模态弹窗。
pub fn open_host_dialog(
    store: Entity<HostStore>,
    editing: Option<HostId>,
    tester: SharedConnectionTester,
    window: &mut Window,
    cx: &mut App,
) {
    let layer = DialogLayer::new();
    let previous_focus = window.focused(cx);
    let view = cx.new(|cx| {
        HostDialog::new(
            store,
            editing,
            tester,
            layer.clone(),
            previous_focus.clone(),
            window,
            cx,
        )
    });
    let name = view.read(cx).general.name.clone();
    let dismissed = view.downgrade();
    let restore = previous_focus;
    layer.open_dismissable(
        move |dialog, _, cx| {
            let state = view.read(cx);
            let closable = !state.dirty && state.testing.is_none();
            modal_dialog_wide(dialog, &Tokens::current(cx))
                .overlay_closable(closable)
                .keyboard(false)
                .on_ok(|_, _, _| false)
                .child(view.clone())
        },
        move |window, cx| {
            dismissed
                .update(cx, |dialog, cx| dialog.forget_secrets(window, cx))
                .ok();
            if let Some(focus) = &restore {
                window.focus(focus, cx);
            }
        },
        window,
        cx,
    );
    name.update(cx, |name, cx| name.focus(window, cx));
}
