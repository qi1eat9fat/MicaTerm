//! 外部 CLI 要改的主机和凭据（Agent CLI 工单 04）：照主机、凭据表单那样核对，在 UI 线程上算出
//! 要存的草稿和要写的秘密；真正写进主机数据和保险库的是窗口壳（`workspace/cli.rs`）。
//! 这里只读主机数据，不改它。
// 参考 since2006/shell-rs @ b2a307a: src/cli/manage.rs 重写：没有分组、协议（Telnet、串口）、临时主机；
// 主机自己的密码按主机 id 存，改地址不用搬；凭据的私钥进保险库，`key_path` 只在这里读一次。

use micaterm_hosts::{
    AuthKind, Credential, CredentialDraft, CredentialDraftError, CredentialId, CredentialKind,
    HostData, HostDraft, HostId, PastedKey, PastedKeyError, ProxyKind, ProxySettings, Route,
    SSH_PORT, SecretRef, read_small,
};
use zeroize::Zeroizing;

use super::protocol::{
    AuthChoice, CliError, CredentialDetails, CredentialFields, CredentialKindChoice, ErrorCode,
    HostFields, ProxyChoice, RouteFields, Secret,
};
use crate::host::ErrorMessage as _;
use crate::i18n::t;

/// CLI 要做的一次改动，在 UI 线程上一个一个做。主机和凭据按 CLI 打印的 ID 指。
#[derive(Debug)]
pub enum CliChange {
    CreateHost(HostFields),
    UpdateHost {
        host: String,
        fields: HostFields,
    },
    DeleteHost {
        host: String,
    },
    CreateCredential(CredentialFields),
    UpdateCredential {
        credential: String,
        fields: CredentialFields,
    },
    DeleteCredential {
        credential: String,
    },
    /// `cards read`（工单 06）：只读，但要在 UI 线程上找卡片。
    ReadCard {
        card: String,
        lines: Option<usize>,
    },
    /// `cards send-key`。
    SendKey {
        card: String,
        key: String,
        repeat: Option<usize>,
    },
}

/// 保险库里一条秘密要怎么样。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretChange {
    Keep,
    Set(Secret),
    Delete,
    /// 从主机原来用的那一条搬过来（代理换了地址或用户），像主机表单那样；原来那条没人用了
    /// 主机数据会把它删掉。
    Carry(SecretRef),
}

impl SecretChange {
    fn from_field(field: Option<Option<Secret>>) -> Self {
        match field {
            None => SecretChange::Keep,
            Some(None) => SecretChange::Delete,
            Some(Some(secret)) => SecretChange::Set(secret),
        }
    }

    /// 从 `before` 换到 `after` 的一条秘密：字段没给就把原来的带过来。
    fn moving(field: Option<Option<Secret>>, before: Option<SecretRef>, after: &SecretRef) -> Self {
        match (field, before) {
            (None, Some(before)) if before != *after => SecretChange::Carry(before),
            (field, _) => Self::from_field(field),
        }
    }

    /// 要不要往保险库里写东西（删除不算：锁着的保险库删不掉的，主机数据会报告）。
    pub fn writes(&self) -> bool {
        matches!(self, SecretChange::Set(_) | SecretChange::Carry(_))
    }
}

/// 核对过的主机改动：草稿，和它的密码、代理密码怎么办。
#[derive(Debug)]
pub struct HostPlan {
    pub draft: HostDraft,
    /// 主机自己的登录密码（按主机 id 存）；不是自己用密码登录时是 `Keep`。
    pub password: SecretChange,
    pub proxy_password: Option<(SecretRef, SecretChange)>,
}

impl HostPlan {
    pub fn writes_secret(&self) -> bool {
        self.password.writes()
            || self
                .proxy_password
                .as_ref()
                .is_some_and(|(_, change)| change.writes())
    }
}

/// 一把要存进保险库的私钥。
pub struct KeyMaterial {
    pub text: Zeroizing<String>,
}

/// 打印出来也看不到私钥。
impl std::fmt::Debug for KeyMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("KeyMaterial(***)")
    }
}

/// 核对过的凭据改动。私钥、口令、密码的条目按凭据的 `secret_id` 存，存好凭据才知道，
/// 所以这里只说怎么改。
#[derive(Debug)]
pub struct CredentialPlan {
    pub draft: CredentialDraft,
    pub key: Option<KeyMaterial>,
    pub password: SecretChange,
    pub passphrase: SecretChange,
}

impl CredentialPlan {
    pub fn writes_secret(&self) -> bool {
        self.key.is_some() || self.password.writes() || self.passphrase.writes()
    }
}

fn bad(message: impl Into<String>) -> CliError {
    CliError::new(ErrorCode::BadRequest, message)
}

pub(super) fn host_not_found(id: &str) -> CliError {
    CliError::new(
        ErrorCode::HostNotFound,
        t!("cli.error.host_not_found", id = id),
    )
}

pub(super) fn credential_not_found(id: &str) -> CliError {
    CliError::new(
        ErrorCode::CredentialNotFound,
        t!("cli.error.credential_not_found", id = id),
    )
}

/// CLI 叫 `id` 的主机。
pub fn find_host(data: &HostData, id: &str) -> Result<HostId, CliError> {
    data.hosts()
        .iter()
        .find(|host| host.public_id.as_str() == id.trim())
        .map(|host| host.id)
        .ok_or_else(|| host_not_found(id))
}

/// CLI 叫 `id` 的凭据。
pub fn find_credential<'a>(data: &'a HostData, id: &str) -> Result<&'a Credential, CliError> {
    data.credentials()
        .iter()
        .find(|credential| credential.secret_id.as_str() == id.trim())
        .ok_or_else(|| credential_not_found(id))
}

/// `fields` 叠在主机 `existing` 现在的样子上（`None` 是新主机的默认值），或者第一处不对的地方，
/// 能用主机表单的话就用它的话。
pub fn plan_host(
    fields: HostFields,
    existing: Option<HostId>,
    data: &HostData,
) -> Result<HostPlan, CliError> {
    let current = existing.and_then(|id| data.host(id));
    let mut draft = current.map_or_else(
        || HostDraft::new("", "", SSH_PORT, "", AuthKind::default()),
        |host| host.draft(),
    );
    if let Some(name) = fields.name {
        draft.name = name.into();
    }
    if let Some(address) = fields.host {
        draft.address = address.into();
    }
    if let Some(port) = fields.port {
        draft.port = port;
    }
    if let Some(notes) = fields.notes {
        draft.notes = notes.into();
    }

    // 怎么登录：点名一条凭据，或者自己登录。
    let named = match &fields.credential {
        Some(Some(id)) => Some(find_credential(data, id)?),
        _ => None,
    };
    let credential = match (fields.auth, named) {
        (Some(AuthChoice::Password | AuthChoice::NoPassword), Some(_)) => {
            return Err(bad(t!("cli.host.credential_with_own_auth")));
        }
        (Some(AuthChoice::Password | AuthChoice::NoPassword), None) => None,
        (_, Some(named)) => Some(named),
        (_, None) if fields.credential == Some(None) => None,
        (_, None) => draft.credential.and_then(|id| data.credential(id)),
    };
    match (fields.auth, credential) {
        (Some(AuthChoice::Credential), None) => return Err(bad(t!("cli.host.no_credential"))),
        (_, Some(credential)) => {
            if let Some(user) = &fields.user
                && user.trim() != credential.user.as_ref()
            {
                return Err(bad(t!("cli.host.credential_user", user = credential.user)));
            }
            draft.credential = Some(credential.id);
            draft.user = credential.user.clone();
            draft.auth = AuthKind::default();
        }
        (auth, None) => {
            let previous = draft.credential.and_then(|id| data.credential(id));
            draft.auth = match (auth, previous) {
                (Some(AuthChoice::NoPassword), _) => AuthKind::NoPassword,
                (Some(_), _) => AuthKind::Password,
                // 离开凭据又没说怎么登录：和删掉凭据时一样。
                (None, Some(previous)) => previous.kind.without_credential(),
                (None, None) => draft.auth,
            };
            draft.credential = None;
            if let Some(user) = fields.user {
                draft.user = user.into();
            }
        }
    }
    let own_password = draft.credential.is_none() && draft.auth == AuthKind::Password;
    if !own_password && matches!(fields.password, Some(Some(_))) {
        return Err(bad(t!("cli.host.password_without_auth")));
    }

    // 给了路线才有代理密码的字段。
    let mut proxy_field = None;
    if let Some(route) = fields.route {
        draft.route = match route {
            RouteFields::Direct => Route::Direct,
            RouteFields::Jump { hosts } => Route::Jump(jump_hosts(&hosts, existing, data)?),
            RouteFields::Proxy {
                kind,
                host,
                port,
                user,
                password,
            } => {
                let user = user.unwrap_or_default();
                if user.trim().is_empty() && matches!(password, Some(Some(_))) {
                    return Err(bad(t!("cli.host.proxy_password_without_user")));
                }
                proxy_field = Some(password);
                let kind = match kind {
                    ProxyChoice::Http => ProxyKind::Http,
                    ProxyChoice::Socks5 => ProxyKind::Socks5,
                };
                Route::Proxy(ProxySettings::new(kind, host, port).with_user(user))
            }
        };
    }
    let draft = draft.validated().map_err(|error| bad(error.message()))?;

    let password = if own_password {
        SecretChange::from_field(fields.password)
    } else {
        SecretChange::Keep
    };
    let proxy_password = match &draft.route {
        Route::Proxy(proxy) => proxy.password_secret().map(|after| {
            let before = current.and_then(|host| match &host.route {
                Route::Proxy(proxy) => proxy.password_secret(),
                _ => None,
            });
            let change = SecretChange::moving(proxy_field.flatten(), before, &after);
            (after, change)
        }),
        _ => None,
    };
    Ok(HostPlan {
        draft,
        password,
        proxy_password,
    })
}

/// CLI 点名的跳板，照主机数据存的样子，或者为什么不行：只能是保存的主机，不能是自己，
/// 每台一次。主机表单也只让选这些。
fn jump_hosts(
    ids: &[Option<String>],
    own: Option<HostId>,
    data: &HostData,
) -> Result<Vec<Option<HostId>>, CliError> {
    let mut hops = Vec::new();
    for id in ids {
        let Some(id) = id else {
            return Err(bad(
                micaterm_hosts::HostDraftError::DeletedJumpHost.message()
            ));
        };
        let hop = find_host(data, id)?;
        if Some(hop) == own {
            return Err(bad(t!("cli.host.jump_self")));
        }
        if hops.contains(&Some(hop)) {
            return Err(bad(t!("cli.host.jump_repeated", id = id)));
        }
        hops.push(Some(hop));
    }
    Ok(hops)
}

/// `fields` 叠在凭据 `existing` 现在的样子上（`None` 是新的密码凭据），或者第一处不对的地方。
pub fn plan_credential(
    fields: CredentialFields,
    existing: Option<CredentialId>,
    data: &HostData,
) -> Result<CredentialPlan, CliError> {
    let current = existing.and_then(|id| data.credential(id));
    let gives_key = fields.key_path.is_some() || fields.private_key.is_some();
    let kind = match (fields.kind, current) {
        (Some(CredentialKindChoice::Password), _) => CredentialKind::Password,
        (Some(CredentialKindChoice::Key), _) => CredentialKind::Key,
        (Some(CredentialKindChoice::Agent), _) => CredentialKind::Agent,
        (None, Some(current)) => current.kind,
        // 给了私钥就是密钥凭据。
        (None, None) if gives_key => CredentialKind::Key,
        (None, None) => CredentialKind::Password,
    };
    let mut draft = current.map_or_else(|| CredentialDraft::new("", kind, ""), Credential::draft);
    draft.kind = kind;
    if let Some(name) = fields.name {
        draft.name = name.into();
    }
    if let Some(user) = fields.user {
        draft.user = user.into();
    }
    if gives_key && kind != CredentialKind::Key {
        return Err(bad(t!("cli.credential.key_without_kind")));
    }
    if kind != CredentialKind::Password && matches!(fields.password, Some(Some(_))) {
        return Err(bad(t!("cli.credential.password_without_kind")));
    }
    if kind != CredentialKind::Key && matches!(fields.passphrase, Some(Some(_))) {
        return Err(bad(t!("cli.credential.passphrase_without_kind")));
    }
    let text =
        match (fields.key_path, fields.private_key) {
            (Some(_), Some(_)) => return Err(bad(t!("cli.credential.two_keys"))),
            (Some(path), None) => {
                if !path.is_absolute() {
                    return Err(bad(t!(
                        "cli.credential.key_not_absolute",
                        path = path.display()
                    )));
                }
                // 「本机文件」只读这一次，之后和原文件再无关系（同凭据表单）。
                Some(read_small(&path).ok_or_else(|| {
                    bad(t!("cli.credential.key_unreadable", path = path.display()))
                })?)
            }
            (None, Some(text)) => Some(Zeroizing::new(text.expose().to_string())),
            (None, None) => None,
        };
    let pasted = match text {
        Some(text) => Some(PastedKey::parse(&text).map_err(|error| bad(key_error(error)))?),
        None => None,
    };
    // 新的密钥凭据（或者改成密钥的）得给私钥：没有私钥的密钥凭据登不了录。
    let had_key = current.is_some_and(|current| current.kind == CredentialKind::Key);
    if kind == CredentialKind::Key && pasted.is_none() && !had_key {
        return Err(bad(t!("cli.credential.key_required")));
    }
    if let Some(pasted) = &pasted {
        draft.public_key = pasted.public_key_line().map(Into::into);
    }
    let draft = draft.validated().map_err(|error| match error {
        CredentialDraftError::Name => bad(t!("cli.credential.name_required")),
    })?;
    let passphrase = match &pasted {
        // 只有加密的私钥才有口令可存；换了私钥，别的情况都清掉原来的口令（同凭据表单）。
        Some(pasted) => match fields.passphrase {
            Some(Some(passphrase)) if pasted.is_encrypted() && !passphrase.expose().is_empty() => {
                SecretChange::Set(passphrase)
            }
            Some(_) => SecretChange::Delete,
            None if pasted.is_encrypted() => SecretChange::Keep,
            None => SecretChange::Delete,
        },
        None => SecretChange::from_field(fields.passphrase),
    };
    Ok(CredentialPlan {
        draft,
        key: pasted.map(|pasted| KeyMaterial {
            text: Zeroizing::new(pasted.text().to_string()),
        }),
        password: if kind == CredentialKind::Password {
            SecretChange::from_field(fields.password)
        } else {
            SecretChange::Keep
        },
        passphrase: if kind == CredentialKind::Key {
            passphrase
        } else {
            SecretChange::Keep
        },
    })
}

/// 贴进来的（或文件里的）不是私钥时说什么。
fn key_error(error: PastedKeyError) -> String {
    match error {
        PastedKeyError::Empty => t!("cli.credential.key_empty"),
        PastedKeyError::PublicKey => t!("cli.credential.key_is_public"),
        PastedKeyError::Unreadable => t!("cli.credential.key_not_a_key"),
    }
    .to_string()
}

/// `credentials show` 打印的一条凭据。`saved` 回答保险库里有没有某条秘密（锁着也能答）。
pub fn credential_details(
    credential: &Credential,
    data: &HostData,
    saved: &impl Fn(&SecretRef) -> bool,
) -> CredentialDetails {
    let kind = credential.kind;
    CredentialDetails {
        id: credential.secret_id.to_string(),
        name: credential.name.to_string(),
        kind: match kind {
            CredentialKind::Password => CredentialKindChoice::Password,
            CredentialKind::Key => CredentialKindChoice::Key,
            CredentialKind::Agent => CredentialKindChoice::Agent,
        },
        user: credential.user.to_string(),
        public_key: credential.public_key.as_ref().map(ToString::to_string),
        hosts: data
            .hosts_using(credential.id)
            .map(|host| host.public_id.to_string())
            .collect(),
        password_saved: (kind == CredentialKind::Password)
            .then(|| saved(&credential.password_secret())),
        private_key_saved: (kind == CredentialKind::Key)
            .then(|| saved(&credential.private_key_secret())),
        passphrase_saved: (kind == CredentialKind::Key)
            .then(|| saved(&credential.passphrase_secret())),
    }
}

/// 凭据的搜索：名称、用户名、种类或 ID。
pub fn credential_matches(details: &CredentialDetails, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    // 两种语言的叫法，Agent 和用户敲哪个都行。
    let kind = match details.kind {
        CredentialKindChoice::Password => "password 密码", // i18n: keep
        CredentialKindChoice::Key => "key 密钥",           // i18n: keep
        CredentialKindChoice::Agent => "agent ssh agent",
    };
    needle.is_empty()
        || details.name.to_lowercase().contains(&needle)
        || details.user.to_lowercase().contains(&needle)
        || kind.contains(&needle)
        || details.id.to_lowercase() == needle
}

/// `hosts show` 打印、没人去设的字段，从 `hosts create` / `update` 读的东西里拿掉，好让一个的
/// 输出能原样喂给另一个；别的不认识的照样拒绝。
pub fn host_fields(mut value: serde_json::Value) -> Result<HostFields, String> {
    if let Some(object) = value.as_object_mut() {
        for key in ["id", "os", "password_saved"] {
            object.remove(key);
        }
        if let Some(route) = object
            .get_mut("route")
            .and_then(|route| route.as_object_mut())
        {
            route.remove("password_saved");
        }
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// 凭据的 [`host_fields`]。
pub fn credential_fields(mut value: serde_json::Value) -> Result<CredentialFields, String> {
    if let Some(object) = value.as_object_mut() {
        for key in [
            "id",
            "hosts",
            "public_key",
            "password_saved",
            "private_key_saved",
            "passphrase_saved",
        ] {
            object.remove(key);
        }
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use micaterm_hosts::{AuthKind, CredentialDraft, CredentialKind, HostData, HostDraft};

    use super::*;

    /// web 自己用密码登录；一条密码凭据 `部署`（用户 deploy）。
    fn data() -> (HostData, HostId, String) {
        let mut data = HostData::empty();
        let web = data.insert_unnotified(HostDraft::new(
            "web",
            "10.0.0.1",
            22,
            "root",
            AuthKind::Password,
        ));
        let credential = data.insert_credential_unnotified(CredentialDraft::new(
            "部署",
            CredentialKind::Password,
            "deploy",
        ));
        let id = data.credential(credential).unwrap().secret_id.to_string();
        (data, web, id)
    }

    fn fields(json: &str) -> HostFields {
        host_fields(serde_json::from_str(json).unwrap()).unwrap()
    }

    #[test]
    fn a_new_host_takes_the_form_s_defaults_and_its_password() {
        let (data, ..) = data();
        let plan = plan_host(
            fields(r#"{"host": " 10.0.1.12 ", "password": "hunter2"}"#),
            None,
            &data,
        )
        .unwrap();
        assert_eq!(plan.draft.name.as_ref(), "10.0.1.12");
        assert_eq!((plan.draft.port, plan.draft.user.as_ref()), (22, "root"));
        assert!(matches!(plan.password, SecretChange::Set(_)));
        assert!(plan.writes_secret());

        let missing = plan_host(fields(r#"{"name": "x"}"#), None, &data).unwrap_err();
        assert_eq!(missing.code, ErrorCode::BadRequest);
    }

    #[test]
    fn a_credential_brings_its_user_and_another_user_is_refused() {
        let (data, web, credential) = data();
        let plan = plan_host(
            fields(&format!(r#"{{"credential": "{credential}"}}"#)),
            Some(web),
            &data,
        )
        .unwrap();
        assert_eq!(plan.draft.user.as_ref(), "deploy");
        assert!(plan.draft.credential.is_some());
        assert_eq!(plan.password, SecretChange::Keep);

        let other = plan_host(
            fields(&format!(
                r#"{{"credential": "{credential}", "user": "admin"}}"#
            )),
            Some(web),
            &data,
        );
        assert_eq!(other.unwrap_err().code, ErrorCode::BadRequest);
        let unknown = plan_host(
            fields(r#"{"credential": "NoSuchCred000000"}"#),
            Some(web),
            &data,
        );
        assert_eq!(unknown.unwrap_err().code, ErrorCode::CredentialNotFound);
        let own_and_named = plan_host(
            fields(&format!(
                r#"{{"credential": "{credential}", "auth": "no_password"}}"#
            )),
            Some(web),
            &data,
        );
        assert!(own_and_named.is_err());
    }

    #[test]
    fn a_password_for_a_host_without_one_is_refused() {
        let (data, web, _) = data();
        let plan = plan_host(
            fields(r#"{"auth": "no_password", "password": "x"}"#),
            Some(web),
            &data,
        );
        assert_eq!(plan.unwrap_err().code, ErrorCode::BadRequest);
    }

    #[test]
    fn a_proxy_moved_elsewhere_carries_its_password_unless_given_one() {
        let (mut data, web, _) = data();
        let mut draft = data.host(web).unwrap().draft();
        draft.route =
            Route::Proxy(ProxySettings::new(ProxyKind::Http, "proxy", 3128).with_user("me"));
        data.update_unnotified(web, draft);
        let before = SecretRef::proxy("me", "proxy", 3128);

        let plan = plan_host(
            fields(r#"{"route": {"type": "proxy", "host": "proxy2", "port": 3128, "user": "me"}}"#),
            Some(web),
            &data,
        )
        .unwrap();
        let (after, change) = plan.proxy_password.unwrap();
        assert_eq!(after, SecretRef::proxy("me", "proxy2", 3128));
        assert_eq!(change, SecretChange::Carry(before));
    }

    #[test]
    fn jump_hosts_are_saved_hosts_other_than_itself_each_once() {
        let (mut data, web, _) = data();
        let db = data.insert_unnotified(HostDraft::new(
            "db",
            "10.0.0.2",
            22,
            "root",
            AuthKind::Password,
        ));
        let id = |host| data.host(host).unwrap().public_id.to_string();
        let route = |ids: &[String]| {
            fields(&format!(
                r#"{{"route": {{"type": "jump", "hosts": {}}}}}"#,
                serde_json::to_string(ids).unwrap()
            ))
        };
        let plan = plan_host(route(&[id(db)]), Some(web), &data).unwrap();
        assert_eq!(plan.draft.route, Route::Jump(vec![Some(db)]));
        assert!(plan_host(route(&[id(web)]), Some(web), &data).is_err());
        assert!(plan_host(route(&[id(db), id(db)]), Some(web), &data).is_err());
        assert!(plan_host(route(&[]), Some(web), &data).is_err());
    }

    #[test]
    fn what_show_prints_can_be_fed_back() {
        let shown = r#"{"id": "x", "name": "web", "host": "10.0.0.1", "port": 22,
            "user": "root", "auth": "password", "credential": null, "password_saved": true,
            "route": {"type": "proxy", "kind": "http", "host": "p", "port": 1, "user": null,
                      "password_saved": false},
            "notes": "", "os": "ubuntu"}"#;
        let fields = host_fields(serde_json::from_str(shown).unwrap()).unwrap();
        assert_eq!(fields.name.as_deref(), Some("web"));
    }

    fn credential(json: &str) -> CredentialFields {
        credential_fields(serde_json::from_str(json).unwrap()).unwrap()
    }

    #[test]
    fn a_credential_needs_a_name_and_a_key_credential_a_key() {
        let (data, ..) = data();
        assert!(plan_credential(credential(r#"{"user": "x"}"#), None, &data).is_err());
        let plan = plan_credential(
            credential(r#"{"name": "运维", "password": "pw"}"#),
            None,
            &data,
        )
        .unwrap();
        assert_eq!(plan.draft.kind, CredentialKind::Password);
        assert!(plan.writes_secret());
        let keyless = plan_credential(credential(r#"{"name": "k", "kind": "key"}"#), None, &data);
        assert_eq!(keyless.unwrap_err().code, ErrorCode::BadRequest);
        let agent =
            plan_credential(credential(r#"{"name": "a", "kind": "agent"}"#), None, &data).unwrap();
        assert!(!agent.writes_secret());
    }

    #[test]
    fn a_pasted_key_goes_to_the_vault_with_its_public_key() {
        let (data, ..) = data();
        let key =
            micaterm_hosts::GeneratedKey::generate(micaterm_hosts::KeyAlgorithm::Ed25519).unwrap();
        let text = key.encode("me@laptop", "").unwrap();
        let json = serde_json::json!({ "name": "k", "private_key": text.as_str() }).to_string();
        let plan = plan_credential(credential(&json), None, &data).unwrap();
        assert_eq!(plan.draft.kind, CredentialKind::Key);
        assert_eq!(
            plan.draft.public_key.as_deref(),
            Some(key.public_key_line("me@laptop").as_str())
        );
        assert!(plan.key.is_some());
        // 没加密的私钥没有口令可存。
        assert_eq!(plan.passphrase, SecretChange::Delete);

        let public =
            serde_json::json!({ "name": "k", "private_key": key.public_key_line("x") }).to_string();
        assert!(plan_credential(credential(&public), None, &data).is_err());
        let relative = r#"{"name": "k", "key_path": "id_ed25519"}"#;
        assert!(plan_credential(credential(relative), None, &data).is_err());
    }

    #[test]
    fn credentials_are_found_by_kind_in_either_language() {
        let (data, _, id) = data();
        let credential = find_credential(&data, &id).unwrap();
        let details = credential_details(credential, &data, &|_| true);
        assert_eq!(details.password_saved, Some(true));
        assert_eq!(details.private_key_saved, None);
        assert!(credential_matches(&details, "密码"));
        assert!(credential_matches(&details, "password"));
        assert!(credential_matches(&details, "deploy"));
        assert!(!credential_matches(&details, "key"));
    }
}
