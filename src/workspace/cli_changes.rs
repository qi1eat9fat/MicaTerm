//! 外部 CLI 要的主机、凭据改动（Agent CLI 工单 04），在 UI 线程上照表单那样做：核对（`cli::manage`），
//! 写主机数据，把密码、私钥、口令写进保险库，等写完再回话。请求线程碰不到主机数据，改动在服务里
//! 排着，由 `workspace/cli.rs` 定时取来交给这里，一次一个。
//!
//! 要写秘密而保险库收不下（锁着、还没建、打不开）时整个改动不做：不弹框（后台的 Agent 不该抢焦点），
//! 回 `vault_locked` / `vault_unavailable`；锁着时顺带提醒用户解锁。
// 参考 since2006/shell-rs @ b2a307a: src/workspace/cli_changes.rs 重写：没有分组、端口转发、私钥文件；
// 秘密写进保险库，写之前先看保险库收不收得下。

use gpui_kit::*;
use zeroize::Zeroizing;

use micaterm_hosts::{CredentialId, HostId, SecretRef, VaultStatus};

use super::Workspace;
use crate::cli::{
    CliChange, CliError, CredentialDeleted, CredentialFields, ErrorCode, HostDeleted, HostFields,
    Reply, SecretChange, credential_details, find_credential, find_host, host_details, host_info,
    plan_credential, plan_host,
};
use crate::i18n::t;

impl Workspace {
    /// 做一次 CLI 要的改动；结果就是命令要打印的回复，成功时是改完的主机或凭据。
    pub fn apply_cli_change(
        &mut self,
        change: CliChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Reply, CliError>> {
        let found = |id: &str, cx: &App| find_host(self.hosts.read(cx), id);
        match change {
            CliChange::CreateHost(fields) => self.save_host_from_cli(None, fields, window, cx),
            CliChange::UpdateHost { host, fields } => match found(&host, cx) {
                Ok(id) => self.save_host_from_cli(Some(id), fields, window, cx),
                Err(error) => Task::ready(Err(error)),
            },
            CliChange::DeleteHost { host } => Task::ready(self.delete_host_from_cli(&host, cx)),
            CliChange::CreateCredential(fields) => {
                self.save_credential_from_cli(None, fields, window, cx)
            }
            CliChange::UpdateCredential { credential, fields } => {
                match find_credential(self.hosts.read(cx), &credential) {
                    Ok(found) => {
                        let id = found.id;
                        self.save_credential_from_cli(Some(id), fields, window, cx)
                    }
                    Err(error) => Task::ready(Err(error)),
                }
            }
            CliChange::DeleteCredential { credential } => {
                Task::ready(self.delete_credential_from_cli(&credential, cx))
            }
            CliChange::ReadCard { card, lines } => {
                Task::ready(self.read_cli_card(&card, lines, cx))
            }
            CliChange::SendKey { card, key, repeat } => {
                Task::ready(self.send_cli_key(&card, &key, repeat, cx))
            }
        }
    }

    /// 要写秘密时保险库收不收得下；收不下时为什么。锁着时顺带弹「点这里解锁」的提醒。
    fn vault_refusal(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<CliError> {
        let status = self.vault.read(cx).status()?;
        match status {
            VaultStatus::Unprotected | VaultStatus::Unlocked { .. } => None,
            VaultStatus::Locked => {
                self.remind_to_unlock(window, cx);
                Some(CliError::new(
                    ErrorCode::VaultLocked,
                    t!("cli.change.vault_locked"),
                ))
            }
            VaultStatus::Absent => Some(CliError::new(
                ErrorCode::VaultUnavailable,
                t!("cli.change.vault_absent"),
            )),
            VaultStatus::KeyLost => Some(CliError::new(
                ErrorCode::VaultUnavailable,
                t!("cli.change.vault_key_lost"),
            )),
        }
    }

    /// 「存没存」问保险库：条目名是明文，锁着也能答。拿一份快照：回话时要同时读主机数据。
    fn saved_in_vault(&self, cx: &App) -> impl Fn(&SecretRef) -> bool + 'static {
        let vault = self.vault.read(cx);
        let data = self.hosts.read(cx);
        let hosts = data.hosts().iter().flat_map(|host| {
            let proxy = match &host.route {
                micaterm_hosts::Route::Proxy(proxy) => proxy.password_secret(),
                _ => None,
            };
            [Some(host.password_secret()), proxy]
        });
        let credentials = data.credentials().iter().flat_map(|credential| {
            [
                Some(credential.password_secret()),
                Some(credential.private_key_secret()),
                Some(credential.passphrase_secret()),
            ]
        });
        let saved: std::collections::HashSet<SecretRef> = hosts
            .chain(credentials)
            .flatten()
            .filter(|secret| vault.contains(secret))
            .collect();
        move |secret| saved.contains(secret)
    }

    fn save_host_from_cli(
        &mut self,
        existing: Option<HostId>,
        fields: HostFields,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Reply, CliError>> {
        let plan = match plan_host(fields, existing, self.hosts.read(cx)) {
            Ok(plan) => plan,
            Err(error) => return Task::ready(Err(error)),
        };
        if plan.writes_secret()
            && let Some(refusal) = self.vault_refusal(window, cx)
        {
            return Task::ready(Err(refusal));
        }
        let store = self.hosts.clone();
        let secrets = store.read(cx).secrets();
        cx.spawn(async move |this, cx| {
            // 搬过来的代理密码先读出来：改完主机，原来那条没人用了就会被删掉。
            let proxy = match plan.proxy_password {
                Some((secret, SecretChange::Carry(from))) => {
                    let secrets = secrets.clone();
                    let value = cx
                        .background_spawn(async move { secrets.get(&from).ok().flatten() })
                        .await;
                    value.map(|value| (secret, Some(value)))
                }
                Some((secret, change)) => secret_value(change).map(|value| (secret, value)),
                None => None,
            };
            let (id, mut failures, writes) = store.update(cx, |store, cx| {
                let (id, failures) = store.capture_failures(cx, |store, cx| match existing {
                    Some(id) => {
                        store.update(id, plan.draft, cx);
                        id
                    }
                    None => store.insert(plan.draft, cx),
                });
                let own = store
                    .host(id)
                    .map(|host| host.password_secret())
                    .zip(secret_value(plan.password));
                let writes: Vec<_> = [own, proxy]
                    .into_iter()
                    .flatten()
                    .map(|(secret, value)| store.write_secret(secret, value, cx))
                    .collect();
                (id, failures, writes)
            });
            for write in writes {
                if let Err(error) = write.await {
                    failures.push(error);
                }
            }
            if !failures.is_empty() {
                return Err(save_failed(failures));
            }
            this.update(cx, |this, cx| {
                let saved = this.saved_in_vault(cx);
                let data = this.hosts.read(cx);
                data.host(id)
                    .map(|host| Reply::Host(Box::new(host_details(host, data, &saved))))
                    .ok_or_else(|| {
                        CliError::new(ErrorCode::HostNotFound, t!("cli.change.host_gone"))
                    })
            })
            .unwrap_or_else(|_| Err(closed()))
        })
    }

    /// 删主机，同主机栏：已经打开的卡片不受影响。
    fn delete_host_from_cli(
        &mut self,
        host: &str,
        cx: &mut Context<Self>,
    ) -> Result<Reply, CliError> {
        let id = find_host(self.hosts.read(cx), host)?;
        let saved = self.saved_in_vault(cx);
        let data = self.hosts.read(cx);
        let Some(found) = data.host(id) else {
            return Err(CliError::new(
                ErrorCode::HostNotFound,
                t!("cli.change.host_gone"),
            ));
        };
        let deleted = HostDeleted {
            host: host_details(found, data, &saved),
            jump_users: data.jump_users(&[id]) as u64,
        };
        let (_, failures) = self.hosts.update(cx, |store, cx| {
            store.capture_failures(cx, |store, cx| store.remove(id, cx))
        });
        if !failures.is_empty() {
            return Err(save_failed(failures));
        }
        Ok(Reply::HostDeleted(deleted))
    }

    fn save_credential_from_cli(
        &mut self,
        existing: Option<CredentialId>,
        fields: CredentialFields,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Reply, CliError>> {
        let plan = match plan_credential(fields, existing, self.hosts.read(cx)) {
            Ok(plan) => plan,
            Err(error) => return Task::ready(Err(error)),
        };
        if plan.writes_secret()
            && let Some(refusal) = self.vault_refusal(window, cx)
        {
            return Task::ready(Err(refusal));
        }
        let store = self.hosts.clone();
        // 凭据（连公钥），再把密码、私钥、口令写进保险库：和凭据对话框一个顺序。
        let saved = store.update(cx, |store, cx| {
            let (id, failures) = store.capture_failures(cx, |store, cx| match existing {
                Some(id) => store.update_credential(id, plan.draft, cx).then_some(id),
                None => Some(store.insert_credential(plan.draft, cx)),
            });
            let credential = store.credential(id?)?.clone();
            let mut writes = Vec::new();
            if let Some(value) = secret_value(plan.password) {
                writes.push(store.write_secret(credential.password_secret(), value, cx));
            }
            if let Some(key) = plan.key {
                writes.push(store.write_secret(
                    credential.private_key_secret(),
                    Some(key.text),
                    cx,
                ));
            }
            if let Some(value) = secret_value(plan.passphrase) {
                writes.push(store.write_secret(credential.passphrase_secret(), value, cx));
            }
            Some((credential.id, failures, writes))
        });
        let Some((id, mut failures, writes)) = saved else {
            return Task::ready(Err(credential_gone()));
        };
        cx.spawn(async move |this, cx| {
            for write in writes {
                if let Err(error) = write.await {
                    failures.push(error);
                }
            }
            if !failures.is_empty() {
                return Err(save_failed(failures));
            }
            this.update(cx, |this, cx| {
                let saved = this.saved_in_vault(cx);
                let data = this.hosts.read(cx);
                data.credential(id)
                    .map(|credential| {
                        Reply::Credential(Box::new(credential_details(credential, data, &saved)))
                    })
                    .ok_or_else(credential_gone)
            })
            .unwrap_or_else(|_| Err(closed()))
        })
    }

    /// 删凭据，同凭据栏：用它的主机从此自己登录，回话里列出它们。
    fn delete_credential_from_cli(
        &mut self,
        credential: &str,
        cx: &mut Context<Self>,
    ) -> Result<Reply, CliError> {
        let saved = self.saved_in_vault(cx);
        let data = self.hosts.read(cx);
        let found = find_credential(data, credential)?;
        let (id, details) = (found.id, credential_details(found, data, &saved));
        let (released, failures) = self.hosts.update(cx, |store, cx| {
            store.capture_failures(cx, |store, cx| store.remove_credential(id, cx))
        });
        if !failures.is_empty() {
            return Err(save_failed(failures));
        }
        let data = self.hosts.read(cx);
        Ok(Reply::CredentialDeleted(CredentialDeleted {
            credential: details,
            released: released
                .iter()
                .filter_map(|id| data.host(*id))
                .map(host_info)
                .collect(),
        }))
    }
}

/// 要往保险库写什么：`None` 是不动，`Some(None)` 是删掉。搬过来的另外读。
fn secret_value(change: SecretChange) -> Option<Option<Zeroizing<String>>> {
    match change {
        SecretChange::Keep | SecretChange::Carry(_) => None,
        SecretChange::Set(secret) => Some(Some(Zeroizing::new(secret.expose().to_string()))),
        SecretChange::Delete => Some(None),
    }
}

fn credential_gone() -> CliError {
    CliError::new(
        ErrorCode::CredentialNotFound,
        t!("cli.change.credential_gone"),
    )
}

fn closed() -> CliError {
    CliError::new(
        ErrorCode::ConnectFailed,
        t!("cli.server.closed_during_change"),
    )
}

/// 一条一条接着说出来。
fn save_failed(failures: Vec<SharedString>) -> CliError {
    let message = failures
        .into_iter()
        .reduce(|before, next| t!("cli.change.failures", before = before, next = next))
        .unwrap_or_default();
    CliError::new(ErrorCode::SaveFailed, message)
}
