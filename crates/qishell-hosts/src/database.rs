// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-hosts/src/database.rs
// 裁剪：schema 重写，从 QiShell 自己的 v1 起步，不沿用上游的 v20 和迁移历史（`STEPS`、备份、
// 补 `public_id`/`sync_id`），和 shell-rs 的数据库不兼容；只剩 `credentials`、`hosts`、
// `host_jumps`、`snippets` 四张表，不建分组、SFTP 书签、端口转发、片段分类、项目终端、同步的表；
// 主机去掉协议、串口、Telnet、分组、右侧栏工具、`public_id`、`edited_ms` 列；凭据的 `key_path`
// 换成不算秘密的 `public_key`。读写方法跟着瘦身，树形排序改成 `save_host_order`。

//! The SQLite file behind the host store.
//!
//! Everything here is synchronous and runs on the UI thread: the writes are
//! single rows triggered by a dialog the user just confirmed, not a stream.
//! The store keeps memory as the source of truth and mirrors each change
//! here, so a failed write costs the user persistence, never the edit.

use std::{collections::HashMap, path::Path};

use rusqlite::{Connection, params};

use super::{
    AuthKind, Credential, CredentialDraft, CredentialId, CredentialKind, Host, HostDraft, HostId,
    HostOs, ProxyKind, ProxySettings, Route, SecretId, Snippet, SnippetDraft, SnippetId,
};

/// SQLite's only integer type is `i64`, so the `u64` ids cross the boundary
/// as `i64`. They are counters that start at 1 and never come near the range
/// where the round trip would lose anything.
fn to_sql(id: u64) -> i64 {
    id as i64
}

fn from_sql(id: i64) -> u64 {
    id as u64
}

/// The schema's version, stored in `PRAGMA user_version`.
///
/// QiShell 从 1 起步。以后改表要：升这个版本号，在 `migrate` 里加一步从上一个版本走过来的
/// 迁移（一个事务，最后写新版本号），加测试；新列照 `a_column_added_later_matches_the_schema`
/// 的规矩加在表的最后一列后面。
///
/// - 2（工单 34）：`hosts.probe_os`，登录后探不探测系统。
/// - 3：`hosts.hostname`，上次探测到的 `uname -n`。
pub const SCHEMA_VERSION: i64 = 3;

/// The whole schema, as a new database gets it.
///
/// Every table is `STRICT`: a value of the wrong type is refused rather than
/// stored. The constraints say what the store already keeps true, so a bug
/// or a hand edit that breaks it fails loudly instead of loading as nonsense.
///
/// - **credentials** come before the hosts that refer to them. Secrets are
///   never here: a credential's password, private key and passphrase are in
///   the vault under `secret_id`. Only a key credential has a `public_key`,
///   which is not a secret.
/// - **hosts** form one flat list in `sort_order`, and carry how they log in
///   and how they are reached:
///   - `auth` is `password` or `no-password` for a login of their own, with
///     `username`, or `credential`, with `credential_id` and no user name of
///     their own (the credential's is used). Deleting a credential that is
///     still used is refused: the store first gives its hosts a login of
///     their own.
///   - `route` is `direct`, `jump` (the hops are in `host_jumps`) or
///     `proxy`, whose four columns are set exactly for a proxy route.
///   - `os` and `last_connected_at` are written by connections, not by the
///     host form.
///   - `probe_os` 是登录后探不探测系统：主机表单写它，探测把连接弄断时连接也写它。
///   - `hostname` 是上次探测到的 `uname -n`，连接写它；关了探测时用来核对远程报告的目录。
/// - **host_jumps** lists a host's jump hosts in order. A deleted jump
///   host leaves its row behind with no `jump_id`, so the host fails to
///   connect rather than skip the hop.
/// - **snippets** are shared by every host. `run_on_click` runs one on a
///   click rather than only typing it.
const SCHEMA: &str = "\
CREATE TABLE credentials (
    id         INTEGER PRIMARY KEY,
    secret_id  TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL CHECK (name <> ''),
    kind       TEXT NOT NULL CHECK (kind IN ('password', 'key', 'agent')),
    username   TEXT NOT NULL,
    public_key TEXT CHECK (public_key <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0,
    CHECK (kind = 'key' OR public_key IS NULL)
) STRICT;
CREATE TABLE hosts (
    id                INTEGER PRIMARY KEY,
    sort_order        INTEGER NOT NULL DEFAULT 0,
    name              TEXT NOT NULL CHECK (name <> ''),
    address           TEXT NOT NULL CHECK (address <> ''),
    port              INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    auth              TEXT NOT NULL CHECK (auth IN ('password', 'no-password', 'credential')),
    username          TEXT,
    credential_id     INTEGER REFERENCES credentials(id),
    route             TEXT NOT NULL DEFAULT 'direct' CHECK (route IN ('direct', 'jump', 'proxy')),
    proxy_kind        TEXT CHECK (proxy_kind IN ('http', 'socks5')),
    proxy_host        TEXT,
    proxy_port        INTEGER CHECK (proxy_port BETWEEN 1 AND 65535),
    proxy_username    TEXT,
    notes             TEXT NOT NULL DEFAULT '',
    os                TEXT,
    last_connected_at INTEGER,
    probe_os          INTEGER NOT NULL DEFAULT 1 CHECK (probe_os IN (0, 1)),
    hostname          TEXT CHECK (hostname <> ''),
    CHECK ((auth = 'credential') = (credential_id IS NOT NULL)),
    CHECK ((auth = 'credential') = (username IS NULL)),
    CHECK ((route = 'proxy') = (proxy_kind IS NOT NULL)),
    CHECK ((proxy_kind IS NULL) = (proxy_host IS NULL)
        AND (proxy_kind IS NULL) = (proxy_port IS NULL)),
    CHECK (proxy_kind IS NOT NULL OR proxy_username IS NULL)
) STRICT;
CREATE INDEX hosts_credential_id ON hosts(credential_id);
CREATE TABLE host_jumps (
    host_id  INTEGER NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    jump_id  INTEGER REFERENCES hosts(id) ON DELETE SET NULL,
    PRIMARY KEY (host_id, position),
    CHECK (jump_id <> host_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX host_jumps_jump_id ON host_jumps(jump_id);
CREATE TABLE snippets (
    id           INTEGER PRIMARY KEY,
    name         TEXT NOT NULL CHECK (name <> ''),
    command      TEXT NOT NULL CHECK (command <> ''),
    run_on_click INTEGER NOT NULL DEFAULT 0 CHECK (run_on_click IN (0, 1))
) STRICT;";

/// Everything one launch reads back from disk.
pub struct StoredData {
    /// Hosts in list order (`sort_order`, then id). A host using a
    /// credential has no user name of its own here; the store copies the
    /// credential's in.
    pub hosts: Vec<Host>,
    /// Hosts that have ever connected, most recently connected first, with
    /// when they last did (Unix seconds).
    pub recent: Vec<(HostId, u64)>,
    /// Credentials in the order the credential list shows them.
    pub credentials: Vec<Credential>,
    /// Snippets in id order.
    pub snippets: Vec<Snippet>,
}

pub struct HostDatabase {
    connection: Connection,
}

/// Why the database did not open. `Newer` leaves the file as it is.
#[derive(Debug)]
pub enum OpenError {
    /// A newer QiShell wrote it, at schema `version`; this one knows up to
    /// `supported`.
    Newer {
        version: i64,
        supported: i64,
    },
    Sqlite(rusqlite::Error),
}

impl From<rusqlite::Error> for OpenError {
    fn from(error: rusqlite::Error) -> Self {
        OpenError::Sqlite(error)
    }
}

/// For logs; the app words it for the user.
impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Newer { version, supported } => write!(
                f,
                "the database is from a newer QiShell (schema {version}, this one knows up to {supported})"
            ),
            OpenError::Sqlite(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OpenError::Sqlite(error) => Some(error),
            OpenError::Newer { .. } => None,
        }
    }
}

impl HostDatabase {
    /// Open the database at `path`, creating it if it is not there yet.
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        Self::prepare(Connection::open(path)?)
    }

    /// An anonymous database that lives only as long as this value.
    #[cfg(any(test, feature = "testing"))]
    pub fn in_memory() -> Result<Self, OpenError> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(connection: Connection) -> Result<Self, OpenError> {
        // Cascading deletes are how removing a host removes its jumps, and
        // SQLite only honours the foreign keys when asked, per connection.
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.execute_batch("PRAGMA journal_mode = WAL;")?;
        migrate(&connection)?;
        Ok(Self { connection })
    }

    /// Start a batch: every write until [`Self::end_batch`] lands in one
    /// transaction, so a burst of them pays for one commit rather than one
    /// each.
    pub fn begin_batch(&self) -> rusqlite::Result<()> {
        self.connection.execute_batch("BEGIN")
    }

    /// Commit the batch. A commit that fails rolls it back, so the
    /// connection is back to committing each write on its own.
    pub fn end_batch(&self) -> rusqlite::Result<()> {
        self.connection.execute_batch("COMMIT").inspect_err(|_| {
            let _ = self.connection.execute_batch("ROLLBACK");
        })
    }

    /// One write that lands whole or not at all: in a transaction of its
    /// own, or, inside a batch, in a savepoint, which costs no commit.
    fn atomically<T>(
        &self,
        write: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        if self.connection.is_autocommit() {
            let transaction = self.connection.unchecked_transaction()?;
            let value = write(&transaction)?;
            transaction.commit()?;
            return Ok(value);
        }
        self.connection.execute_batch("SAVEPOINT write")?;
        match write(&self.connection) {
            Ok(value) => {
                self.connection.execute_batch("RELEASE write")?;
                Ok(value)
            }
            Err(error) => {
                let _ = self
                    .connection
                    .execute_batch("ROLLBACK TO write; RELEASE write");
                Err(error)
            }
        }
    }

    pub fn load(&self) -> rusqlite::Result<StoredData> {
        let mut hosts = self
            .connection
            .prepare(
                "SELECT id, sort_order, name, address, port, auth, username, credential_id, \
                 route, proxy_kind, proxy_host, proxy_port, proxy_username, notes, os, \
                 probe_os, hostname FROM hosts ORDER BY sort_order, id",
            )?
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let name: String = row.get(2)?;
                let address: String = row.get(3)?;
                let port: u16 = row.get(4)?;
                let auth: String = row.get(5)?;
                let user: Option<String> = row.get(6)?;
                let credential: Option<i64> = row.get(7)?;
                let route: String = row.get(8)?;
                let proxy_kind: Option<String> = row.get(9)?;
                let proxy_host: Option<String> = row.get(10)?;
                let proxy_port: Option<u16> = row.get(11)?;
                let proxy_user: Option<String> = row.get(12)?;
                let notes: String = row.get(13)?;
                let os: Option<String> = row.get(14)?;
                let probe_os: bool = row.get(15)?;
                let hostname: Option<String> = row.get(16)?;
                let mut host = Host::new(
                    HostId(from_sql(id)),
                    HostDraft::new(
                        name,
                        address,
                        port,
                        user.unwrap_or_default(),
                        AuthKind::from_stored(&auth),
                    )
                    .with_notes(notes)
                    .with_probe_os(probe_os),
                );
                host.sort_order = row.get(1)?;
                host.credential = credential.map(|id| CredentialId(from_sql(id)));
                // The table's CHECKs keep the proxy's columns whole.
                host.route = match (route.as_str(), proxy_kind, proxy_host, proxy_port) {
                    ("proxy", Some(kind), Some(proxy_host), Some(port)) => {
                        ProxyKind::from_stored(&kind)
                            .map(|kind| {
                                Route::Proxy(
                                    ProxySettings::new(kind, proxy_host, port)
                                        .with_user(proxy_user.unwrap_or_default()),
                                )
                            })
                            .unwrap_or_default()
                    }
                    ("jump", ..) => Route::Jump(Vec::new()),
                    _ => Route::Direct,
                };
                host.os = os.as_deref().and_then(HostOs::from_stored);
                host.hostname = hostname.map(Into::into);
                Ok(host)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.load_jumps(&mut hosts)?;

        let recent = self
            .connection
            .prepare(
                "SELECT id, last_connected_at FROM hosts WHERE last_connected_at IS NOT NULL \
                 ORDER BY last_connected_at DESC, id DESC",
            )?
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let at: i64 = row.get(1)?;
                Ok((HostId(from_sql(id)), from_sql(at)))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let credentials = self
            .connection
            .prepare(
                "SELECT id, secret_id, name, kind, username, public_key, sort_order \
                 FROM credentials ORDER BY sort_order, id",
            )?
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let secret_id: String = row.get(1)?;
                let name: String = row.get(2)?;
                let kind: String = row.get(3)?;
                let user: String = row.get(4)?;
                let public_key: Option<String> = row.get(5)?;
                let sort_order: i64 = row.get(6)?;
                // The table's CHECK admits only the kinds this build knows.
                Ok(CredentialKind::from_stored(&kind).map(|kind| {
                    let mut draft = CredentialDraft::new(name, kind, user);
                    draft.public_key = public_key.map(Into::into);
                    let mut credential = Credential::new(CredentialId(from_sql(id)), draft);
                    credential.secret_id = SecretId::from_stored(secret_id);
                    credential.sort_order = sort_order;
                    credential
                }))
            })?
            .filter_map(Result::transpose)
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let snippets = self
            .connection
            .prepare("SELECT id, name, command, run_on_click FROM snippets ORDER BY id")?
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                let command: String = row.get(2)?;
                let run_on_click: bool = row.get(3)?;
                Ok(Snippet::new(
                    SnippetId(from_sql(id)),
                    SnippetDraft::new(name, command).with_run_on_click(run_on_click),
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(StoredData {
            hosts,
            recent,
            credentials,
            snippets,
        })
    }

    /// Give each jump route among `hosts` its hops, in order.
    fn load_jumps(&self, hosts: &mut [Host]) -> rusqlite::Result<()> {
        let index: HashMap<HostId, usize> = hosts
            .iter()
            .enumerate()
            .map(|(ix, host)| (host.id, ix))
            .collect();
        let hops = self
            .connection
            .prepare("SELECT host_id, jump_id FROM host_jumps ORDER BY host_id, position")?
            .query_map([], |row| {
                let host: i64 = row.get(0)?;
                let jump: Option<i64> = row.get(1)?;
                Ok((HostId(from_sql(host)), jump.map(|id| HostId(from_sql(id)))))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (host, hop) in hops {
            if let Some(Route::Jump(hops)) = index.get(&host).map(|ix| &mut hosts[*ix].route) {
                hops.push(hop);
            }
        }
        Ok(())
    }

    /// Insert a host together with its jump hosts.
    pub fn insert_host(&self, host: &Host) -> rusqlite::Result<()> {
        self.atomically(|connection| {
            let row = HostRow::of(host);
            connection.execute(
                "INSERT INTO hosts (id, sort_order, name, address, port, auth, username, \
                 credential_id, route, proxy_kind, proxy_host, proxy_port, proxy_username, \
                 notes, os, probe_os, hostname) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, \
                 ?17)",
                params![
                    to_sql(host.id.0),
                    host.sort_order,
                    host.name.as_ref(),
                    host.address.as_ref(),
                    host.port,
                    row.auth,
                    row.username,
                    row.credential,
                    row.route,
                    row.proxy_kind,
                    row.proxy_host,
                    row.proxy_port,
                    row.proxy_username,
                    host.notes.as_ref(),
                    host.os.map(HostOs::as_str),
                    host.probe_os,
                    host.hostname.as_deref(),
                ],
            )?;
            write_jumps(connection, host)
        })
    }

    /// Rewrite the fields the host form edits, jump hosts included.
    /// `last_connected_at` and `os` are left alone: they are written by
    /// `touch_connected` and `set_host_os`, and neither is part of the host
    /// form. `hostname` 按主机数据给的写：地址、端口、路线变了时主机数据已经把它清掉了。
    pub fn update_host(&self, host: &Host) -> rusqlite::Result<()> {
        self.atomically(|connection| {
            let row = HostRow::of(host);
            connection.execute(
                "UPDATE hosts SET sort_order = ?2, name = ?3, address = ?4, port = ?5, \
                 auth = ?6, username = ?7, credential_id = ?8, route = ?9, proxy_kind = ?10, \
                 proxy_host = ?11, proxy_port = ?12, proxy_username = ?13, notes = ?14, \
                 probe_os = ?15, hostname = ?16 WHERE id = ?1",
                params![
                    to_sql(host.id.0),
                    host.sort_order,
                    host.name.as_ref(),
                    host.address.as_ref(),
                    host.port,
                    row.auth,
                    row.username,
                    row.credential,
                    row.route,
                    row.proxy_kind,
                    row.proxy_host,
                    row.proxy_port,
                    row.proxy_username,
                    host.notes.as_ref(),
                    host.probe_os,
                    host.hostname.as_deref(),
                ],
            )?;
            write_jumps(connection, host)
        })
    }

    /// Deleting a host takes its jump hosts with it; the hosts that went
    /// through it keep its place, empty (`ON DELETE SET NULL`).
    pub fn remove_host(&self, id: HostId) -> rusqlite::Result<()> {
        self.connection
            .execute("DELETE FROM hosts WHERE id = ?1", params![to_sql(id.0)])?;
        Ok(())
    }

    /// 把一次拖动（或插入副本）后整张列表的顺序作为一个整体写下去。
    pub fn save_host_order(&self, hosts: &[Host]) -> rusqlite::Result<()> {
        self.atomically(|connection| {
            let mut update =
                connection.prepare("UPDATE hosts SET sort_order = ?2 WHERE id = ?1")?;
            for host in hosts {
                update.execute(params![to_sql(host.id.0), host.sort_order])?;
            }
            Ok(())
        })
    }

    pub fn insert_credential(&self, credential: &Credential) -> rusqlite::Result<()> {
        self.connection.execute(
            "INSERT INTO credentials (id, secret_id, name, kind, username, public_key, \
             sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                to_sql(credential.id.0),
                credential.secret_id.as_str(),
                credential.name.as_ref(),
                credential.kind.as_str(),
                credential.user.as_ref(),
                credential.public_key.as_deref(),
                credential.sort_order,
            ],
        )?;
        Ok(())
    }

    /// Rewrite a credential. The hosts using it have no user name of
    /// their own here, so they follow without being touched.
    pub fn update_credential(&self, credential: &Credential) -> rusqlite::Result<()> {
        self.connection.execute(
            "UPDATE credentials SET name = ?2, kind = ?3, username = ?4, public_key = ?5, \
             sort_order = ?6 WHERE id = ?1",
            params![
                to_sql(credential.id.0),
                credential.name.as_ref(),
                credential.kind.as_str(),
                credential.user.as_ref(),
                credential.public_key.as_deref(),
                credential.sort_order,
            ],
        )?;
        Ok(())
    }

    /// Delete a credential. The hosts using it go back to logging in on
    /// their own with `auth`, as the credential's user, first: a credential
    /// still in use cannot be deleted.
    pub fn remove_credential(&self, id: CredentialId, auth: AuthKind) -> rusqlite::Result<()> {
        self.atomically(|connection| {
            connection.execute(
                "UPDATE hosts SET auth = ?2, credential_id = NULL, \
                 username = (SELECT username FROM credentials WHERE id = ?1) \
                 WHERE credential_id = ?1",
                params![to_sql(id.0), auth.as_str()],
            )?;
            connection.execute(
                "DELETE FROM credentials WHERE id = ?1",
                params![to_sql(id.0)],
            )?;
            Ok(())
        })
    }

    pub fn insert_snippet(&self, snippet: &Snippet) -> rusqlite::Result<()> {
        self.connection.execute(
            "INSERT INTO snippets (id, name, command, run_on_click) VALUES (?1, ?2, ?3, ?4)",
            params![
                to_sql(snippet.id.0),
                snippet.name.as_ref(),
                snippet.command,
                snippet.run_on_click,
            ],
        )?;
        Ok(())
    }

    pub fn update_snippet(&self, snippet: &Snippet) -> rusqlite::Result<()> {
        self.connection.execute(
            "UPDATE snippets SET name = ?2, command = ?3, run_on_click = ?4 WHERE id = ?1",
            params![
                to_sql(snippet.id.0),
                snippet.name.as_ref(),
                snippet.command,
                snippet.run_on_click,
            ],
        )?;
        Ok(())
    }

    pub fn remove_snippet(&self, id: SnippetId) -> rusqlite::Result<()> {
        self.connection
            .execute("DELETE FROM snippets WHERE id = ?1", params![to_sql(id.0)])?;
        Ok(())
    }

    /// Record the operating system a probe found on the host. `None` clears
    /// it, which is what a failed probe on a rebuilt host leaves behind.
    pub fn set_host_os(&self, id: HostId, os: Option<HostOs>) -> rusqlite::Result<()> {
        self.connection.execute(
            "UPDATE hosts SET os = ?2 WHERE id = ?1",
            params![to_sql(id.0), os.map(HostOs::as_str)],
        )?;
        Ok(())
    }

    /// 记下探测到的 `uname -n`；`None` 是忘掉。
    pub fn set_host_hostname(&self, id: HostId, hostname: Option<&str>) -> rusqlite::Result<()> {
        self.connection.execute(
            "UPDATE hosts SET hostname = ?2 WHERE id = ?1",
            params![to_sql(id.0), hostname],
        )?;
        Ok(())
    }

    /// 记下登录后探不探测系统（探测把连接弄断时自动关掉，工单 34）。
    pub fn set_host_probe(&self, id: HostId, probe_os: bool) -> rusqlite::Result<()> {
        self.connection.execute(
            "UPDATE hosts SET probe_os = ?2 WHERE id = ?1",
            params![to_sql(id.0), probe_os],
        )?;
        Ok(())
    }

    /// Record that a host just connected, which is what orders the start
    /// card's recent list across launches.
    pub fn touch_connected(&self, id: HostId, at: u64) -> rusqlite::Result<()> {
        self.connection.execute(
            "UPDATE hosts SET last_connected_at = ?2 WHERE id = ?1",
            params![to_sql(id.0), to_sql(at)],
        )?;
        Ok(())
    }
}

/// The columns of a host's row that are spelled differently from the
/// model: its login and its route.
struct HostRow<'a> {
    auth: &'static str,
    username: Option<&'a str>,
    credential: Option<i64>,
    route: &'static str,
    proxy_kind: Option<&'static str>,
    proxy_host: Option<&'a str>,
    proxy_port: Option<u16>,
    proxy_username: Option<&'a str>,
}

impl<'a> HostRow<'a> {
    fn of(host: &'a Host) -> Self {
        let proxy = match &host.route {
            Route::Proxy(proxy) => Some(proxy),
            _ => None,
        };
        Self {
            auth: match host.credential {
                Some(_) => "credential",
                None => host.auth.as_str(),
            },
            // A host using a credential logs in as the credential's
            // user; the copy the store keeps in memory is not stored.
            username: match host.credential {
                Some(_) => None,
                None => Some(host.user.as_ref()),
            },
            credential: host.credential.map(|id| to_sql(id.0)),
            route: match host.route {
                Route::Direct => "direct",
                Route::Jump(_) => "jump",
                Route::Proxy(_) => "proxy",
            },
            proxy_kind: proxy.map(|proxy| proxy.kind.as_str()),
            proxy_host: proxy.map(|proxy| proxy.host.as_ref()),
            proxy_port: proxy.map(|proxy| proxy.port),
            proxy_username: proxy.and_then(|proxy| proxy.user.as_deref()),
        }
    }
}

/// Record `host`'s jump hosts, in place of whatever was recorded.
fn write_jumps(connection: &Connection, host: &Host) -> rusqlite::Result<()> {
    let id = to_sql(host.id.0);
    connection.execute("DELETE FROM host_jumps WHERE host_id = ?1", params![id])?;
    if let Route::Jump(hops) = &host.route {
        let mut insert = connection
            .prepare("INSERT INTO host_jumps (host_id, position, jump_id) VALUES (?1, ?2, ?3)")?;
        for (position, hop) in (0_i64..).zip(hops) {
            insert.execute(params![id, position, hop.map(|hop| to_sql(hop.0))])?;
        }
    }
    Ok(())
}

/// Bring the database to `SCHEMA_VERSION`. A new file has version 0 and gets
/// the whole schema in one transaction.
///
/// A file from a newer QiShell is refused and left as it is: this version
/// does not know what the newer tables mean, and writing to them could lose
/// what the newer version saved. That happens after going back to an older
/// version by hand, the one way an update can be undone.
///
/// 旧文件一个版本一个版本地走上来，每一步一个事务，最后写新的版本号。
fn migrate(connection: &Connection) -> Result<(), OpenError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 0 {
        connection.execute_batch(&format!(
            "BEGIN;\n{SCHEMA}\nPRAGMA user_version = {SCHEMA_VERSION};\nCOMMIT;"
        ))?;
        return Ok(());
    }
    if version > SCHEMA_VERSION {
        return Err(OpenError::Newer {
            version,
            supported: SCHEMA_VERSION,
        });
    }
    if version < 2 {
        // 工单 34：登录后探不探测系统，升级前的主机照旧探测。
        connection.execute_batch(
            "BEGIN;
             ALTER TABLE hosts ADD COLUMN
                 probe_os INTEGER NOT NULL DEFAULT 1 CHECK (probe_os IN (0, 1));
             PRAGMA user_version = 2;
             COMMIT;",
        )?;
    }
    if version < 3 {
        // 上次探测到的 `uname -n`：升级前的主机还没记过。
        connection.execute_batch(
            "BEGIN;
             ALTER TABLE hosts ADD COLUMN hostname TEXT CHECK (hostname <> '');
             PRAGMA user_version = 3;
             COMMIT;",
        )?;
    }
    Ok(())
}

/// Seconds since the Unix epoch, for `touch_connected`.
pub fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(id: u64, name: &str) -> Host {
        Host::new(
            HostId(id),
            HostDraft::new(name, "10.0.0.1", 22, "root", AuthKind::Password),
        )
    }

    fn version_of(connection: &Connection) -> i64 {
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    fn tables_of(connection: &Connection) -> Vec<String> {
        connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    #[test]
    fn a_fresh_database_is_empty_and_at_the_current_schema() {
        let db = HostDatabase::in_memory().unwrap();
        assert_eq!(SCHEMA_VERSION, 3);
        assert_eq!(version_of(&db.connection), SCHEMA_VERSION);
        let data = db.load().unwrap();
        assert!(data.hosts.is_empty());
        assert!(data.recent.is_empty());
        assert!(data.credentials.is_empty());
        assert!(data.snippets.is_empty());
    }

    #[test]
    fn schema_one_has_only_strict_tables_and_no_bookmarks() {
        let db = HostDatabase::in_memory().unwrap();
        assert_eq!(
            tables_of(&db.connection),
            ["credentials", "host_jumps", "hosts", "snippets"]
        );
        let strict: Vec<(String, bool)> = db
            .connection
            .prepare("SELECT name, strict FROM pragma_table_list WHERE schema = 'main' AND name NOT LIKE 'sqlite_%'")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(strict.len(), 4, "{strict:?}");
        assert!(strict.iter().all(|(_, strict)| *strict), "{strict:?}");
    }

    #[test]
    fn a_database_from_a_newer_qishell_is_refused_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qishell.db");
        drop(HostDatabase::open(&path).unwrap());
        let newer = SCHEMA_VERSION + 1;
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", newer)
            .unwrap();

        let Err(error) = HostDatabase::open(&path) else {
            panic!("a newer database opened");
        };
        assert!(
            matches!(error, OpenError::Newer { version, supported: SCHEMA_VERSION } if version == newer),
            "{error}"
        );
        assert_eq!(version_of(&Connection::open(&path).unwrap()), newer);
    }

    /// Every table and index as SQLite recorded its definition, with the
    /// layout of the text taken out: a column added with `ALTER TABLE ADD
    /// COLUMN` is spelled `x, y …` where the schema writes `x,\n    y …`.
    fn schema_of(connection: &Connection) -> Vec<(String, String)> {
        connection
            .prepare("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, normalized_sql(&row.get::<_, String>(1)?)))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    fn normalized_sql(sql: &str) -> String {
        sql.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace(" ,", ",")
            .replace("( ", "(")
            .replace(" )", ")")
    }

    #[test]
    fn a_column_added_later_matches_the_schema() {
        // What a later version does to a file it finds: the new column goes
        // where the schema puts it, after the last column, before the
        // table's own constraints.
        let old = Connection::open_in_memory().unwrap();
        old.execute_batch(
            "CREATE TABLE t (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    CHECK (name <> '')
) STRICT;
ALTER TABLE t ADD COLUMN notes TEXT NOT NULL DEFAULT '';",
        )
        .unwrap();
        let new = Connection::open_in_memory().unwrap();
        new.execute_batch(
            "CREATE TABLE t (
    id    INTEGER PRIMARY KEY,
    name  TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    CHECK (name <> '')
) STRICT;",
        )
        .unwrap();
        assert_eq!(schema_of(&old), schema_of(&new));
    }

    fn snippet(id: u64, name: &str) -> Snippet {
        Snippet::new(
            SnippetId(id),
            SnippetDraft::new(name, format!("echo {name}")),
        )
    }

    #[test]
    fn snippets_round_trip() {
        let db = HostDatabase::in_memory().unwrap();
        let mut ps = snippet(1, "列出容器");
        ps.command = "docker ps -a\ndocker images".into();
        db.insert_snippet(&ps).unwrap();
        db.insert_snippet(&snippet(2, "磁盘")).unwrap();
        let data = db.load().unwrap();
        assert_eq!(data.snippets[0], ps);

        ps.name = "所有容器".into();
        ps.run_on_click = true;
        db.update_snippet(&ps).unwrap();
        assert_eq!(db.load().unwrap().snippets[0], ps);

        db.remove_snippet(SnippetId(2)).unwrap();
        assert_eq!(db.load().unwrap().snippets, [ps]);
        // 片段不跟着主机走。
        db.insert_host(&host(1, "web")).unwrap();
        db.remove_host(HostId(1)).unwrap();
        assert_eq!(db.load().unwrap().snippets.len(), 1);
    }

    #[test]
    fn notes_round_trip() {
        let db = HostDatabase::in_memory().unwrap();
        let mut noted = host(1, "web");
        noted.notes = "机房 A，到期 2027-03\n负责人：张三".into();
        db.insert_host(&noted).unwrap();
        db.insert_host(&host(2, "db")).unwrap();
        let notes = |db: &HostDatabase| -> Vec<String> {
            db.load()
                .unwrap()
                .hosts
                .into_iter()
                .map(|host| host.notes.to_string())
                .collect()
        };
        assert_eq!(
            notes(&db),
            [
                "机房 A，到期 2027-03\n负责人：张三".to_string(),
                String::new()
            ]
        );

        db.update_host(&host(1, "web")).unwrap();
        assert_eq!(notes(&db), [String::new(), String::new()]);
    }

    fn routed(mut host: Host, route: Route) -> Host {
        host.route = route;
        host
    }

    #[test]
    fn routes_round_trip_and_a_deleted_jump_host_leaves_its_place() {
        let db = HostDatabase::in_memory().unwrap();
        let proxy =
            Route::Proxy(ProxySettings::new(ProxyKind::Socks5, "127.0.0.1", 7890).with_user("me"));
        let hosts = [
            host(1, "阿里云99"),
            host(2, "禅道"),
            routed(
                host(3, "db"),
                Route::Jump(vec![Some(HostId(1)), Some(HostId(2))]),
            ),
            routed(host(4, "abroad"), proxy.clone()),
            routed(
                host(5, "http"),
                Route::Proxy(ProxySettings::new(ProxyKind::Http, "proxy.test", 8080)),
            ),
        ];
        for host in &hosts {
            db.insert_host(host).unwrap();
        }
        let routes = |db: &HostDatabase| -> Vec<Route> {
            db.load()
                .unwrap()
                .hosts
                .into_iter()
                .map(|host| host.route)
                .collect()
        };
        assert_eq!(
            routes(&db),
            hosts.iter().map(|s| s.route.clone()).collect::<Vec<_>>()
        );

        // Updating rewrites the route, and the order of the hops with it.
        db.update_host(&routed(
            host(3, "db"),
            Route::Jump(vec![Some(HostId(2)), Some(HostId(1))]),
        ))
        .unwrap();
        db.update_host(&host(4, "abroad")).unwrap();
        assert_eq!(
            routes(&db)[2..4],
            [
                Route::Jump(vec![Some(HostId(2)), Some(HostId(1))]),
                Route::Direct
            ]
        );

        // The jump host goes; its place stays, empty. Its own route goes
        // with a host that is deleted.
        db.remove_host(HostId(1)).unwrap();
        db.remove_host(HostId(5)).unwrap();
        assert_eq!(
            routes(&db),
            [
                Route::Direct,
                Route::Jump(vec![Some(HostId(2)), None]),
                Route::Direct
            ]
        );
        // A route that is no longer a proxy keeps none of the proxy's columns.
        let proxies: i64 = db
            .connection
            .query_row(
                "SELECT COUNT(*) FROM hosts WHERE proxy_host IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(proxies, 0);
    }

    #[test]
    fn the_database_refuses_a_host_that_jumps_through_itself() {
        let db = HostDatabase::in_memory().unwrap();
        let looped = routed(host(1, "web"), Route::Jump(vec![Some(HostId(1))]));
        assert!(db.insert_host(&looped).is_err());
        // The transaction took the host with it.
        assert!(db.load().unwrap().hosts.is_empty());
    }

    fn credential(id: u64, name: &str, kind: CredentialKind) -> Credential {
        let draft = CredentialDraft::new(name, kind, "deploy");
        let draft = match kind {
            CredentialKind::Key => draft.with_public_key("ssh-ed25519 AAAAC3Nza deploy@web"),
            _ => draft,
        };
        Credential::new(CredentialId(id), draft)
    }

    fn using(mut host: Host, credential: &Credential) -> Host {
        host.credential = Some(credential.id);
        host.user = credential.user.clone();
        host.auth = AuthKind::Password;
        host
    }

    #[test]
    fn credentials_round_trip_and_hosts_keep_their_link() {
        let db = HostDatabase::in_memory().unwrap();
        let mut agent = credential(1, "agent", CredentialKind::Agent);
        agent.sort_order = 1;
        db.insert_credential(&agent).unwrap();
        let key = credential(2, "部署", CredentialKind::Key);
        db.insert_credential(&key).unwrap();
        let web = using(host(1, "web"), &key);
        db.insert_host(&web).unwrap();
        db.insert_host(&host(2, "db")).unwrap();

        let data = db.load().unwrap();
        // `sort_order` first, then id.
        assert_eq!(data.credentials, [key.clone(), agent.clone()]);
        assert_eq!(data.hosts[0].credential, Some(key.id));
        assert_eq!(data.hosts[1].credential, None);

        // Moving a host to another credential replaces its link; leaving it
        // takes the link away.
        db.update_host(&using(web.clone(), &agent)).unwrap();
        assert_eq!(db.load().unwrap().hosts[0].credential, Some(agent.id));
        let mut manual = web.clone();
        manual.credential = None;
        db.update_host(&manual).unwrap();
        assert_eq!(db.load().unwrap().hosts[0].credential, None);

        // A host using a credential keeps no user name of its own: the
        // credential's is the only copy on disk.
        db.update_host(&web).unwrap();
        let mut renamed = key.clone();
        renamed.name = "生产部署".into();
        renamed.user = "admin".into();
        db.update_credential(&renamed).unwrap();
        let data = db.load().unwrap();
        assert_eq!(data.credentials[0], renamed);
        assert_eq!(data.hosts[0].user.as_ref(), "");
        assert_eq!(data.hosts[1].user.as_ref(), "root");

        // A credential still in use cannot simply go.
        assert!(
            db.connection
                .execute("DELETE FROM credentials WHERE id = ?1", [to_sql(key.id.0)])
                .is_err()
        );
    }

    #[test]
    fn removing_a_credential_leaves_its_hosts_on_their_own_and_drops_the_link() {
        let db = HostDatabase::in_memory().unwrap();
        let key = credential(1, "部署", CredentialKind::Key);
        db.insert_credential(&key).unwrap();
        db.insert_host(&using(host(1, "web"), &key)).unwrap();
        db.insert_host(&host(2, "db")).unwrap();

        db.remove_credential(key.id, AuthKind::NoPassword).unwrap();
        let data = db.load().unwrap();
        assert!(data.credentials.is_empty());
        assert_eq!(data.hosts[0].credential, None);
        assert_eq!(data.hosts[0].auth, AuthKind::NoPassword);
        assert_eq!(data.hosts[0].user.as_ref(), "deploy");
        // A host that never used it is untouched.
        assert_eq!(data.hosts[1].auth, AuthKind::Password);
    }

    #[test]
    fn the_database_refuses_a_public_key_that_does_not_fit_the_kind() {
        let db = HostDatabase::in_memory().unwrap();
        // 读不出公钥的密钥凭据照样能存。
        let mut key_without_public_half = credential(1, "部署", CredentialKind::Key);
        key_without_public_half.public_key = None;
        assert!(db.insert_credential(&key_without_public_half).is_ok());

        let mut password_with_public_key = credential(2, "运维", CredentialKind::Password);
        password_with_public_key.public_key = Some("ssh-ed25519 AAAAC3Nza".into());
        assert!(db.insert_credential(&password_with_public_key).is_err());

        // A host cannot use a credential that is not there.
        let mut orphan = host(1, "web");
        orphan.credential = Some(CredentialId(99));
        assert!(db.insert_host(&orphan).is_err());
        // And the failed insert left no half of the host behind.
        assert!(db.load().unwrap().hosts.is_empty());
    }

    #[test]
    fn updating_rewrites_the_editable_fields() {
        let db = HostDatabase::in_memory().unwrap();
        db.insert_host(&host(1, "web-01")).unwrap();

        let mut edited = host(1, "web-01");
        edited.port = 2222;
        edited.auth = AuthKind::NoPassword;
        db.update_host(&edited).unwrap();

        let data = db.load().unwrap();
        assert_eq!(data.hosts[0].port, 2222);
        assert_eq!(data.hosts[0].auth, AuthKind::NoPassword);
    }

    #[test]
    fn hosts_load_in_list_order() {
        let db = HostDatabase::in_memory().unwrap();
        for (id, name) in [(1, "web"), (2, "db"), (3, "api")] {
            db.insert_host(&host(id, name)).unwrap();
        }
        let mut hosts = db.load().unwrap().hosts;
        for (host, order) in hosts.iter_mut().zip([2, 1, 0]) {
            host.sort_order = order;
        }
        db.save_host_order(&hosts).unwrap();
        let names: Vec<_> = db
            .load()
            .unwrap()
            .hosts
            .iter()
            .map(|host| host.name.to_string())
            .collect();
        assert_eq!(names, ["api", "db", "web"]);
    }

    #[test]
    fn recent_lists_the_most_recently_connected_first() {
        let db = HostDatabase::in_memory().unwrap();
        for id in 1..=3 {
            db.insert_host(&host(id, &format!("s{id}"))).unwrap();
        }
        db.touch_connected(HostId(1), 100).unwrap();
        db.touch_connected(HostId(3), 300).unwrap();
        // Host 2 never connected and stays out of the list.
        assert_eq!(
            db.load().unwrap().recent,
            [(HostId(3), 300), (HostId(1), 100)],
            "newest connection first"
        );

        db.touch_connected(HostId(1), 400).unwrap();
        assert_eq!(
            db.load().unwrap().recent,
            [(HostId(1), 400), (HostId(3), 300)]
        );
    }

    #[test]
    fn a_batch_lands_in_one_commit_and_a_failed_write_takes_only_itself() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qishell.db");
        {
            let db = HostDatabase::open(&path).unwrap();
            db.begin_batch().unwrap();
            db.insert_host(&host(1, "web-01")).unwrap();
            // A host that jumps through itself fails whole: neither its row
            // nor its jump is left behind, and the batch goes on.
            let looped = routed(host(2, "loop"), Route::Jump(vec![Some(HostId(2))]));
            assert!(db.insert_host(&looped).is_err());
            let jumped = routed(host(3, "app"), Route::Jump(vec![Some(HostId(1))]));
            db.insert_host(&jumped).unwrap();
            db.end_batch().unwrap();
            assert!(db.connection.is_autocommit());
            // Back to one transaction per write.
            db.insert_host(&host(4, "db-01")).unwrap();
        }
        let db = HostDatabase::open(&path).unwrap();
        let data = db.load().unwrap();
        let names: Vec<&str> = data.hosts.iter().map(|host| host.name.as_ref()).collect();
        assert_eq!(names, ["web-01", "app", "db-01"]);
        let jumps: i64 = db
            .connection
            .query_row("SELECT COUNT(*) FROM host_jumps", [], |row| row.get(0))
            .unwrap();
        assert_eq!(jumps, 1);
    }

    #[test]
    fn reopening_the_same_file_keeps_the_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qishell.db");
        {
            let db = HostDatabase::open(&path).unwrap();
            db.insert_host(&host(1, "web-01")).unwrap();
            db.touch_connected(HostId(1), 42).unwrap();
        }
        let db = HostDatabase::open(&path).unwrap();
        let data = db.load().unwrap();
        assert_eq!(data.hosts[0].name.as_ref(), "web-01");
        assert_eq!(data.recent, [(HostId(1), 42)]);
    }

    #[test]
    fn the_detected_operating_system_is_read_back() {
        let db = HostDatabase::in_memory().unwrap();
        let host = host(1, "web");
        db.insert_host(&host).unwrap();
        assert_eq!(db.load().unwrap().hosts[0].os, None);

        db.set_host_os(host.id, Some(HostOs::Ubuntu)).unwrap();
        assert_eq!(db.load().unwrap().hosts[0].os, Some(HostOs::Ubuntu));

        // Renaming must not disturb it: the form does not own this column.
        let mut renamed = host.clone();
        renamed.name = "web-01".into();
        db.update_host(&renamed).unwrap();
        assert_eq!(db.load().unwrap().hosts[0].os, Some(HostOs::Ubuntu));

        db.set_host_os(host.id, None).unwrap();
        assert_eq!(db.load().unwrap().hosts[0].os, None);
    }

    #[test]
    fn whether_to_probe_the_system_is_kept_and_the_form_can_turn_it_back_on() {
        let db = HostDatabase::in_memory().unwrap();
        let bastion = host(1, "bastion");
        db.insert_host(&bastion).unwrap();
        assert!(db.load().unwrap().hosts[0].probe_os, "新主机默认探测");

        // 探测把连接弄断了：连接这边记下不再探测，主机表单没开过。
        db.set_host_probe(bastion.id, false).unwrap();
        assert!(!db.load().unwrap().hosts[0].probe_os);

        // 用户在主机对话框里重新打开。
        let mut edited = bastion.clone();
        edited.probe_os = true;
        db.update_host(&edited).unwrap();
        assert!(db.load().unwrap().hosts[0].probe_os);

        // 新建时就关掉的也记得住。
        let mut quiet = host(2, "quiet");
        quiet.probe_os = false;
        db.insert_host(&quiet).unwrap();
        assert!(!db.load().unwrap().hosts[1].probe_os);
    }

    #[test]
    fn the_probed_hostname_is_kept_across_restarts_and_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qishell.db");
        {
            let db = HostDatabase::open(&path).unwrap();
            db.insert_host(&host(1, "bastion")).unwrap();
            assert_eq!(db.load().unwrap().hosts[0].hostname, None);
            db.set_host_hostname(HostId(1), Some("web-01")).unwrap();
        }
        let db = HostDatabase::open(&path).unwrap();
        let mut stored = db.load().unwrap().hosts.remove(0);
        assert_eq!(stored.hostname.as_deref(), Some("web-01"));

        // 主机表单保存时按主机数据给的写（地址变了时主机数据会先清掉它）。
        stored.hostname = None;
        db.update_host(&stored).unwrap();
        assert_eq!(db.load().unwrap().hosts[0].hostname, None);

        // 新建（复制出来的主机）时也写进去。
        let mut copy = host(2, "copy");
        copy.hostname = Some("web-01".into());
        db.insert_host(&copy).unwrap();
        assert_eq!(
            db.load().unwrap().hosts[1].hostname.as_deref(),
            Some("web-01")
        );
    }

    #[test]
    fn a_schema_two_file_is_brought_to_three_without_a_hostname() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qishell.db");
        drop(HostDatabase::open(&path).unwrap());
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch(
                "ALTER TABLE hosts DROP COLUMN hostname;\n\
                 INSERT INTO hosts (id, name, address, port, auth, username, probe_os) \
                 VALUES (1, 'web', '10.0.0.1', 22, 'password', 'root', 0);\n\
                 PRAGMA user_version = 2;",
            )
            .unwrap();
        }

        let db = HostDatabase::open(&path).unwrap();
        assert_eq!(version_of(&db.connection), SCHEMA_VERSION);
        let hosts = db.load().unwrap().hosts;
        assert_eq!(hosts[0].hostname, None);
        assert!(!hosts[0].probe_os);
        let fresh = HostDatabase::in_memory().unwrap();
        assert_eq!(schema_of(&db.connection), schema_of(&fresh.connection));
    }

    #[test]
    fn a_probe_turned_off_stays_off_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qishell.db");
        {
            let db = HostDatabase::open(&path).unwrap();
            db.insert_host(&host(1, "bastion")).unwrap();
            db.set_host_probe(HostId(1), false).unwrap();
        }
        let db = HostDatabase::open(&path).unwrap();
        assert!(!db.load().unwrap().hosts[0].probe_os);
    }

    #[test]
    fn a_schema_one_file_is_brought_to_two_and_its_hosts_still_probe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qishell.db");
        drop(HostDatabase::open(&path).unwrap());
        // 做出一个 schema 1 的文件：v2 加的列去掉，版本号退回 1，里面有一台主机。
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch(
                "ALTER TABLE hosts DROP COLUMN hostname;\n\
                 ALTER TABLE hosts DROP COLUMN probe_os;\n\
                 INSERT INTO hosts (id, name, address, port, auth, username) \
                 VALUES (1, 'web', '10.0.0.1', 22, 'password', 'root');\n\
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        }

        let db = HostDatabase::open(&path).unwrap();
        assert_eq!(version_of(&db.connection), SCHEMA_VERSION);
        let hosts = db.load().unwrap().hosts;
        assert_eq!(hosts[0].name.as_ref(), "web");
        assert!(hosts[0].probe_os, "升级前的主机照旧探测");
        let fresh = HostDatabase::in_memory().unwrap();
        assert_eq!(schema_of(&db.connection), schema_of(&fresh.connection));
    }

    #[test]
    fn schema_never_contains_secret_columns() {
        let db = HostDatabase::in_memory().unwrap();
        let mut columns = Vec::new();
        for table in tables_of(&db.connection) {
            let mut statement = db
                .connection
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap();
            columns.extend(
                statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap(),
            );
        }
        assert!(columns.contains(&"secret_id".to_string()));
        assert!(!columns.iter().any(|column| {
            column.contains("password")
                || column.contains("passphrase")
                || column.contains("private")
                || column.contains("key_path")
                || column.contains("otp")
        }));
    }
}
