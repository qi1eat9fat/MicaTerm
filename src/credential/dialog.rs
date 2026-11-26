// 参考 since2006/shell-rs @ b2a307a: src/credential/credential_dialog.rs 重写
// 裁剪：私钥不再是文件路径，`keys/` 目录和「复制进 keys/」的说明去掉：本机文件只读一次，私钥正文
// 和口令一起加密存进保险库；版式改成全部界面原型的 `credDlg`（`.dlg.small`），校验出错时字段描红、
// 下面写原因，不弹通知；去掉「生成密钥…」入口。

//! 凭据对话框（原型「未设主密码 + 凭据对话框（生成新密钥）」「主机对话框上叠『新建凭据』」）：
//! 只有一组：名称、类型（密码 / 密钥 / SSH Agent）、用户名。
//!
//! - 密码：密码框（必填）；
//! - 密钥：私钥来源（本机文件 / 粘贴 / 生成新密钥；编辑已有的密钥时还有「当前私钥」），生成时选算法
//!   （Ed25519 / RSA 4096），直接显示公钥并能复制；最后是口令（可选）；
//! - SSH Agent：只有一句说明，什么秘密都不存。
//!
//! 私钥正文、口令、密码都按凭据的 `secret_id` 加密存进保险库；数据库里只存不算秘密的公钥。
//! 本机文件只在保存时读一次，之后和原文件再无关系。要写秘密而保险库还收不下（没建、锁着）时，
//! 先弹保险库的框（`vault::before_saving_secret`）。
//!
//! 叠在主机对话框上新建时（「使用凭据」下拉里的「新建凭据…」），保存后调 `on_saved`，主机对话框
//! 自动选上新凭据；Esc 只关最上面这一层。
//!
//! 样子是共用的 `ui::modal` 弹窗（26 号）：头部钥匙图标 + 「新建凭据」「编辑凭据」+ 淡字，字段照新建主机的
//! 写法（字段名在上、框在下，高 30），底部「取消 Esc」「保存 Enter」。单行框里回车就是保存。

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::*;
use zeroize::Zeroizing;

use crate::app::CatalogIcon;
use crate::host::{
    CredentialDraft, CredentialDraftError, CredentialId, CredentialKind, DEFAULT_USER,
    GeneratedKey, HostStore, KeyAlgorithm, PastedKey, PastedKeyError, read_small,
};
use crate::i18n::t;
use crate::settings::{OnChoose, segmented};
use crate::theme::space;
use crate::theme::{MONO_FONT_FAMILY, Tokens, TypeScale, size, ui_text};
use crate::ui::DialogLayer;
use crate::ui::form::{FieldErrors, FieldNote, form_field, form_group, locked_secret};
use crate::ui::modal::{
    ModalButtonTone, ModalHead, ModalInputKind, OnClick, modal_button, modal_dialog, modal_input,
    modal_textarea, render_modal,
};
use crate::vault::{SecretField, VaultStore, asks_before_saving, before_saving_secret};

pub const FIELD_NAME: &str = "credential-name";
pub const FIELD_USER: &str = "credential-user";
pub const FIELD_PASSWORD: &str = "credential-password";
pub const FIELD_FILE: &str = "credential-key-file";
pub const FIELD_KEY_TEXT: &str = "credential-key-text";
pub const FIELD_PASSPHRASE: &str = "credential-passphrase";
/// 公钥那一行（生成的、粘贴的、编辑时现在的），全文在无障碍名称里。
pub const PUBLIC_KEY: &str = "credential-public-key";
const KIND_CHOICE: &str = "credential-kind";
const SOURCE_CHOICE: &str = "credential-key-source";
const ALGORITHM_CHOICE: &str = "credential-key-algorithm";
/// 「生成新密钥」还在生成、生成失败、保存时加密失败：写在公钥那一行。
const KEY_PROBLEM: &str = "credential-key-problem";

/// 凭据表单保留底部留白；短窗口里正文仍有足够的滚动区域。
fn max_body_height(window: &Window) -> Pixels {
    crate::ui::modal::modal_body_max_height(window.viewport_size().height, px(48.), px(240.))
}

/// 保存了一条凭据：叠在主机对话框上新建时，主机对话框据此自动选上它。
pub type CredentialSaved = Box<dyn FnOnce(CredentialId, &mut Window, &mut App)>;

/// 密钥凭据的私钥从哪来。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeySource {
    /// 编辑密钥凭据时：保险库里已经存着的那把，不换。
    Kept,
    /// 本机的私钥文件，保存时读一次。
    File,
    /// 贴进来的私钥文本。
    Paste,
    /// 现场生成一对新的。
    Generate,
}

impl KeySource {
    fn label(self) -> SharedString {
        match self {
            KeySource::Kept => t!("credential.dialog.source_kept"),
            KeySource::File => t!("credential.dialog.source_file"),
            KeySource::Paste => t!("credential.dialog.source_paste"),
            KeySource::Generate => t!("credential.dialog.source_generate"),
        }
    }
}

/// 生成中、生成好的密钥。
enum Generated {
    Working,
    Ready(Arc<GeneratedKey>),
    Failed(SharedString),
}

/// 保存时要写进保险库的私钥：正文、口令和公钥。
struct KeyMaterial {
    text: Zeroizing<String>,
    /// `None`：没有口令（原来存的要删掉）。
    passphrase: Option<Zeroizing<String>>,
    public_key: Option<String>,
}

/// 凭据对话框的内容。每次打开新建一个。
pub struct CredentialDialog {
    store: Entity<HostStore>,
    /// 编辑的凭据；新建时是 `None`。
    editing: Option<CredentialId>,
    /// 编辑时标题后面的灰字：凭据原来的名字。
    editing_name: Option<SharedString>,
    /// 叠在主机对话框上新建：标题后面写「保存后自动选上」，保存后调 `on_saved`。
    on_saved: Option<CredentialSaved>,
    stacked: bool,
    kind: CredentialKind,
    name: Entity<InputState>,
    user: Entity<InputState>,
    password: SecretField,
    passphrase: SecretField,
    /// 可选的私钥来源，按分段按钮的顺序。
    sources: Vec<KeySource>,
    source: KeySource,
    /// 编辑的密钥凭据现在的公钥。
    kept_public_key: Option<SharedString>,
    file: Entity<InputState>,
    key_text: Entity<TextareaState>,
    /// 贴进来的文字读出来是什么，每次改动重读。
    pasted: Option<Result<PastedKey, PastedKeyError>>,
    algorithm: KeyAlgorithm,
    /// 为哪种算法生成的密钥。每次请求编一个号，先选的算法晚到的结果丢掉。
    generated: Option<(KeyAlgorithm, Generated)>,
    generation: u64,
    /// 有几台主机在用编辑的这条凭据。
    hosts: usize,
    errors: FieldErrors,
    /// 正在读私钥文件、加密私钥：保存按钮不能再点。
    saving: Option<Task<()>>,
    /// 对话框是哪一层：保存、取消时只关自己。
    layer: DialogLayer,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl CredentialDialog {
    pub fn new(
        store: Entity<HostStore>,
        editing: Option<CredentialId>,
        on_saved: Option<CredentialSaved>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (credential, hosts) = {
            let data = store.read(cx);
            let credential = editing.and_then(|id| data.credential(id)).cloned();
            let hosts = credential
                .as_ref()
                .map_or(0, |credential| data.hosts_using(credential.id).count());
            (credential, hosts)
        };
        let editing = credential.as_ref().map(|credential| credential.id);
        // 新建时照原型默认「密钥 · 生成新密钥」。
        let draft = credential
            .as_ref()
            .map(|credential| credential.draft())
            .unwrap_or_else(|| CredentialDraft::new("", CredentialKind::Key, ""));
        let kept = credential
            .as_ref()
            .is_some_and(|credential| credential.kind == CredentialKind::Key);
        let mut sources = vec![KeySource::File, KeySource::Paste, KeySource::Generate];
        if kept {
            sources.insert(0, KeySource::Kept);
        }
        let source = if kept {
            KeySource::Kept
        } else {
            KeySource::Generate
        };
        let input = |value: SharedString,
                     placeholder: SharedString,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let name = input(
            draft.name.clone(),
            t!("credential.dialog.name_placeholder"),
            window,
            cx,
        );
        let user = input(
            if editing.is_some() {
                draft.user.clone()
            } else {
                SharedString::default()
            },
            DEFAULT_USER.into(),
            window,
            cx,
        );
        let file = input(
            SharedString::default(),
            t!("credential.dialog.file_placeholder"),
            window,
            cx,
        );
        let key_text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 8)
                .placeholder(t!("credential.dialog.key_text_placeholder"))
        });
        // 编辑时原来存在哪：密码凭据的密码、密钥凭据的口令。
        let kind = credential.as_ref().map(|credential| credential.kind);
        let password = SecretField::new(
            credential
                .as_ref()
                .filter(|_| kind == Some(CredentialKind::Password))
                .map(|credential| credential.password_secret()),
            t!("credential.dialog.password_placeholder"),
            window,
            cx,
        );
        let passphrase = SecretField::new(
            credential
                .as_ref()
                .filter(|_| kind == Some(CredentialKind::Key))
                .map(|credential| credential.passphrase_secret()),
            t!("credential.dialog.passphrase_placeholder"),
            window,
            cx,
        );
        let mut subscriptions = vec![cx.subscribe(
            &key_text,
            |this: &mut Self, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value();
                    this.pasted = (!text.trim().is_empty()).then(|| PastedKey::parse(&text));
                    this.errors.clear(FIELD_KEY_TEXT);
                    cx.notify();
                }
            },
        )];
        // 改了哪个字段，就先收起它的错误；生成的公钥的注释跟着名称走。单行框里回车就是保存。
        for (field, state) in [
            (FIELD_NAME, name.clone()),
            (FIELD_USER, user.clone()),
            (FIELD_PASSWORD, password.input().clone()),
            (FIELD_FILE, file.clone()),
            (FIELD_PASSPHRASE, passphrase.input().clone()),
        ] {
            subscriptions.push(cx.subscribe_in(
                &state,
                window,
                move |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.errors.clear(field);
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.save(window, cx),
                    _ => {}
                },
            ));
        }
        if let Some(vault) = VaultStore::global(cx) {
            subscriptions.push(cx.observe_in(&vault, window, |this, _, window, cx| {
                this.vault_changed(window, cx);
                cx.notify();
            }));
        }
        let mut dialog = Self {
            store,
            editing,
            editing_name: credential
                .as_ref()
                .map(|credential| credential.name.clone()),
            stacked: on_saved.is_some(),
            on_saved,
            kind: draft.kind,
            name,
            user,
            password,
            passphrase,
            sources,
            source,
            kept_public_key: credential.and_then(|credential| credential.public_key),
            file,
            key_text,
            pasted: None,
            algorithm: KeyAlgorithm::default(),
            generated: None,
            generation: 0,
            hosts,
            errors: FieldErrors::default(),
            saving: None,
            layer: DialogLayer::new(),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        let read = HostStore::reader(&dialog.store);
        dialog
            .password
            .load_saved(&read, window, cx, |this| &mut this.password);
        dialog
            .passphrase
            .load_saved(&read, window, cx, |this| &mut this.passphrase);
        if dialog.kind == CredentialKind::Key && dialog.source == KeySource::Generate {
            dialog.generate(cx);
        }
        dialog
    }

    /// 出错的字段和原因，测试用。
    pub fn errors(&self) -> &FieldErrors {
        &self.errors
    }

    /// 保险库变了：存过的秘密锁着就显示「已保存，保险库锁定中」，解开了就读回来。
    fn vault_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let read = HostStore::reader(&self.store);
        self.password
            .vault_changed(&read, window, cx, |this| &mut this.password);
        self.passphrase
            .vault_changed(&read, window, cx, |this| &mut this.passphrase);
    }

    fn set_kind(&mut self, kind: CredentialKind, cx: &mut Context<Self>) {
        if self.kind == kind {
            return;
        }
        self.kind = kind;
        self.errors = FieldErrors::default();
        if kind == CredentialKind::Key && self.source == KeySource::Generate {
            self.generate(cx);
        }
        cx.notify();
    }

    fn set_source(&mut self, source: KeySource, window: &mut Window, cx: &mut Context<Self>) {
        if self.source == source {
            return;
        }
        self.source = source;
        self.errors = FieldErrors::default();
        // 口令属于一把私钥：换了私钥，原来框里的字就不算数了；回到「当前私钥」时再读回存的。
        self.passphrase.clear(window, cx);
        if source == KeySource::Kept {
            let read = HostStore::reader(&self.store);
            self.passphrase
                .load_saved(&read, window, cx, |this| &mut this.passphrase);
        } else {
            self.passphrase.forget_saved();
        }
        if source == KeySource::Generate {
            self.generate(cx);
        }
        cx.notify();
    }

    fn set_algorithm(&mut self, algorithm: KeyAlgorithm, cx: &mut Context<Self>) {
        if self.algorithm != algorithm {
            self.algorithm = algorithm;
            self.generate(cx);
            cx.notify();
        }
    }

    /// 为选中的算法生成一对密钥（已经有、正在生成就不再生成）。在后台：RSA 要一会儿。
    fn generate(&mut self, cx: &mut Context<Self>) {
        let algorithm = self.algorithm;
        if self
            .generated
            .as_ref()
            .is_some_and(|(made_for, generated)| {
                *made_for == algorithm && !matches!(generated, Generated::Failed(_))
            })
        {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.generated = Some((algorithm, Generated::Working));
        cx.spawn(async move |this, cx| {
            let made = cx
                .background_spawn(async move { GeneratedKey::generate(algorithm) })
                .await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                let generated = match made {
                    Ok(key) => Generated::Ready(Arc::new(key)),
                    Err(error) => Generated::Failed(t!(
                        "credential.dialog.generate_failed",
                        error = error.to_string()
                    )),
                };
                this.generated = Some((algorithm, generated));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 生成好的、选中的算法的那一把。
    fn ready_key(&self) -> Option<&Arc<GeneratedKey>> {
        match &self.generated {
            Some((algorithm, Generated::Ready(key))) if *algorithm == self.algorithm => Some(key),
            _ => None,
        }
    }

    /// 用系统的选择框挑一个私钥文件。
    fn choose_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.file.update(cx, |input, cx| {
                    input.set_value(path.to_string_lossy().into_owned(), window, cx)
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 校验，取好私钥（读文件、加密在后台），再写进存储。出错时字段描红，对话框不关。
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving.is_some() {
            return;
        }
        let mut errors = FieldErrors::default();
        let draft = CredentialDraft::new(
            self.name.read(cx).value(),
            self.kind,
            self.user.read(cx).value(),
        )
        .validated();
        let draft = match draft {
            Ok(draft) => Some(draft),
            Err(CredentialDraftError::Name) => {
                errors.set(FIELD_NAME, t!("credential.dialog.name_required"));
                None
            }
        };
        if self.kind == CredentialKind::Password
            && !self.password.is_locked()
            && self.password.typed(cx).is_empty()
        {
            errors.set(FIELD_PASSWORD, t!("credential.dialog.password_required"));
        }
        if self.kind == CredentialKind::Key {
            match self.source {
                KeySource::File if self.file.read(cx).value().trim().is_empty() => {
                    errors.set(FIELD_FILE, t!("credential.dialog.file_required"));
                }
                KeySource::Paste => {
                    let text = self.key_text.read(cx).value();
                    if let Err(error) = PastedKey::parse(&text) {
                        errors.set(FIELD_KEY_TEXT, pasted_key_error(error));
                    }
                }
                KeySource::Generate => match &self.generated {
                    Some((_, Generated::Failed(error))) => errors.set(KEY_PROBLEM, error.clone()),
                    _ if self.ready_key().is_none() => {
                        errors.set(KEY_PROBLEM, t!("credential.dialog.still_generating"))
                    }
                    _ => {}
                },
                KeySource::Kept | KeySource::File => {}
            }
        }
        let Some(draft) = draft.filter(|_| errors.is_empty()) else {
            self.errors = errors;
            cx.notify();
            return;
        };
        if self.kind != CredentialKind::Key || self.source == KeySource::Kept {
            self.finish(draft, None, window, cx);
            return;
        }
        // 读私钥文件、给生成的私钥加口令都会阻塞（口令派生密钥本来就慢），放到后台去。
        let passphrase = self.passphrase.typed(cx);
        let prepare = match self.source {
            KeySource::File => {
                let path = expand_home(self.file.read(cx).value().trim());
                Prepare::File(path)
            }
            KeySource::Paste => {
                Prepare::Text(Zeroizing::new(self.key_text.read(cx).value().to_string()))
            }
            KeySource::Generate => match self.ready_key() {
                Some(key) => Prepare::Generated(key.clone(), draft.name.to_string()),
                None => return,
            },
            KeySource::Kept => return,
        };
        let job = cx.background_spawn(async move { prepare.run(passphrase) });
        self.saving = Some(cx.spawn_in(window, async move |this, cx| {
            let prepared = job.await;
            this.update_in(cx, |this, window, cx| {
                this.saving = None;
                match prepared {
                    Ok(material) => this.finish(draft, Some(material), window, cx),
                    Err((field, message)) => {
                        this.errors.set(field, message);
                        cx.notify();
                    }
                }
            })
            .ok();
        }));
        cx.notify();
    }

    /// 要写秘密而保险库还收不下时先弹保险库的框，好了再写；取消了对话框照旧开着。
    fn finish(
        &mut self,
        draft: CredentialDraft,
        key: Option<KeyMaterial>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let writes_secret = key.is_some()
            || (self.kind == CredentialKind::Password
                && matches!(self.password.change(cx), Some(Some(_))))
            || (self.kind == CredentialKind::Key
                && matches!(self.passphrase.change(cx), Some(Some(_))));
        // 不用弹框就直接写：同步回调会重入正在更新的对话框。
        if writes_secret && asks_before_saving(cx) {
            let this = cx.entity().downgrade();
            before_saving_secret(window, cx, move |window, cx| {
                this.update(cx, |this, cx| this.write(draft, key, window, cx))
                    .ok();
            });
            return;
        }
        self.write(draft, key, window, cx);
    }

    /// 写进存储：凭据（连公钥），再把私钥、口令、密码写进保险库，然后关掉对话框。
    fn write(
        &mut self,
        draft: CredentialDraft,
        key: Option<KeyMaterial>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = draft.kind;
        let draft = match (&key, kind) {
            (Some(key), _) => match &key.public_key {
                Some(line) => draft.with_public_key(line.clone()),
                None => draft,
            },
            // 「当前私钥」不换：公钥也不变。
            (None, CredentialKind::Key) => match &self.kept_public_key {
                Some(line) => draft.with_public_key(line.clone()),
                None => draft,
            },
            (None, _) => draft,
        };
        let password = (kind == CredentialKind::Password)
            .then(|| self.password.change(cx))
            .flatten();
        let passphrase = match &key {
            Some(key) => Some(key.passphrase.clone()),
            None if kind == CredentialKind::Key => self.passphrase.change(cx),
            None => None,
        };
        let editing = self.editing;
        let id = self.store.update(cx, |store, cx| {
            let id = match editing {
                Some(id) => store.update_credential(id, draft, cx).then_some(id),
                None => Some(store.insert_credential(draft, cx)),
            }?;
            let credential = store.credential(id)?.clone();
            if let Some(change) = password {
                store.save_secret(credential.password_secret(), change, cx);
            }
            if let Some(key) = &key {
                store.save_secret(credential.private_key_secret(), Some(key.text.clone()), cx);
            }
            if let Some(change) = passphrase {
                store.save_secret(credential.passphrase_secret(), change, cx);
            }
            Some(id)
        });
        self.close(window, cx);
        if let (Some(id), Some(on_saved)) = (id, self.on_saved.take()) {
            on_saved(id, window, cx);
        }
    }

    /// 关掉凭据对话框（只关它自己那一层）。
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.forget_secrets(window, cx);
        self.layer.close(window, cx);
    }

    /// 对话框要关了：明文现在就放掉，不等实体回收。
    fn forget_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.key_text
            .update(cx, |text, cx| text.set_value("", window, cx));
        self.password.clear(window, cx);
        self.passphrase.clear(window, cx);
        self.pasted = None;
        self.generated = None;
    }

    /// 编辑的凭据有主机在用时的那句话。
    fn usage_note(&self) -> Option<SharedString> {
        (self.hosts > 0).then(|| {
            if self.hosts == 1 {
                t!("credential.dialog.usage_one", count = self.hosts)
            } else {
                t!("credential.dialog.usage", count = self.hosts)
            }
        })
    }

    /// 头部（26 号 `.cp-h`）：钥匙图标 + 「新建凭据」「编辑凭据」，淡字是编辑的那条的名字，
    /// 叠在主机对话框上新建时是「保存后自动选上」。
    fn head(&self) -> ModalHead {
        let (title, detail) = match (&self.editing_name, self.stacked) {
            (Some(name), _) => (t!("credential.dialog.title_edit"), Some(name.clone())),
            (None, true) => (
                t!("credential.dialog.title_new"),
                Some(t!("credential.dialog.subtitle_stacked")),
            ),
            (None, false) => (t!("credential.dialog.title_new"), None),
        };
        ModalHead {
            icon: CatalogIcon::KeyRound,
            title,
            detail,
        }
    }

    fn render_body(&self, tokens: &Tokens, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let kind = self.kind;
        let dialog = cx.entity().downgrade();
        let choose_kind: OnChoose = Rc::new(move |index, _, cx| {
            if let Some(kind) = CredentialKind::ALL.get(index).copied() {
                dialog
                    .update(cx, |dialog, cx| dialog.set_kind(kind, cx))
                    .ok();
            }
        });
        let mut rows = vec![
            form_field(
                Some(t!("credential.dialog.name")),
                modal_input(
                    FIELD_NAME,
                    t!("credential.dialog.name"),
                    &self.name,
                    ModalInputKind::Text,
                    self.errors.get(FIELD_NAME).is_some(),
                    tokens,
                    window,
                    cx,
                ),
                FieldNote::error_of(FIELD_NAME, &self.errors)
                    .into_iter()
                    .collect(),
                tokens,
            ),
            form_field(
                Some(t!("credential.dialog.kind")),
                segmented(
                    KIND_CHOICE,
                    CredentialKind::ALL
                        .iter()
                        .map(|kind| super::kind_label(*kind))
                        .collect(),
                    CredentialKind::ALL
                        .iter()
                        .position(|each| *each == kind)
                        .unwrap_or(0),
                    choose_kind,
                    tokens,
                    TypeScale::current(cx),
                ),
                Vec::new(),
                tokens,
            ),
            form_field(
                Some(t!("credential.dialog.user")),
                modal_input(
                    FIELD_USER,
                    t!("credential.dialog.user"),
                    &self.user,
                    ModalInputKind::Text,
                    false,
                    tokens,
                    window,
                    cx,
                ),
                Vec::new(),
                tokens,
            ),
        ];
        match kind {
            CredentialKind::Password => rows.push(self.render_secret(
                Secret::Password,
                t!("credential.dialog.password"),
                tokens,
                window,
                cx,
            )),
            CredentialKind::Key => {
                rows.extend(self.render_key(tokens, window, cx));
                rows.push(self.render_secret(
                    Secret::Passphrase,
                    t!("credential.dialog.passphrase"),
                    tokens,
                    window,
                    cx,
                ));
            }
            CredentialKind::Agent => {
                let hint = if cfg!(windows) {
                    t!("credential.dialog.agent_hint_windows")
                } else {
                    t!("credential.dialog.agent_hint")
                };
                rows.push(form_field(
                    None,
                    note("credential-agent-hint", hint, tokens),
                    Vec::new(),
                    tokens,
                ));
            }
        }
        if let Some(usage) = self.usage_note() {
            rows.push(form_field(
                None,
                note("credential-usage", usage, tokens),
                Vec::new(),
                tokens,
            ));
        }
        form_group(rows)
    }

    /// 密码框、口令框：锁着又存过时换成「已保存，保险库锁定中 · 解锁…」。
    fn render_secret(
        &self,
        which: Secret,
        label: SharedString,
        tokens: &Tokens,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let (field, id) = match which {
            Secret::Password => (&self.password, FIELD_PASSWORD),
            Secret::Passphrase => (&self.passphrase, FIELD_PASSPHRASE),
        };
        let control = if field.is_locked() {
            locked_secret(
                which.locked_id(),
                "credential-secret-unlock",
                tokens,
                TypeScale::current(cx),
            ).into_any_element()
        } else {
            modal_input(
                id,
                label.clone(),
                field.input(),
                ModalInputKind::Secret,
                self.errors.get(id).is_some(),
                tokens,
                window,
                cx,
            )
            .into_any_element()
        };
        form_field(
            Some(label),
            control,
            FieldNote::error_of(id, &self.errors).into_iter().collect(),
            tokens,
        )
    }

    /// 密钥的几行：私钥来源，本机文件 / 粘贴的文字 / 生成的算法，公钥。
    fn render_key(
        &self,
        tokens: &Tokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let source = self.source;
        let dialog = cx.entity().downgrade();
        let sources = self.sources.clone();
        let choose_source: OnChoose = Rc::new(move |index, window, cx| {
            if let Some(source) = sources.get(index).copied() {
                dialog
                    .update(cx, |dialog, cx| dialog.set_source(source, window, cx))
                    .ok();
            }
        });
        let mut rows = vec![form_field(
            Some(t!("credential.dialog.source")),
            segmented(
                SOURCE_CHOICE,
                self.sources.iter().map(|source| source.label()).collect(),
                self.sources
                    .iter()
                    .position(|each| *each == source)
                    .unwrap_or(0),
                choose_source,
                tokens,
                TypeScale::current(cx),
            ),
            Vec::new(),
            tokens,
        )];
        match source {
            KeySource::Kept => {
                if let Some(line) = &self.kept_public_key {
                    rows.push(self.render_public_key(line.clone(), false, tokens, window, cx));
                }
                rows.push(form_field(
                    None,
                    note(
                        "credential-kept-hint",
                        t!("credential.dialog.kept_hint"),
                        tokens,
                    ),
                    Vec::new(),
                    tokens,
                ));
            }
            KeySource::File => {
                let mut notes: Vec<FieldNote> = FieldNote::error_of(FIELD_FILE, &self.errors)
                    .into_iter()
                    .collect();
                notes.push(FieldNote::Hint(t!("credential.dialog.file_hint")));
                rows.push(form_field(
                    Some(t!("credential.dialog.file")),
                    div()
                        .flex()
                        .items_center()
                        .gap(space::SPACE_8)
                        .child(div().flex_1().min_w_0().child(modal_input(
                            FIELD_FILE,
                            t!("credential.dialog.file"),
                            &self.file,
                            ModalInputKind::Text,
                            self.errors.get(FIELD_FILE).is_some(),
                            tokens,
                            window,
                            cx,
                        )))
                        .child(modal_button(
                            "credential-key-choose",
                            t!("credential.dialog.choose_file"),
                            None,
                            ModalButtonTone::Line,
                            Some(Box::new(cx.listener(|this, _, window, cx| {
                                this.choose_file(window, cx)
                            }))),
                            tokens,
                            window,
                            cx,
                        )),
                    notes,
                    tokens,
                ));
            }
            KeySource::Paste => {
                rows.push(form_field(
                    Some(t!("credential.dialog.key_text")),
                    modal_textarea(
                        FIELD_KEY_TEXT,
                        t!("credential.dialog.key_text"),
                        &self.key_text,
                        self.errors.get(FIELD_KEY_TEXT).is_some(),
                        tokens,
                        window,
                        cx,
                    ),
                    FieldNote::error_of(FIELD_KEY_TEXT, &self.errors)
                        .into_iter()
                        .collect(),
                    tokens,
                ));
                if let Some(Ok(pasted)) = &self.pasted
                    && let Some(line) = pasted.public_key_line()
                {
                    rows.push(self.render_public_key(line.into(), false, tokens, window, cx));
                }
            }
            KeySource::Generate => {
                let dialog = cx.entity().downgrade();
                let choose_algorithm: OnChoose = Rc::new(move |index, _, cx| {
                    if let Some(algorithm) = KeyAlgorithm::ALL.get(index).copied() {
                        dialog
                            .update(cx, |dialog, cx| dialog.set_algorithm(algorithm, cx))
                            .ok();
                    }
                });
                rows.push(form_field(
                    Some(t!("credential.dialog.algorithm")),
                    segmented(
                        ALGORITHM_CHOICE,
                        KeyAlgorithm::ALL
                            .iter()
                            .map(|algorithm| algorithm.label().into())
                            .collect(),
                        KeyAlgorithm::ALL
                            .iter()
                            .position(|each| *each == self.algorithm)
                            .unwrap_or(0),
                        choose_algorithm,
                        tokens,
                        TypeScale::current(cx),
                    ),
                    Vec::new(),
                    tokens,
                ));
                let row = match (&self.generated, self.ready_key()) {
                    (_, Some(key)) => {
                        let line = key.public_key_line(&self.name.read(cx).value());
                        self.render_public_key(line.into(), true, tokens, window, cx)
                    }
                    (Some((_, Generated::Failed(error))), _) => form_field(
                        Some(t!("credential.dialog.public_key")),
                        note(KEY_PROBLEM, error.clone(), tokens),
                        Vec::new(),
                        tokens,
                    ),
                    _ => form_field(
                        Some(t!("credential.dialog.public_key")),
                        note(
                            "credential-key-generating",
                            t!("credential.dialog.generating"),
                            tokens,
                        ),
                        Vec::new(),
                        tokens,
                    ),
                };
                rows.push(row);
            }
        }
        if let Some(problem) = self.errors.get(KEY_PROBLEM) {
            rows.push(form_field(
                None,
                div(),
                vec![FieldNote::Error {
                    field: KEY_PROBLEM,
                    message: problem.clone(),
                }],
                tokens,
            ));
        }
        rows
    }

    /// 公钥一行（样子同弹窗里的指纹框 `.cp-fp`：内容底、细边、圆角 6，等宽小字），
    /// 下面是「复制公钥」和「贴进服务器的 authorized_keys」。
    fn render_public_key(
        &self,
        line: SharedString,
        hint: bool,
        tokens: &Tokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let copied = line.clone();
        form_field(
            Some(t!("credential.dialog.public_key")),
            div()
                .flex()
                .flex_col()
                .gap(space::SPACE_6)
                .child(
                    div()
                        .id(PUBLIC_KEY)
                        .test_support()
                        .aria_label(line.clone())
                        .px(space::SPACE_10)
                        .py(space::SPACE_7)
                        .rounded(size::RADIUS_MD)
                        .bg(tokens.content)
                        .border_1()
                        .border_color(tokens.line)
                        .text_color(tokens.text2)
                        .text_size(TypeScale::current(cx).sm)
                        .line_height(relative(1.5))
                        .font_family(MONO_FONT_FAMILY)
                        .child(line),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::SPACE_8)
                        .child(modal_button(
                            "credential-copy-public-key",
                            t!("credential.dialog.copy_public_key"),
                            None,
                            ModalButtonTone::Line,
                            Some(Box::new(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()))
                            })),
                            tokens,
                            window,
                            cx,
                        ))
                        .children(hint.then(|| {
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(tokens.faint)
                                .child(t!("credential.dialog.public_key_hint"))
                        })),
                ),
            Vec::new(),
            tokens,
        )
    }

    /// 底部（26 号 `.cp-f`）：「取消 Esc」「保存 Enter」；读私钥、加密时保存按钮写「正在保存…」、点不了。
    fn render_footer(&self, tokens: &Tokens, window: &Window, cx: &mut Context<Self>) -> Div {
        let saving = self.saving.is_some();
        div()
            .flex()
            .gap(space::SPACE_6)
            .child(modal_button(
                "credential-dialog-cancel",
                t!("credential.dialog.cancel"),
                Some("Esc"),
                ModalButtonTone::Plain,
                Some(Box::new(
                    cx.listener(|this, _, window, cx| this.close(window, cx)),
                )),
                tokens,
                window,
                cx,
            ))
            .child(modal_button(
                "credential-dialog-save",
                if saving {
                    t!("credential.dialog.saving")
                } else {
                    t!("credential.dialog.save")
                },
                Some("Enter"),
                ModalButtonTone::Primary,
                (!saving).then(|| -> OnClick {
                    Box::new(cx.listener(|this, _, window, cx| this.save(window, cx)))
                }),
                tokens,
                window,
                cx,
            ))
    }
}

/// 两个秘密框中的哪一个。
#[derive(Clone, Copy)]
enum Secret {
    Password,
    Passphrase,
}

impl Secret {
    fn locked_id(self) -> &'static str {
        match self {
            Secret::Password => "credential-password-locked",
            Secret::Passphrase => "credential-passphrase-locked",
        }
    }
}

/// 保存时在后台取私钥：读文件、读贴进来的文字、给生成的私钥加口令。
enum Prepare {
    File(PathBuf),
    Text(Zeroizing<String>),
    Generated(Arc<GeneratedKey>, String),
}

impl Prepare {
    /// 阻塞：读文件，派生口令的密钥。出错时是出错的字段和原因。
    fn run(
        self,
        passphrase: Zeroizing<String>,
    ) -> Result<KeyMaterial, (&'static str, SharedString)> {
        let passphrase = (!passphrase.is_empty()).then_some(passphrase);
        let (field, text) = match self {
            Prepare::Generated(key, comment) => {
                let text = key
                    .encode(&comment, passphrase.as_deref().map_or("", |p| p.as_str()))
                    .map_err(|error| {
                        (
                            KEY_PROBLEM,
                            t!("credential.dialog.encode_failed", error = error.to_string()),
                        )
                    })?;
                return Ok(KeyMaterial {
                    text,
                    passphrase,
                    public_key: Some(key.public_key_line(&comment)),
                });
            }
            Prepare::File(path) => (
                FIELD_FILE,
                read_small(&path)
                    .ok_or_else(|| (FIELD_FILE, t!("credential.dialog.file_unreadable")))?,
            ),
            Prepare::Text(text) => (FIELD_KEY_TEXT, text),
        };
        let pasted = PastedKey::parse(&text).map_err(|error| (field, pasted_key_error(error)))?;
        Ok(KeyMaterial {
            text: Zeroizing::new(pasted.text().to_string()),
            // 没加密的私钥没有口令可存。
            passphrase: passphrase.filter(|_| pasted.is_encrypted()),
            public_key: pasted.public_key_line(),
        })
    }
}

/// 贴进来的（或文件里的）不是私钥时说什么。
fn pasted_key_error(error: PastedKeyError) -> SharedString {
    match error {
        PastedKeyError::Empty => t!("credential.dialog.key_empty"),
        PastedKeyError::PublicKey => t!("credential.dialog.key_is_public"),
        PastedKeyError::Unreadable => t!("credential.dialog.key_unreadable"),
    }
}

/// `~/.ssh/id_ed25519` 里的 `~` 换成用户目录。
fn expand_home(path: &str) -> PathBuf {
    let rest = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\"));
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    match (rest, home) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// 字段位置上的一句灰字。
fn note(id: &'static str, text: SharedString, tokens: &Tokens) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .aria_label(text.clone())
        .text_size(ui_text())
        .line_height(relative(1.5))
        .text_color(tokens.st_dim)
        .child(text)
        .into_any_element()
}

impl Focusable for CredentialDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CredentialDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        // 字段多（生成密钥时有七行）：正文再高就在里面滚动，头部和底部不动。
        let body = div()
            .id("credential-dialog-body")
            .flex()
            .flex_col()
            .max_h(max_body_height(window))
            .overflow_y_scroll()
            .child(self.render_body(&tokens, window, cx))
            .test_support();
        let footer = self.render_footer(&tokens, window, cx);
        render_modal(Some(self.head()), body, footer, &tokens, window, cx)
            .id("credential-dialog")
            .test_support()
            .track_focus(&self.focus_handle)
    }
}

/// 打开新建（`editing` 为 `None`）或编辑凭据的对话框。`on_saved`：叠在主机对话框上新建时，
/// 保存后要做的事（选上新凭据）。对话框不点遮罩关闭，用「取消」或 Esc 关。
pub fn open_credential_dialog(
    store: Entity<HostStore>,
    editing: Option<CredentialId>,
    on_saved: Option<CredentialSaved>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| CredentialDialog::new(store, editing, on_saved, window, cx));
    let name = view.read(cx).name.clone();
    let new = view.read(cx).editing.is_none();
    let layer = view.read(cx).layer.clone();
    let dismissed = view.downgrade();
    layer.open_dismissable(
        move |dialog, _, cx| {
            modal_dialog(dialog, &Tokens::current(cx))
                // 回车由单行框自己处理（保存，读私钥在后台）：gpui-component 的 `Confirm` 不关框。
                .on_ok(|_, _, _| false)
                .child(view.clone())
        },
        // Esc 关掉的：明文一样现在就放掉。
        move |window, cx| {
            dismissed
                .update(cx, |dialog, cx| dialog.forget_secrets(window, cx))
                .ok();
        },
        window,
        cx,
    );
    // 打开就能打字：要在打开的同一次更新里给焦点，否则对话框自己的焦点会抢过去。
    if new {
        name.update(cx, |name, cx| name.focus(window, cx));
    }
}
