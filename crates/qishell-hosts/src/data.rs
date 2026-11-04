// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-hosts/src/data.rs
// 裁剪：去掉同步（`sync_revision`、`edited_ms`、`secret_edits`、`sync_*`）、Telnet、串口、
// 项目终端、端口转发、主机分组（树形拖动改成平铺列表里重新排序）、临时连接和外部连接、
// 右侧栏工具字段、SFTP 书签、管理 `keys/` 目录的代码（私钥进保险库）、给外部 CLI 用的 capture；
// 把主机拼成 SSH 登录的部分（`login`、`login_of`、`route_login`）挪到 login.rs；
// 主机读哪些秘密由本文件的 `secrets_of` 算，和 login.rs 拼出的登录读的是同一套条目。

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use gpui_shared_string::SharedString;
use zeroize::Zeroizing;

use crate::secrets::{
    NoSecretStore, SecretRef, SecretsLocked, SecretsUnavailable, SharedSecretStore,
};

use super::database::now_seconds;
use super::{
    AuthKind, Credential, CredentialDraft, CredentialId, CredentialKind, Host, HostDatabase,
    HostDraft, HostDrop, HostId, HostOs, Route, SecretId, Snippet, StoredData,
};

mod snippets;

/// The single source of truth for hosts, the credentials hosts log in with,
/// and the command snippets. The app holds it in a GPUI entity, which every
/// panel and dialog observes.
///
/// 主机列表是平铺的：[`Self::hosts`] 按列表顺序（`sort_order`）给出，拖动用
/// [`Self::move_host`] 重新排序。
///
/// A host using a credential keeps a copy of the credential's user name,
/// so everything that shows or keys by `user@host:port` reads the host
/// alone. The store keeps the copy in step: it takes it from the credential
/// when a host is saved or loaded and rewrites it when the credential
/// changes. The copy lives in memory only; the database keeps the user name
/// on the credential alone.
///
/// Mutators mirror the change into the database and say what observers
/// should know as [`HostEvent`]s, which whoever holds the data takes with
/// [`Self::take_events`] after each one; the `*_unnotified` variants are pure
/// in-memory and exist for tests, for loading, and for callers that batch
/// several changes before one notification.
///
/// Memory is authoritative. A failed write costs persistence, never the edit:
/// the change stands and a [`Failure`] is said so the user can be told.
///
/// Passwords, passphrases and private keys never reach the database. They go
/// to the vault through `secrets`, on a background thread (see
/// [`Self::take_secret_jobs`]), and report failures the same way.
pub struct HostData {
    /// 按列表顺序（`sort_order`，再按插入先后）。
    hosts: Vec<Host>,
    next_host_id: u64,
    /// Hosts in the order they last connected, most recent first.
    recent: Vec<HostId>,
    /// Credentials, in the order the credential list shows them.
    credentials: Vec<Credential>,
    next_credential_id: u64,
    /// Command snippets, in the order they were made.
    snippets: Vec<Snippet>,
    next_snippet_id: u64,
    /// `None` for a memory-only store, as used by tests.
    database: Option<HostDatabase>,
    /// Where secrets go: the vault given to `with_secrets`. Defaults to a
    /// store that keeps nothing, so unit tests never touch a real one.
    secrets: SharedSecretStore,
    /// What happened that whoever holds the data has yet to pass on.
    events: RefCell<Vec<HostEvent>>,
    /// The vault writes waiting for whoever holds the data to run them.
    secret_jobs: Vec<SecretJob>,
    /// Inside a batch (see [`Self::begin_batch`]).
    batch: RefCell<Option<Batch>>,
}

/// How many hosts the start card lists as recently connected.
const MAX_RECENT: usize = 10;

/// A change the data writes through to the database, to say which one
/// could not be saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Change {
    NewHost,
    SaveHost,
    DeleteHost,
    DuplicateHost,
    HostOrder,
    NewCredential,
    SaveCredential,
    DeleteCredential,
    Recent,
    HostOs,
    NewSnippet,
    SaveSnippet,
    DeleteSnippet,
    Batch,
}

/// What could not be kept. The change stands in memory all the same.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Failure {
    /// `change` is not in the database, which said `error`.
    NotSaved { change: Change, error: String },
    /// The vault did not write `secret` (or delete it, not `writing`).
    Secret {
        secret: SecretRef,
        writing: bool,
        error: SecretError,
    },
}

/// Why a secret is not kept.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SecretError {
    /// There is nowhere to keep secrets.
    Unavailable,
    /// 保险库锁着：调用方弹出解锁框，解锁后再写。
    Locked,
    /// The store's own failure, as it put it.
    Store(String),
}

impl SecretError {
    /// A store call's failure.
    pub fn of(error: &anyhow::Error) -> Self {
        if error.downcast_ref::<SecretsUnavailable>().is_some() {
            SecretError::Unavailable
        } else if error.downcast_ref::<SecretsLocked>().is_some() {
            SecretError::Locked
        } else {
            SecretError::Store(error.to_string())
        }
    }
}

/// What happened to the data, for whoever holds it to pass on: the GPUI
/// entity as events and notifications.
#[derive(Debug)]
pub enum HostEvent {
    /// Something could not be kept; the change stands in memory.
    Failed(Failure),
    /// What observers see has changed.
    Changed,
}

/// A vault write, to run on a background thread: the call blocks.
pub struct SecretJob {
    secrets: SharedSecretStore,
    secret: SecretRef,
    value: Option<Zeroizing<String>>,
}

impl SecretJob {
    /// Write the secret, or delete it.
    pub fn run(self) -> SecretOutcome {
        let writing = self.value.is_some();
        let result = match &self.value {
            Some(value) => self.secrets.set(&self.secret, value),
            None => self.secrets.delete(&self.secret),
        };
        SecretOutcome {
            secret: self.secret,
            writing,
            error: result.err().map(|error| SecretError::of(&error)),
        }
    }
}

/// How a [`SecretJob`] went.
pub struct SecretOutcome {
    secret: SecretRef,
    writing: bool,
    error: Option<SecretError>,
}

impl SecretOutcome {
    /// What it could not do, if anything.
    pub fn failure(&self) -> Option<Failure> {
        self.error.clone().map(|error| Failure::Secret {
            secret: self.secret.clone(),
            writing: self.writing,
            error,
        })
    }
}

/// A batch of database writes in one transaction.
struct Batch {
    /// Whether the database began one.
    begun: bool,
    /// The failures said already, so a burst of writes failing the same way
    /// says so once.
    reported: HashSet<Failure>,
}

impl HostData {
    /// An empty memory-only store.
    pub fn empty() -> Self {
        Self {
            hosts: Vec::new(),
            next_host_id: 1,
            recent: Vec::new(),
            credentials: Vec::new(),
            next_credential_id: 1,
            snippets: Vec::new(),
            next_snippet_id: 1,
            database: None,
            secrets: Arc::new(NoSecretStore),
            events: RefCell::new(Vec::new()),
            secret_jobs: Vec::new(),
            batch: RefCell::new(None),
        }
    }

    /// Attach the vault. The app does this once at startup; tests attach an
    /// in-memory store instead.
    pub fn with_secrets(mut self, secrets: SharedSecretStore) -> Self {
        self.secrets = secrets;
        self
    }

    /// Read everything back from `database` and keep writing to it. Ids carry
    /// over from disk, so the allocators resume past the largest stored id.
    pub fn load(database: HostDatabase) -> rusqlite::Result<Self> {
        let StoredData {
            mut hosts,
            mut recent,
            credentials,
            snippets,
        } = database.load()?;
        recent.truncate(MAX_RECENT);
        // A host using a credential has no user name of its own on disk:
        // memory keeps a copy of the credential's, taken here.
        for host in &mut hosts {
            let draft = normalized(&credentials, host.draft());
            host.user = draft.user;
            host.auth = draft.auth;
            host.credential = draft.credential;
        }
        Ok(Self {
            next_host_id: hosts.iter().map(|s| s.id.0).max().unwrap_or(0) + 1,
            next_credential_id: credentials.iter().map(|c| c.id.0).max().unwrap_or(0) + 1,
            next_snippet_id: snippets.iter().map(|s| s.id.0).max().unwrap_or(0) + 1,
            hosts,
            recent,
            credentials,
            snippets,
            database: Some(database),
            ..Self::empty()
        })
    }

    /// Sample data for tests. Production starts from an empty database, so
    /// nothing outside tests builds a store this way; the UI tests depend
    /// on the exact shape (ids 1..=6 in the order listed here, web-01 and
    /// staging-api connected, staging-api last).
    pub fn seed() -> Self {
        let mut store = Self::empty();
        // Test data in Chinese where the UI tests look for it.
        let drafts = [
            HostDraft::new("web-01", "10.0.1.12", 22, "root", AuthKind::Password),
            HostDraft::new("web-02", "10.0.1.13", 22, "root", AuthKind::Password),
            HostDraft::new("db-01", "10.0.2.5", 22, "postgres", AuthKind::Password),
            HostDraft::new(
                "staging-api",
                "10.0.9.20",
                2222,
                "deploy",
                AuthKind::Password,
            ),
            HostDraft::new("qa-runner", "10.0.9.31", 22, "ci", AuthKind::Password),
            HostDraft::new("dev-box", "192.168.1.20", 22, "xuz", AuthKind::Password),
        ];
        for draft in drafts {
            store.insert_unnotified(draft);
        }
        for name in ["web-01", "staging-api"] {
            if let Some(id) = store.hosts.iter().find(|s| s.name == name).map(|s| s.id) {
                store.record_connection_unnotified(id);
            }
        }
        store
    }

    /// The saved hosts, in list order.
    pub fn hosts(&self) -> &[Host] {
        &self.hosts
    }

    pub fn host(&self, id: HostId) -> Option<&Host> {
        self.hosts.iter().find(|host| host.id == id)
    }

    fn host_mut(&mut self, id: HostId) -> Option<&mut Host> {
        self.hosts.iter_mut().find(|host| host.id == id)
    }

    /// Hosts in the order they last connected, most recent first. A
    /// host joins (or moves to the front) each time one of its terminals
    /// or SFTP tabs connects.
    pub fn recent_hosts(&self) -> impl Iterator<Item = &Host> {
        self.recent.iter().filter_map(|id| self.host(*id))
    }

    /// Add a host at the end of the list.
    pub fn insert(&mut self, draft: HostDraft) -> HostId {
        let id = self.insert_unnotified(draft);
        if let Some(host) = self.host(id) {
            self.persist(Change::NewHost, |db| db.insert_host(host));
        }
        self.changed();
        id
    }

    pub fn insert_unnotified(&mut self, draft: HostDraft) -> HostId {
        let mut draft = normalized(&self.credentials, draft);
        draft.route = self.normalized_route(draft.route, None);
        let id = HostId(self.next_host_id);
        self.next_host_id += 1;
        let mut host = Host::new(id, draft);
        host.sort_order = self.last_host_order();
        self.hosts.push(host);
        id
    }

    /// Change a host. What is connected stays connected as it was: a new
    /// login is the next connection's, as in Xshell.
    pub fn update(&mut self, id: HostId, draft: HostDraft) -> bool {
        let previous = self.host(id).map(|host| self.secrets_of(host));
        let updated = self.update_unnotified(id, draft);
        if updated {
            if let Some(host) = self.host(id) {
                self.persist(Change::SaveHost, |db| db.update_host(host));
            }
            // The host may no longer go through the proxy it did, so that
            // proxy's password is an orphan unless another host still uses
            // it. 主机自己的密码按主机 id 存，改了地址也还在。
            for secret in previous.unwrap_or_default() {
                self.forget_password(secret);
            }
            self.changed();
        }
        updated
    }

    pub fn update_unnotified(&mut self, id: HostId, draft: HostDraft) -> bool {
        let mut draft = normalized(&self.credentials, draft);
        draft.route = self.normalized_route(draft.route, Some(id));
        let Some(host) = self.host_mut(id) else {
            return false;
        };
        // The form owns none of these, so rebuilding from the draft must not
        // drop them.
        let (os, sort_order) = (host.os, host.sort_order);
        *host = Host::new(id, draft);
        host.os = os;
        host.sort_order = sort_order;
        true
    }

    /// Delete a host. The hosts that went through it keep its place in
    /// their jump hosts, empty, and are not reconnected: deleting a host does
    /// not drop a working connection behind it.
    pub fn remove(&mut self, id: HostId) -> bool {
        let Some(own) = self.host(id).map(Host::password_secret) else {
            return false;
        };
        let secrets = self.host(id).map(|host| self.secrets_of(host));
        let removed = self.remove_unnotified(id);
        if removed {
            self.persist(Change::DeleteHost, |db| db.remove_host(id));
            // 主机自己的密码跟着主机走；代理的密码要等最后一台用它的主机没了。
            self.save_secret(own, None);
            for secret in secrets.unwrap_or_default() {
                self.forget_password(secret);
            }
            self.changed();
        }
        removed
    }

    pub fn remove_unnotified(&mut self, id: HostId) -> bool {
        let before = self.hosts.len();
        self.hosts.retain(|s| s.id != id);
        if self.hosts.len() == before {
            return false;
        }
        // As the database's `ON DELETE SET NULL` does.
        for host in &mut self.hosts {
            if let Route::Jump(hops) = &mut host.route {
                for hop in hops.iter_mut().filter(|hop| **hop == Some(id)) {
                    *hop = None;
                }
            }
        }
        self.recent.retain(|recent| *recent != id);
        true
    }

    /// Copy a host as `name` (the app's `<name> 副本`), placed right after
    /// the original.
    pub fn duplicate(&mut self, id: HostId, name: SharedString) -> Option<HostId> {
        let copy = self.duplicate_unnotified(id, name)?;
        if let Some(host) = self.host(copy) {
            self.persist(Change::DuplicateHost, |db| db.insert_host(host));
            self.persist(Change::HostOrder, |db| db.save_host_order(&self.hosts));
        }
        self.changed();
        Some(copy)
    }

    pub fn duplicate_unnotified(&mut self, id: HostId, name: SharedString) -> Option<HostId> {
        let ix = self.hosts.iter().position(|s| s.id == id)?;
        let mut draft = self.hosts[ix].draft();
        draft.name = name;
        let copy_id = HostId(self.next_host_id);
        self.next_host_id += 1;
        let mut copy = Host::new(copy_id, draft);
        // Same host, so the copy already knows what it will find there.
        copy.os = self.hosts[ix].os;
        copy.sort_order = self.hosts[ix].sort_order + 1;
        for host in &mut self.hosts {
            if host.sort_order >= copy.sort_order {
                host.sort_order += 1;
            }
        }
        self.hosts.insert(ix + 1, copy);
        Some(copy_id)
    }

    /// 拖动排序：把 `id` 放到 `drop` 说的那台主机前面或后面。没动（放回原处、放到自己身上、
    /// 主机不存在）时返回 false，什么也不写。
    pub fn move_host(&mut self, id: HostId, drop: HostDrop) -> bool {
        if !self.move_host_unnotified(id, drop) {
            return false;
        }
        self.persist(Change::HostOrder, |db| db.save_host_order(&self.hosts));
        self.changed();
        true
    }

    pub fn move_host_unnotified(&mut self, id: HostId, drop: HostDrop) -> bool {
        let (target, after) = match drop {
            HostDrop::Before(target) => (target, false),
            HostDrop::After(target) => (target, true),
        };
        if target == id || self.host(id).is_none() {
            return false;
        }
        let siblings = self.hosts.iter().map(|s| (s.sort_order, s.id)).collect();
        let Some(ids) = reordered(siblings, id, Some(target), after) else {
            return false;
        };
        for host in &mut self.hosts {
            if let Some(order) = ids.iter().position(|sibling| *sibling == host.id) {
                host.sort_order = order as i64;
            }
        }
        self.hosts.sort_by_key(|host| host.sort_order);
        true
    }

    pub fn credentials(&self) -> &[Credential] {
        &self.credentials
    }

    pub fn credential(&self, id: CredentialId) -> Option<&Credential> {
        self.credentials
            .iter()
            .find(|credential| credential.id == id)
    }

    /// The hosts that log in with `credential`.
    pub fn hosts_using(&self, credential: CredentialId) -> impl Iterator<Item = &Host> {
        self.hosts
            .iter()
            .filter(move |host| host.credential == Some(credential))
    }

    /// How many hosts other than `ids` go through one of them as a jump
    /// host: what deleting `ids` leaves without a way through.
    pub fn jump_users(&self, ids: &[HostId]) -> usize {
        self.jumping_through(ids).len()
    }

    /// The hosts other than `ids` that go through one of them, in list
    /// order: the ones the delete confirmation names.
    pub fn jumping_through(&self, ids: &[HostId]) -> Vec<HostId> {
        self.hosts
            .iter()
            .filter(|host| {
                !ids.contains(&host.id) && host.route.jump_hosts().any(|hop| ids.contains(&hop))
            })
            .map(|host| host.id)
            .collect()
    }

    /// Add a credential at the end of the list.
    pub fn insert_credential(&mut self, draft: CredentialDraft) -> CredentialId {
        let id = self.insert_credential_unnotified(draft);
        if let Some(credential) = self.credential(id) {
            self.persist(Change::NewCredential, |db| db.insert_credential(credential));
        }
        self.changed();
        id
    }

    pub fn insert_credential_unnotified(&mut self, draft: CredentialDraft) -> CredentialId {
        let sort_order = self
            .credentials
            .iter()
            .map(|credential| credential.sort_order)
            .max()
            .unwrap_or(-1)
            + 1;
        let id = CredentialId(self.next_credential_id);
        self.next_credential_id += 1;
        let mut credential = Credential::new(id, draft);
        credential.secret_id = SecretId::generate_unused(|candidate| {
            self.credentials
                .iter()
                .any(|credential| &credential.secret_id == candidate)
        });
        credential.sort_order = sort_order;
        self.credentials.push(credential);
        id
    }

    /// Replace the editable fields of a credential. Every host using it
    /// takes the new user name; what is connected stays as it was. A
    /// credential that changes kind leaves the secrets of its old kind
    /// behind, so they go.
    pub fn update_credential(&mut self, id: CredentialId, draft: CredentialDraft) -> bool {
        let Some(previous) = self.credential(id).cloned() else {
            return false;
        };
        let read_before: Vec<SecretRef> = self
            .hosts_using(id)
            .flat_map(|host| self.secrets_of(host))
            .collect();
        if !self.update_credential_unnotified(id, draft) {
            return false;
        }
        if let Some(credential) = self.credential(id) {
            self.persist(Change::SaveCredential, |db| {
                db.update_credential(credential)
            });
        }
        let kept = self
            .credential(id)
            .map(Credential::secrets)
            .unwrap_or_default();
        for secret in previous.secrets() {
            if !kept.contains(&secret) {
                self.save_secret(secret, None);
            }
        }
        for secret in read_before {
            self.forget_password(secret);
        }
        self.changed();
        true
    }

    pub fn update_credential_unnotified(
        &mut self,
        id: CredentialId,
        draft: CredentialDraft,
    ) -> bool {
        let Some(credential) = self
            .credentials
            .iter_mut()
            .find(|credential| credential.id == id)
        else {
            return false;
        };
        let secret_id = credential.secret_id.clone();
        let sort_order = credential.sort_order;
        *credential = Credential::new(id, draft);
        credential.secret_id = secret_id;
        credential.sort_order = sort_order;
        let user = credential.user.clone();
        for host in &mut self.hosts {
            if host.credential == Some(id) {
                host.user = user.clone();
            }
        }
        true
    }

    /// Delete a credential with its secrets. The hosts using it log in on
    /// their own again, automatically and as the same user, and are
    /// returned. They are not reconnected: deleting a credential does not
    /// drop a working connection.
    pub fn remove_credential(&mut self, id: CredentialId) -> Vec<HostId> {
        let Some(credential) = self.credential(id).cloned() else {
            return Vec::new();
        };
        let released = self.remove_credential_unnotified(id);
        let auth = credential.kind.without_credential();
        self.persist(Change::DeleteCredential, |db| {
            db.remove_credential(id, auth)
        });
        for secret in credential.secrets() {
            self.save_secret(secret, None);
        }
        self.changed();
        released
    }

    pub fn remove_credential_unnotified(&mut self, id: CredentialId) -> Vec<HostId> {
        let Some(index) = self
            .credentials
            .iter()
            .position(|credential| credential.id == id)
        else {
            return Vec::new();
        };
        let auth = self.credentials.remove(index).kind.without_credential();
        let mut released = Vec::new();
        for host in &mut self.hosts {
            if host.credential == Some(id) {
                host.credential = None;
                host.auth = auth;
                released.push(host.id);
            }
        }
        released
    }

    /// A terminal or SFTP tab of the host connected: the host goes to the
    /// front of the recent list.
    pub fn record_connection(&mut self, id: HostId) {
        if !self.record_connection_unnotified(id) {
            return;
        }
        self.persist(Change::Recent, |db| db.touch_connected(id, now_seconds()));
        self.changed();
    }

    /// Returns whether it was a saved host's.
    pub fn record_connection_unnotified(&mut self, id: HostId) -> bool {
        if self.host(id).is_none() {
            return false;
        }
        self.recent.retain(|recent| *recent != id);
        self.recent.insert(0, id);
        self.recent.truncate(MAX_RECENT);
        true
    }

    /// Record what a connection found on the host. Called after every
    /// successful connection, so a rebuilt machine corrects itself.
    pub fn set_host_os(&mut self, id: HostId, os: Option<HostOs>) {
        if !self.set_host_os_unnotified(id, os) {
            return;
        }
        self.persist(Change::HostOs, |db| db.set_host_os(id, os));
        self.changed();
    }

    /// Returns whether the recorded system changed.
    pub fn set_host_os_unnotified(&mut self, id: HostId, os: Option<HostOs>) -> bool {
        let Some(host) = self.host_mut(id) else {
            return false;
        };
        if host.os == os {
            return false;
        }
        host.os = os;
        true
    }

    /// Where secrets are read from. The host form uses this to pre-fill
    /// its field; the connection reads its secrets through it.
    pub fn secrets(&self) -> SharedSecretStore {
        self.secrets.clone()
    }

    /// Write a secret to the vault, or delete it when `value` is `None`.
    ///
    /// The vault call blocks, so it is not made here: the write waits in
    /// [`Self::take_secret_jobs`] for whoever holds the data to run it on a
    /// background thread. A failure costs persistence, never the edit: it
    /// comes back as [`Failure::Secret`], exactly like a failed database
    /// write.
    pub fn save_secret(&mut self, secret: SecretRef, value: Option<String>) {
        let job = self.secret_job(secret, value);
        self.secret_jobs.push(job);
    }

    /// The vault write [`Self::save_secret`] queues, for a caller that runs
    /// it itself to know how it went. Hand what it comes to back to
    /// [`Self::secret_done`].
    pub fn secret_job(&self, secret: SecretRef, value: Option<String>) -> SecretJob {
        SecretJob {
            secrets: self.secrets(),
            secret,
            value: value.map(Zeroizing::new),
        }
    }

    /// The vault writes waiting to run, oldest first.
    pub fn take_secret_jobs(&mut self) -> Vec<SecretJob> {
        std::mem::take(&mut self.secret_jobs)
    }

    /// Take how a [`SecretJob`] went.
    pub fn secret_done(&mut self, outcome: SecretOutcome) {
        match outcome.failure() {
            None => self.changed(),
            // Never in a batch: that is over by the time a vault write is.
            Some(failure) => self.event(HostEvent::Failed(failure)),
        }
    }

    /// 这台主机登录时会读的密码条目：用密码凭据时是凭据的密码，否则是主机自己的密码
    /// （密钥、Agent 凭据和「无密码」的主机也读它，服务器在密钥之后还要密码时用）；
    /// 走代理且代理有用户名时，再加上代理的密码。照上游 `HostConnection::password_entries`。
    fn secrets_of(&self, host: &Host) -> Vec<SecretRef> {
        let credential = host.credential.and_then(|id| self.credential(id));
        let login = match credential {
            Some(credential) if credential.kind == CredentialKind::Password => {
                credential.password_secret()
            }
            _ => host.password_secret(),
        };
        let proxy = match &host.route {
            Route::Proxy(proxy) => proxy.password_secret(),
            _ => None,
        };
        [Some(login), proxy].into_iter().flatten().collect()
    }

    /// Delete a proxy's saved password once no host goes through it. A
    /// host's own password goes with the host, a credential's secrets with
    /// the credential.
    fn forget_password(&mut self, secret: SecretRef) {
        if secret.is_shared_by_hosts() && !self.password_in_use(&secret) {
            self.save_secret(secret, None);
        }
    }

    /// Whether any host still reads this password entry. A proxy's is
    /// shared, so it may only be cleaned up once the last host using it is
    /// gone.
    fn password_in_use(&self, secret: &SecretRef) -> bool {
        self.hosts
            .iter()
            .any(|host| self.secrets_of(host).contains(secret))
    }

    /// Write a change through to the database, when there is one. A failed
    /// write becomes an event; the in-memory change stands.
    fn persist(&self, change: Change, write: impl FnOnce(&HostDatabase) -> rusqlite::Result<()>) {
        if let Some(database) = &self.database
            && let Err(error) = write(database)
        {
            self.report(Failure::NotSaved {
                change,
                error: error.to_string(),
            });
        }
    }

    /// Say that something could not be kept, as an event.
    fn report(&self, failure: Failure) {
        let first = self
            .batch
            .borrow_mut()
            .as_mut()
            .is_none_or(|batch| batch.reported.insert(failure.clone()));
        if first {
            self.event(HostEvent::Failed(failure));
        }
    }

    /// Say what observers of the data should know.
    fn event(&self, event: HostEvent) {
        self.events.borrow_mut().push(event);
    }

    /// Say that what observers see has changed.
    fn changed(&self) {
        self.event(HostEvent::Changed);
    }

    /// What happened since last asked, in order.
    pub fn take_events(&mut self) -> Vec<HostEvent> {
        std::mem::take(self.events.get_mut())
    }

    /// Start writing every change to the database in one transaction, until
    /// [`Self::end_batch`]: a burst of them pays for one commit rather than
    /// one each. Each write still lands whole or not at all, and the memory
    /// changes stand as they always do. A failure repeated within it is said
    /// once.
    ///
    /// Inside a batch already this joins it and returns false: end only
    /// the batch that this returned true for.
    pub fn begin_batch(&mut self) -> bool {
        if self.batch.get_mut().is_some() {
            return false;
        }
        let begun = self
            .database
            .as_ref()
            .is_some_and(|database| database.begin_batch().is_ok());
        *self.batch.get_mut() = Some(Batch {
            begun,
            reported: HashSet::new(),
        });
        true
    }

    /// Commit the batch [`Self::begin_batch`] began.
    pub fn end_batch(&mut self) {
        let Some(batch) = self.batch.get_mut().take() else {
            return;
        };
        if batch.begun
            && let Some(database) = &self.database
            && let Err(error) = database.end_batch()
        {
            self.report(Failure::NotSaved {
                change: Change::Batch,
                error: error.to_string(),
            });
        }
    }

    /// The sort order that puts a host last in the list.
    fn last_host_order(&self) -> i64 {
        self.hosts.iter().map(|s| s.sort_order).max().unwrap_or(-1) + 1
    }

    /// `route` as the store keeps it for host `own` (`None` while it is
    /// being created): no host jumps through itself, a jump host that is
    /// not there is a deleted one, and a jump with no hosts is no jump.
    fn normalized_route(&self, route: Route, own: Option<HostId>) -> Route {
        match route {
            Route::Jump(hops) => {
                let hops: Vec<_> = hops
                    .into_iter()
                    .filter(|hop| hop.is_none() || *hop != own)
                    .map(|hop| hop.filter(|id| self.host(*id).is_some()))
                    .collect();
                if hops.is_empty() {
                    Route::Direct
                } else {
                    Route::Jump(hops)
                }
            }
            route => route,
        }
    }
}

/// The order `id` and its new neighbours end up in: `id` goes before or
/// after `target`, or to the end without one. `siblings` are the rows as
/// `(sort_order, id)`, `id` among them. `None` when `target` is not one of
/// them, or nothing would move.
fn reordered<Id: Copy + Ord>(
    mut siblings: Vec<(i64, Id)>,
    id: Id,
    target: Option<Id>,
    after: bool,
) -> Option<Vec<Id>> {
    siblings.sort();
    let old_index = siblings.iter().position(|(_, sibling)| *sibling == id);
    let mut ids: Vec<Id> = siblings
        .into_iter()
        .map(|(_, sibling)| sibling)
        .filter(|sibling| *sibling != id)
        .collect();
    let index = match target {
        Some(target) => ids.iter().position(|sibling| *sibling == target)? + usize::from(after),
        None => ids.len(),
    };
    if old_index == Some(index) {
        return None;
    }
    ids.insert(index, id);
    Some(ids)
}

/// `draft` as the store keeps it. A host using a credential logs in as
/// the credential's user and keeps nothing of its own login; one naming a
/// credential that is not there logs in on its own.
fn normalized(credentials: &[Credential], mut draft: HostDraft) -> HostDraft {
    let credential = draft
        .credential
        .and_then(|id| credentials.iter().find(|credential| credential.id == id));
    match credential {
        Some(credential) => {
            draft.user = credential.user.clone();
            draft.auth = AuthKind::default();
        }
        None => draft.credential = None,
    }
    draft
}

#[cfg(test)]
mod tests {
    use super::super::{ProxyKind, ProxySettings};
    use super::*;
    use crate::secrets::{InMemorySecretStore, SecretStore as _};

    fn draft(name: &str) -> HostDraft {
        HostDraft::new(name, "10.0.0.1", 22, "root", AuthKind::Password)
    }

    fn names(store: &HostData) -> Vec<&str> {
        store
            .hosts()
            .iter()
            .map(|host| host.name.as_ref())
            .collect()
    }

    fn password_credential(store: &mut HostData, user: &str) -> CredentialId {
        store.insert_credential_unnotified(CredentialDraft::new(
            "运维",
            CredentialKind::Password,
            user,
        ))
    }

    /// Run the vault writes the changes queued, as the app does on a
    /// background thread.
    fn run_secret_jobs(store: &mut HostData) {
        for job in store.take_secret_jobs() {
            let outcome = job.run();
            store.secret_done(outcome);
        }
    }

    #[test]
    fn a_host_using_a_credential_logs_in_as_its_user() {
        let mut store = HostData::empty();
        let credential = password_credential(&mut store, "deploy");
        let mut form = draft("web").with_credential(credential);
        // Whatever the form still held of its own login is dropped.
        form.auth = AuthKind::NoPassword;
        let web = store.insert_unnotified(form);

        let host = store.host(web).unwrap();
        assert_eq!(host.credential, Some(credential));
        assert_eq!(host.user.as_ref(), "deploy");
        assert_eq!(host.auth, AuthKind::Password);
        assert_eq!(
            store.secrets_of(host),
            [store.credential(credential).unwrap().password_secret()]
        );
        assert_eq!(store.hosts_using(credential).count(), 1);
    }

    #[test]
    fn a_draft_naming_a_missing_credential_is_saved_as_manual() {
        let mut store = HostData::empty();
        let web = store.insert_unnotified(draft("web").with_credential(CredentialId(9)));
        let host = store.host(web).unwrap();
        assert_eq!(host.credential, None);
        assert_eq!(host.user.as_ref(), "root");
    }

    #[test]
    fn renaming_a_credentials_user_renames_every_host_using_it() {
        let mut store = HostData::empty();
        let credential = password_credential(&mut store, "deploy");
        let web = store.insert_unnotified(draft("web").with_credential(credential));
        let db = store.insert_unnotified(draft("db"));

        let mut edited = store.credential(credential).unwrap().draft();
        edited.user = "admin".into();
        assert!(store.update_credential_unnotified(credential, edited));
        assert_eq!(store.host(web).unwrap().user.as_ref(), "admin");
        assert_eq!(store.host(web).unwrap().endpoint(), "admin@10.0.0.1:22");
        assert_eq!(store.host(db).unwrap().user.as_ref(), "root");
        assert!(!store.update_credential_unnotified(
            CredentialId(9),
            CredentialDraft::new("x", CredentialKind::Agent, "x")
        ));
    }

    #[test]
    fn changing_only_a_credentials_name_changes_no_login() {
        let mut store = HostData::empty();
        let credential = password_credential(&mut store, "deploy");
        let web = store.insert_unnotified(draft("web").with_credential(credential));
        let before = store.secrets_of(store.host(web).unwrap());
        let secret_id = store.credential(credential).unwrap().secret_id.clone();

        let mut edited = store.credential(credential).unwrap().draft();
        edited.name = "生产运维".into();
        assert!(store.update_credential_unnotified(credential, edited));
        assert_eq!(store.secrets_of(store.host(web).unwrap()), before);
        // The vault entry stays the credential's.
        assert_eq!(store.credential(credential).unwrap().secret_id, secret_id);
    }

    #[test]
    fn deleting_a_credential_leaves_its_hosts_on_their_own_with_the_user() {
        let mut store = HostData::empty();
        let credential = store.insert_credential_unnotified(CredentialDraft::new(
            "部署",
            CredentialKind::Key,
            "deploy",
        ));
        let web = store.insert_unnotified(draft("web").with_credential(credential));
        let db = store.insert_unnotified(draft("db"));

        assert_eq!(store.remove_credential_unnotified(credential), [web]);
        assert!(store.credentials().is_empty());
        let host = store.host(web).unwrap();
        assert_eq!(host.credential, None);
        // Without its key, it tries the agent and the default keys.
        assert_eq!(host.auth, AuthKind::NoPassword);
        assert_eq!(host.user.as_ref(), "deploy");
        assert_eq!(store.host(db).unwrap().user.as_ref(), "root");
        assert!(store.remove_credential_unnotified(credential).is_empty());

        // Without its password, it asks for one.
        let credential = password_credential(&mut store, "deploy");
        let api = store.insert_unnotified(draft("api").with_credential(credential));
        store.remove_credential_unnotified(credential);
        assert_eq!(store.host(api).unwrap().auth, AuthKind::Password);
    }

    #[test]
    fn deleting_a_credential_deletes_its_secrets_from_the_vault() {
        let secrets = Arc::new(InMemorySecretStore::default());
        let mut store = HostData::empty().with_secrets(secrets.clone());
        let key =
            store.insert_credential(CredentialDraft::new("部署", CredentialKind::Key, "deploy"));
        let credential = store.credential(key).unwrap().clone();
        secrets
            .set(
                &credential.private_key_secret(),
                "-----BEGIN OPENSSH PRIVATE KEY-----",
            )
            .unwrap();
        secrets
            .set(&credential.passphrase_secret(), "correct horse")
            .unwrap();
        // 别的凭据的秘密不受影响。
        let other = password_credential(&mut store, "root");
        let other = store.credential(other).unwrap().password_secret();
        secrets.set(&other, "hunter2").unwrap();

        store.remove_credential(key);
        run_secret_jobs(&mut store);
        assert_eq!(secrets.len(), 1);
        assert!(secrets.get(&other).unwrap().is_some());
    }

    #[test]
    fn a_credential_that_changes_kind_drops_the_secrets_of_its_old_kind() {
        let secrets = Arc::new(InMemorySecretStore::default());
        let mut store = HostData::empty().with_secrets(secrets.clone());
        let id = password_credential(&mut store, "deploy");
        let password = store.credential(id).unwrap().password_secret();
        secrets.set(&password, "hunter2").unwrap();

        // 只改名字：密码留着。
        let mut renamed = store.credential(id).unwrap().draft();
        renamed.name = "生产运维".into();
        assert!(store.update_credential(id, renamed));
        run_secret_jobs(&mut store);
        assert!(secrets.get(&password).unwrap().is_some());

        let mut key = store.credential(id).unwrap().draft();
        key.kind = CredentialKind::Key;
        assert!(store.update_credential(id, key));
        run_secret_jobs(&mut store);
        assert!(secrets.get(&password).unwrap().is_none());
        assert_eq!(
            store.credential(id).unwrap().private_key_secret(),
            SecretRef::private_key(store.credential(id).unwrap().secret_id.as_str())
        );
    }

    #[test]
    fn a_copy_of_a_host_uses_the_same_credential() {
        let mut store = HostData::empty();
        let credential = password_credential(&mut store, "deploy");
        let web = store.insert_unnotified(draft("web").with_credential(credential));
        let copy = store.duplicate_unnotified(web, "copy".into()).unwrap();
        assert_eq!(store.host(copy).unwrap().credential, Some(credential));
        assert_eq!(store.hosts_using(credential).count(), 2);
    }

    #[test]
    fn a_host_with_a_password_credential_reads_the_credential_s_password() {
        let mut store = HostData::empty();
        let credential = password_credential(&mut store, "root");
        let web = store.insert_unnotified(draft("web"));
        let own = store.host(web).unwrap().password_secret();
        assert_eq!(store.secrets_of(store.host(web).unwrap()), [own]);

        store.update_unnotified(web, draft("web").with_credential(credential));
        let secret = store.credential(credential).unwrap().password_secret();
        assert_eq!(store.secrets_of(store.host(web).unwrap()), [secret]);
    }

    #[test]
    fn load_resumes_the_credential_id_sequence_and_repairs_usernames() {
        let database = HostDatabase::in_memory().unwrap();
        let credential = Credential::new(
            CredentialId(4),
            CredentialDraft::new("运维", CredentialKind::Agent, "deploy"),
        );
        database.insert_credential(&credential).unwrap();
        let mut stale = Host::new(HostId(1), draft("web"));
        stale.credential = Some(credential.id);
        stale.auth = AuthKind::NoPassword;
        database.insert_host(&stale).unwrap();

        let mut store = HostData::load(database).unwrap();
        let host = store.host(HostId(1)).unwrap();
        assert_eq!(host.user.as_ref(), "deploy");
        assert_eq!(host.auth, AuthKind::Password);
        assert_eq!(
            store.credential(CredentialId(4)).unwrap().secret_id,
            credential.secret_id
        );
        assert_eq!(
            store.insert_credential_unnotified(CredentialDraft::new(
                "x",
                CredentialKind::Agent,
                "x"
            )),
            CredentialId(5)
        );
    }

    #[test]
    fn the_detected_system_survives_an_edit() {
        let mut store = HostData::empty();
        let id = store.insert_unnotified(draft("web"));
        assert!(store.set_host_os_unnotified(id, Some(HostOs::Debian)));
        assert!(!store.set_host_os_unnotified(id, Some(HostOs::Debian)));

        store.update_unnotified(id, draft("web-renamed"));
        assert_eq!(store.host(id).unwrap().os, Some(HostOs::Debian));
    }

    #[test]
    fn a_copy_inherits_the_detected_system() {
        let mut store = HostData::empty();
        let id = store.insert_unnotified(draft("web"));
        store.set_host_os_unnotified(id, Some(HostOs::Alpine));

        let copy = store.duplicate_unnotified(id, "copy".into()).unwrap();
        assert_eq!(store.host(copy).unwrap().os, Some(HostOs::Alpine));
    }

    #[test]
    fn probing_an_unknown_host_changes_nothing() {
        let mut store = HostData::empty();
        assert!(!store.set_host_os_unnotified(HostId(99), Some(HostOs::Linux)));
    }

    #[test]
    fn each_host_has_its_own_password() {
        let mut store = HostData::empty();
        let first = store.insert_unnotified(draft("web-01"));
        let second = store.insert_unnotified(draft("web-02"));
        // 同一个 user@地址:端口 的两台主机，密码也各存各的。
        assert_ne!(
            store.host(first).unwrap().password_secret(),
            store.host(second).unwrap().password_secret()
        );
    }

    #[test]
    fn a_host_keeps_its_password_through_edits_and_takes_it_when_deleted() {
        let secrets = Arc::new(InMemorySecretStore::default());
        let mut store = HostData::empty().with_secrets(secrets.clone());
        let id = store.insert(draft("web"));
        let own = store.host(id).unwrap().password_secret();
        secrets.set(&own, "hunter2").unwrap();

        // 改地址、端口、用户名：密码还是这台主机的。
        let moved = HostDraft::new("web-01", "10.0.0.9", 2222, "admin", AuthKind::Password);
        assert!(store.update(id, moved));
        run_secret_jobs(&mut store);
        assert_eq!(store.host(id).unwrap().password_secret(), own);
        assert!(secrets.get(&own).unwrap().is_some());

        // 副本是另一台主机，不带密码。
        let copy = store.duplicate(id, "web-01 副本".into()).unwrap();
        let copied = store.host(copy).unwrap().password_secret();
        assert_ne!(copied, own);
        assert!(secrets.get(&copied).unwrap().is_none());

        assert!(store.remove(id));
        run_secret_jobs(&mut store);
        assert!(secrets.get(&own).unwrap().is_none());
    }

    fn jump_through(hops: &[HostId]) -> Route {
        Route::Jump(hops.iter().copied().map(Some).collect())
    }

    #[test]
    fn a_route_never_jumps_through_its_own_host_or_one_that_is_not_there() {
        let mut store = HostData::empty();
        let jump = store.insert_unnotified(draft("jump"));
        let id = store.insert_unnotified(draft("web"));
        store.update_unnotified(
            id,
            draft("web").with_route(Route::Jump(vec![Some(id), Some(jump), Some(HostId(99))])),
        );
        assert_eq!(
            store.host(id).unwrap().route,
            Route::Jump(vec![Some(jump), None])
        );

        // A jump through nothing is no jump.
        store.update_unnotified(id, draft("web").with_route(Route::Jump(vec![Some(id)])));
        assert_eq!(store.host(id).unwrap().route, Route::Direct);
    }

    #[test]
    fn deleting_a_jump_host_leaves_its_place_and_is_counted() {
        let mut store = HostData::empty();
        let jump = store.insert_unnotified(draft("jump"));
        let inside = store.insert_unnotified(draft("inside").with_route(jump_through(&[jump])));
        let outside = store.insert_unnotified(draft("db").with_route(jump_through(&[jump])));
        let direct = store.insert_unnotified(draft("web"));

        assert_eq!(store.jump_users(&[jump]), 2);
        assert_eq!(store.jumping_through(&[jump]), [inside, outside]);
        // Deleting `jump` with `inside` leaves only `outside` without a way
        // through.
        assert_eq!(store.jump_users(&[jump, inside]), 1);
        assert_eq!(store.jump_users(&[direct]), 0);

        store.remove_unnotified(jump);
        assert_eq!(store.host(outside).unwrap().route, Route::Jump(vec![None]));
        assert_eq!(store.jump_users(&[inside]), 0);
    }

    #[test]
    fn a_copy_takes_the_same_way_there() {
        let mut store = HostData::empty();
        let jump = store.insert_unnotified(draft("jump"));
        let id = store.insert_unnotified(draft("db").with_route(jump_through(&[jump])));
        let copy = store.duplicate_unnotified(id, "copy".into()).unwrap();
        assert_eq!(store.host(copy).unwrap().route, jump_through(&[jump]));
    }

    #[test]
    fn notes_stay_through_an_edit_and_go_with_a_copy() {
        let mut store = HostData::empty();
        let id = store.insert_unnotified(draft("db").with_notes("只读副本"));
        store.update_unnotified(id, draft("db-01").with_notes("只读副本，勿写"));
        assert_eq!(store.host(id).unwrap().notes.as_ref(), "只读副本，勿写");
        let copy = store.duplicate_unnotified(id, "copy".into()).unwrap();
        assert_eq!(store.host(copy).unwrap().notes.as_ref(), "只读副本，勿写");
    }

    #[test]
    fn a_proxy_password_goes_when_no_host_uses_the_proxy() {
        let secrets = Arc::new(InMemorySecretStore::default());
        let mut store = HostData::empty().with_secrets(secrets.clone());
        let proxy =
            Route::Proxy(ProxySettings::new(ProxyKind::Http, "proxy.test", 8080).with_user("me"));
        let one = store.insert_unnotified(draft("one").with_route(proxy.clone()));
        let two = store.insert_unnotified(draft("two").with_route(proxy));
        let secret = SecretRef::proxy("me", "proxy.test", 8080);
        secrets.set(&secret, "hunter2").unwrap();

        store.update(one, draft("one"));
        run_secret_jobs(&mut store);
        assert!(secrets.get(&secret).unwrap().is_some(), "two still uses it");

        store.remove(two);
        run_secret_jobs(&mut store);
        assert!(secrets.get(&secret).unwrap().is_none());
    }

    #[test]
    fn seed_has_six_hosts_in_a_flat_list() {
        let mut store = HostData::seed();
        assert_eq!(
            names(&store),
            [
                "web-01",
                "web-02",
                "db-01",
                "staging-api",
                "qa-runner",
                "dev-box"
            ]
        );
        assert_eq!(store.hosts()[0].id, HostId(1));
        assert_eq!(store.hosts()[5].id, HostId(6));
        assert_eq!(store.insert_unnotified(draft("new")), HostId(7));
        assert_eq!(names(&store).last(), Some(&"new"));
    }

    #[test]
    fn duplicate_places_a_copy_after_the_original() {
        let mut store = HostData::seed();
        let web01 = store.hosts()[0].id;
        let mut draft = store.host(web01).unwrap().draft();
        draft.auth = AuthKind::NoPassword;
        assert!(store.update_unnotified(web01, draft));
        let copy = store
            .duplicate_unnotified(web01, "web-01 副本".into())
            .unwrap();
        assert_eq!(store.hosts()[1].id, copy);
        assert_eq!(store.hosts()[1].name.as_ref(), "web-01 副本");
        assert_eq!(store.hosts()[1].auth, AuthKind::NoPassword);
        // 后面的主机都往后挪了一位，顺序不乱。
        let orders: Vec<_> = store.hosts().iter().map(|host| host.sort_order).collect();
        assert!(
            orders.windows(2).all(|pair| pair[0] < pair[1]),
            "{orders:?}"
        );
    }

    #[test]
    fn dragging_reorders_the_flat_list() {
        let mut store = HostData::seed();
        let [web01, web02, db01] = [HostId(1), HostId(2), HostId(3)];
        assert!(store.move_host_unnotified(db01, HostDrop::Before(web01)));
        assert_eq!(
            names(&store),
            [
                "db-01",
                "web-01",
                "web-02",
                "staging-api",
                "qa-runner",
                "dev-box"
            ]
        );
        // 放回原处、放到自己身上、不存在的主机：都不动。
        assert!(!store.move_host_unnotified(db01, HostDrop::Before(web01)));
        assert!(!store.move_host_unnotified(web02, HostDrop::After(web01)));
        assert!(!store.move_host_unnotified(web01, HostDrop::After(web01)));
        assert!(!store.move_host_unnotified(HostId(99), HostDrop::After(web01)));
        assert!(!store.move_host_unnotified(web01, HostDrop::After(HostId(99))));

        assert!(store.move_host_unnotified(db01, HostDrop::After(HostId(6))));
        assert_eq!(names(&store).last(), Some(&"db-01"));
        // 新建的主机仍然排在最后。
        store.insert_unnotified(draft("new"));
        assert_eq!(names(&store).last(), Some(&"new"));
    }

    #[test]
    fn the_dragged_order_survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qishell.db");
        let mut store = HostData::load(HostDatabase::open(&path).unwrap()).unwrap();
        let web = store.insert(draft("web"));
        let db = store.insert(draft("db"));
        let api = store.insert(draft("api"));
        assert!(store.move_host(api, HostDrop::Before(web)));
        store.duplicate(db, "db 副本".into());
        assert_eq!(names(&store), ["api", "web", "db", "db 副本"]);
        assert!(
            store
                .take_events()
                .iter()
                .all(|event| matches!(event, HostEvent::Changed))
        );

        let reopened = HostData::load(HostDatabase::open(&path).unwrap()).unwrap();
        assert_eq!(names(&reopened), ["api", "web", "db", "db 副本"]);
    }

    #[test]
    fn update_keeps_the_system_found_and_the_place_in_the_list() {
        let mut store = HostData::seed();
        let web01 = store.hosts()[0].id;
        store.set_host_os_unnotified(web01, Some(HostOs::Ubuntu));
        let mut draft = store.host(web01).unwrap().draft();
        draft.port = 2200;
        assert!(store.update_unnotified(web01, draft));
        let host = store.host(web01).unwrap();
        assert_eq!((host.port, host.os), (2200, Some(HostOs::Ubuntu)));
        assert_eq!(store.hosts()[0].id, web01);
        assert!(!store.update_unnotified(HostId(999), store.host(web01).unwrap().draft()));
        assert!(store.remove_unnotified(web01));
        assert!(!store.remove_unnotified(web01));
    }

    #[test]
    fn seed_lists_connected_hosts_as_recent_newest_first() {
        let store = HostData::seed();
        let recent: Vec<_> = store.recent_hosts().map(|s| s.name.as_ref()).collect();
        assert_eq!(recent, ["staging-api", "web-01"]);
    }

    #[test]
    fn connecting_moves_a_host_to_the_front_of_recent() {
        let mut store = HostData::seed();
        let web01 = store.hosts()[0].id;
        let db01 = store.hosts()[2].id;
        assert!(store.record_connection_unnotified(db01));
        assert!(store.record_connection_unnotified(db01));
        assert!(store.record_connection_unnotified(web01));
        assert!(!store.record_connection_unnotified(HostId(99)));
        let recent: Vec<_> = store.recent_hosts().map(|s| s.name.as_ref()).collect();
        assert_eq!(recent, ["web-01", "db-01", "staging-api"]);
    }

    #[test]
    fn remove_drops_the_host_from_recent() {
        let mut store = HostData::seed();
        let web01 = store.hosts()[0].id;
        assert!(store.remove_unnotified(web01));
        let recent: Vec<_> = store.recent_hosts().map(|s| s.name.as_ref()).collect();
        assert_eq!(recent, ["staging-api"]);
    }

    #[test]
    fn recent_keeps_only_the_newest_entries() {
        let mut store = HostData::empty();
        for ix in 0..(MAX_RECENT + 2) {
            let id = store.insert_unnotified(draft(&format!("s{ix}")));
            store.record_connection_unnotified(id);
        }
        assert_eq!(store.recent_hosts().count(), MAX_RECENT);
        let newest = store.recent_hosts().next().unwrap();
        assert_eq!(newest.name.as_ref(), "s11");
    }

    #[test]
    fn load_resumes_the_id_sequences_and_the_recent_order() {
        let database = HostDatabase::in_memory().unwrap();
        database
            .insert_host(&Host::new(HostId(7), draft("web-01")))
            .unwrap();
        database
            .insert_host(&Host::new(HostId(9), draft("db-01")))
            .unwrap();
        database.touch_connected(HostId(9), 100).unwrap();

        let mut store = HostData::load(database).unwrap();
        assert_eq!(
            store.recent_hosts().map(|s| s.id).collect::<Vec<_>>(),
            [HostId(9)]
        );
        assert_eq!(store.insert_unnotified(draft("new")), HostId(10));
    }

    #[test]
    fn a_batch_says_a_repeated_failure_once() {
        let mut store = HostData::load(HostDatabase::in_memory().unwrap()).unwrap();
        assert!(store.begin_batch());
        // Nested: joins the outer batch.
        assert!(!store.begin_batch());
        store.insert(draft("web"));
        // Three hosts using a credential the database never heard of.
        let credential = password_credential(&mut store, "deploy");
        for name in ["a", "b", "c"] {
            store.insert(draft(name).with_credential(credential));
        }
        store.end_batch();
        let failed: Vec<Failure> = store
            .take_events()
            .into_iter()
            .filter_map(|event| match event {
                HostEvent::Failed(failure) => Some(failure),
                _ => None,
            })
            .collect();
        assert_eq!(failed.len(), 1, "{failed:?}");
        assert!(matches!(
            &failed[0],
            Failure::NotSaved {
                change: Change::NewHost,
                ..
            }
        ));
        // The memory changes stand; the events are taken.
        assert_eq!(store.hosts().len(), 4);
        assert!(store.take_events().is_empty());
    }

    #[test]
    fn a_vault_write_waits_for_its_runner_and_says_how_it_went() {
        let mut store = HostData::empty();
        let secret = SecretRef::password(1);
        store.save_secret(secret.clone(), Some("hunter2".into()));
        assert!(store.take_events().is_empty(), "nothing written yet");
        let jobs = store.take_secret_jobs();
        assert_eq!(jobs.len(), 1);
        for job in jobs {
            // A store without a vault keeps nothing.
            let outcome = job.run();
            assert_eq!(
                outcome.failure(),
                Some(Failure::Secret {
                    secret: secret.clone(),
                    writing: true,
                    error: SecretError::Unavailable,
                })
            );
            store.secret_done(outcome);
        }
        assert!(matches!(
            store.take_events().as_slice(),
            [HostEvent::Failed(Failure::Secret { .. })]
        ));
    }

    /// 锁着的保险库：读写都返回 `SecretsLocked`。
    struct LockedVault;

    impl crate::secrets::SecretStore for LockedVault {
        fn get(&self, _: &SecretRef) -> anyhow::Result<Option<Zeroizing<String>>> {
            Err(SecretsLocked.into())
        }

        fn set(&self, _: &SecretRef, _: &str) -> anyhow::Result<()> {
            Err(SecretsLocked.into())
        }

        fn delete(&self, _: &SecretRef) -> anyhow::Result<()> {
            Err(SecretsLocked.into())
        }

        fn is_available(&self) -> bool {
            true
        }
    }

    #[test]
    fn a_locked_vault_says_so_rather_than_failing_otherwise() {
        let mut store = HostData::empty().with_secrets(Arc::new(LockedVault));
        let secret = SecretRef::password(1);
        store.save_secret(secret.clone(), None);
        let outcome = store.take_secret_jobs().pop().unwrap().run();
        assert_eq!(
            outcome.failure(),
            Some(Failure::Secret {
                secret,
                writing: false,
                error: SecretError::Locked,
            })
        );
        // 锁定错误包在别的上下文里也认得出来。
        let wrapped = anyhow::Error::from(SecretsLocked).context("reading web-01's password");
        assert_eq!(SecretError::of(&wrapped), SecretError::Locked);
        assert_eq!(
            SecretError::of(&anyhow::anyhow!("disk full")),
            SecretError::Store("disk full".into())
        );
    }
}
