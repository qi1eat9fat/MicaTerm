// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-hosts/src/login.rs，以及 src/data.rs 的
// `login`、`login_of`、`direct_login`、`route_login`。
// 裁剪：去掉 Telnet、串口（`HostConnection`、`TelnetLogin`、`SerialLine`）、临时连接和外部连接
// （`SecretRef::temporary`、`shell_only`），以及只给它们用的 `manual`、`with_credential`。
// 改造：密钥凭据的私钥在保险库里（`SavedKey`，不再是 `key_path`）；主机自己的密码按主机 id
// （`Host::password_secret`），不再按 `user@host:port`。

//! How a connection logs in to a host, with the credential the host uses, if
//! any, already looked up. The store builds one on the UI thread and hands it
//! to the terminal and SFTP workers, which cannot read the store themselves.

pub use micaterm_ssh::{
    DEFAULT_KEY_FILES, HostLogin, JumpLogin, LoginMethod, LoginRoute, ProxyLogin, SavedKey,
};

use super::{AuthKind, Credential, CredentialKind, Host, HostData, HostId, ProxySettings, Route};

impl From<&ProxySettings> for ProxyLogin {
    fn from(proxy: &ProxySettings) -> Self {
        Self {
            kind: proxy.kind,
            host: proxy.host.to_string(),
            port: proxy.port,
            user: proxy.user.as_ref().map(ToString::to_string),
        }
    }
}

/// How a saved host logs in: [`HostLogin`]'s constructor from what the app
/// saved.
pub trait HostLoginExt {
    /// How `host` logs in, given the credential it uses, as if it were
    /// reached directly: its route takes the store to look up. `credential`
    /// is ignored unless it is the one the host names, so a host whose
    /// credential has gone logs in with what the form holds.
    fn of(host: &Host, credential: Option<&Credential>) -> HostLogin;
}

impl HostLoginExt for HostLogin {
    fn of(host: &Host, credential: Option<&Credential>) -> HostLogin {
        let address = host.address.to_string();
        let Some(credential) =
            credential.filter(|credential| host.credential == Some(credential.id))
        else {
            let method = match host.auth {
                AuthKind::Password => LoginMethod::Password,
                AuthKind::NoPassword => LoginMethod::NoPassword,
            };
            // 「无密码」也留着主机自己的密码：服务器在密钥之后还要密码时用。
            return HostLogin::new(address, host.port, host.user.as_ref(), method)
                .with_password(host.password_secret());
        };
        let login = |method| HostLogin::new(address, host.port, credential.user.as_ref(), method);
        match credential.kind {
            CredentialKind::Password => {
                login(LoginMethod::Password).with_password(credential.password_secret())
            }
            // 密钥和 Agent 凭据也留着主机自己的密码，理由同上。
            CredentialKind::Key => login(LoginMethod::Key)
                .with_key(SavedKey::new(credential.secret_id.as_str()))
                .with_password(host.password_secret()),
            CredentialKind::Agent => {
                login(LoginMethod::Agent).with_password(host.password_secret())
            }
        }
    }
}

impl HostData {
    /// How a host logs in, with its credential and its jump hosts' logins
    /// looked up. What the terminal and SFTP workers are given.
    pub fn login(&self, id: HostId) -> Option<HostLogin> {
        self.host(id).map(|host| self.login_of(host))
    }

    pub fn login_of(&self, host: &Host) -> HostLogin {
        self.direct_login(host)
            .with_route(self.route_login(&host.route))
    }

    /// How `host` logs in once the connection has reached it.
    fn direct_login(&self, host: &Host) -> HostLogin {
        HostLogin::of(host, host.credential.and_then(|id| self.credential(id)))
    }

    /// `route` with every jump host's login looked up. A jump host is
    /// reached through the one before it, so its own route plays no part.
    pub fn route_login(&self, route: &Route) -> LoginRoute {
        match route {
            Route::Direct => LoginRoute::Direct,
            Route::Jump(hops) => LoginRoute::Jump(
                hops.iter()
                    .map(|hop| match hop.and_then(|id| self.host(id)) {
                        Some(host) => JumpLogin::Host {
                            name: host.name.to_string(),
                            login: Box::new(self.direct_login(host)),
                        },
                        None => JumpLogin::Deleted,
                    })
                    .collect(),
            ),
            Route::Proxy(proxy) => LoginRoute::Proxy(ProxyLogin::from(proxy)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{CredentialDraft, CredentialId, HostDraft, ProxyKind, SecretRef};
    use super::*;

    fn host(auth: AuthKind) -> Host {
        Host::new(
            HostId(1),
            HostDraft::new("web", "10.0.0.1", 22, "root", auth),
        )
    }

    fn credential(kind: CredentialKind) -> Credential {
        Credential::new(
            CredentialId(7),
            CredentialDraft::new("运维", kind, "deploy"),
        )
    }

    fn using(kind: CredentialKind) -> (Host, Credential) {
        let credential = credential(kind);
        let mut host = host(AuthKind::Password);
        host.credential = Some(credential.id);
        (host, credential)
    }

    #[test]
    fn a_manual_host_logs_in_with_its_own_password() {
        let login = HostLogin::of(&host(AuthKind::Password), None);
        assert_eq!(login.method, LoginMethod::Password);
        assert_eq!(login.user, "root");
        assert_eq!(login.password, Some(SecretRef::password(1)));
        assert_eq!(login.endpoint(), "root@10.0.0.1:22");
    }

    #[test]
    fn a_host_without_a_password_keeps_its_own_for_a_partial_success() {
        let login = HostLogin::of(&host(AuthKind::NoPassword), None);
        assert_eq!(login.method, LoginMethod::NoPassword);
        assert_eq!(login.key, None);
        assert_eq!(login.password, Some(SecretRef::password(1)));
    }

    #[test]
    fn a_password_credential_logs_in_with_its_own_entry() {
        let (host, credential) = using(CredentialKind::Password);
        let login = HostLogin::of(&host, Some(&credential));
        assert_eq!(login.method, LoginMethod::Password);
        assert_eq!(login.user, "deploy");
        assert_eq!(login.password, Some(credential.password_secret()));
    }

    /// 密钥凭据的私钥和口令就是凭据在保险库里的那两条。
    #[test]
    fn a_key_credential_logs_in_with_its_key_from_the_vault() {
        let (host, credential) = using(CredentialKind::Key);
        let login = HostLogin::of(&host, Some(&credential));
        assert_eq!(login.method, LoginMethod::Key);
        let key = login.key.unwrap();
        assert_eq!(key.private_key(), credential.private_key_secret());
        assert_eq!(key.passphrase(), credential.passphrase_secret());
        assert_eq!(login.password, Some(SecretRef::password(1)));
    }

    #[test]
    fn an_agent_credential_keeps_the_hosts_password_for_a_partial_success() {
        let (host, credential) = using(CredentialKind::Agent);
        let login = HostLogin::of(&host, Some(&credential));
        assert_eq!(login.method, LoginMethod::Agent);
        assert_eq!(login.key, None);
        assert_eq!(login.password, Some(SecretRef::password(1)));
    }

    #[test]
    fn a_credential_the_host_does_not_name_is_not_used() {
        let credential = credential(CredentialKind::Password);
        let login = HostLogin::of(&host(AuthKind::NoPassword), Some(&credential));
        assert_eq!(login.method, LoginMethod::NoPassword);
        assert_eq!(login.user, "root");
    }

    /// 连接读的密码条目和 `HostData` 删主机、删代理时算的是同一套名字。
    #[test]
    fn only_a_proxy_with_a_user_has_a_password() {
        let proxy = ProxySettings::new(ProxyKind::Socks5, "127.0.0.1", 7890);
        assert_eq!(ProxyLogin::from(&proxy).password(), None);
        let proxy = proxy.with_user("me");
        let login = ProxyLogin::from(&proxy);
        assert_eq!(login.password(), proxy.password_secret());
        assert_eq!(
            LoginRoute::Proxy(login).proxy_password(),
            Some(SecretRef::proxy("me", "127.0.0.1", 7890))
        );
    }

    #[test]
    fn renaming_a_credential_does_not_change_the_login() {
        let (host, mut credential) = using(CredentialKind::Password);
        let before = HostLogin::of(&host, Some(&credential));
        credential.name = "别的名字".into();
        assert_eq!(HostLogin::of(&host, Some(&credential)), before);
        credential.user = "admin".into();
        assert_ne!(HostLogin::of(&host, Some(&credential)), before);
    }

    #[test]
    fn a_host_behind_jump_hosts_logs_in_through_their_own_logins() {
        let mut store = HostData::empty();
        let credential = store.insert_credential_unnotified(CredentialDraft::new(
            "跳板账号",
            CredentialKind::Agent,
            "jumper",
        ));
        let first = store.insert_unnotified(
            HostDraft::new("阿里云99", "120.25.220.186", 22, "root", AuthKind::Password)
                .with_credential(credential),
        );
        // Its own route plays no part when another host jumps through it.
        let second = store.insert_unnotified(
            HostDraft::new("禅道", "8.138.95.125", 22, "ops", AuthKind::NoPassword)
                .with_route(Route::Jump(vec![Some(first)])),
        );
        let target = store.insert_unnotified(
            HostDraft::new("db", "10.0.0.9", 22, "root", AuthKind::Password)
                .with_route(Route::Jump(vec![Some(first), Some(second)])),
        );

        let login = store.login(target).unwrap();
        assert_eq!(login.password, Some(SecretRef::password(target.0)));
        let LoginRoute::Jump(hops) = &login.route else {
            panic!("not a jump: {:?}", login.route);
        };
        let hops: Vec<_> = hops
            .iter()
            .map(|hop| match hop {
                JumpLogin::Host { name, login } => Some((
                    name.as_str(),
                    login.user.as_str(),
                    login.method,
                    login.route.clone(),
                )),
                JumpLogin::Deleted => None,
            })
            .collect();
        assert_eq!(
            hops,
            [
                Some(("阿里云99", "jumper", LoginMethod::Agent, LoginRoute::Direct)),
                Some(("禅道", "ops", LoginMethod::NoPassword, LoginRoute::Direct)),
            ]
        );
        assert_eq!(store.login(HostId(999)), None);
    }

    /// 删掉的跳板留在原位，连接时直接报错，不会悄悄跳过一跳。
    #[test]
    fn a_deleted_jump_host_stays_in_the_route() {
        let mut store = HostData::empty();
        let jump = store.insert_unnotified(HostDraft::new(
            "跳板",
            "10.0.0.2",
            22,
            "root",
            AuthKind::Password,
        ));
        let target = store.insert_unnotified(
            HostDraft::new("db", "10.0.0.9", 22, "root", AuthKind::Password)
                .with_route(Route::Jump(vec![Some(jump)])),
        );
        assert!(store.remove_unnotified(jump));
        assert_eq!(
            store.login(target).unwrap().route,
            LoginRoute::Jump(vec![JumpLogin::Deleted])
        );
    }
}
