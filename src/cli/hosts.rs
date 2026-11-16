//! 外部 CLI 看到的主机：列表一行（[`HostInfo`]）和 `hosts show` 的全部（[`HostDetails`]）。
//! 都在 UI 线程上从主机数据算好，交给 [`super::CliServer::set_targets`]；请求线程只读这份快照，
//! 不碰主机数据和保险库。
// 参考 since2006/shell-rs @ b2a307a: src/cli/manage.rs 的 `host_info`、`host_details`、`host_secrets`、
// `with_saved_passwords` 重写：没有分组、协议、临时主机；「存没存密码」在算快照时就问好。

use micaterm_hosts::{AuthKind, Host, HostData, ProxyKind, Route, SecretRef, host_matches};
use micaterm_ssh::HostLogin;

use super::protocol::{AuthChoice, HostDetails, HostInfo, ProxyChoice, RouteDetails};

/// 一台主机在 CLI 眼里的样子。
#[derive(Clone, Debug)]
pub struct CliTarget {
    pub info: HostInfo,
    pub details: HostDetails,
    host: Host,
    /// 怎么登录：凭据、跳板和代理的路线都在 UI 线程上解析好了（工单 02）。
    login: HostLogin,
    /// 登录要用的秘密在保险库里，保险库却锁着：`exec` 不去连，直接回 `vault_locked`。
    vault_locked: bool,
}

impl CliTarget {
    /// `hosts list -q` 的搜索：和主机栏的搜索一样（名称、地址、用户名，空格分开的词都要匹配），
    /// 再加上 ID 完全相同。
    pub fn matches(&self, query: &str) -> bool {
        host_matches(&self.host, query) || self.info.id == query.trim()
    }

    pub fn login(&self) -> &HostLogin {
        &self.login
    }

    /// 要等用户在 MicaTerm 里解锁保险库才能登录。
    pub fn vault_locked(&self) -> bool {
        self.vault_locked
    }
}

/// `data` 里的全部主机，按主机栏的顺序。`saved` 回答保险库里有没有某条秘密（锁着也能答），
/// `locked` 是保险库这会儿锁没锁。
pub fn targets(
    data: &HostData,
    saved: impl Fn(&SecretRef) -> bool,
    locked: bool,
) -> Vec<CliTarget> {
    data.hosts()
        .iter()
        .map(|host| {
            let login = data.login_of(host);
            // 和远程卡片连接前一样（`vault::asks_before_using`）：秘密真在保险库里才算。
            let vault_locked = locked && crate::vault::login_secrets(&login).iter().any(&saved);
            CliTarget {
                info: host_info(host),
                details: host_details(host, data, &saved),
                host: host.clone(),
                login,
                vault_locked,
            }
        })
        .collect()
}

/// 凭据的快照，按凭据栏的顺序。
pub fn credentials(
    data: &HostData,
    saved: impl Fn(&SecretRef) -> bool,
) -> Vec<super::protocol::CredentialDetails> {
    data.credentials()
        .iter()
        .map(|credential| super::manage::credential_details(credential, data, &saved))
        .collect()
}

pub(crate) fn host_info(host: &Host) -> HostInfo {
    HostInfo {
        id: host.public_id.to_string(),
        name: host.name.to_string(),
        user: host.user.to_string(),
        host: host.address.to_string(),
        port: host.port,
        os: host.os.map(|os| os.as_str().to_string()),
    }
}

/// `hosts show` 打印的一台主机，也是改动做完后回给 CLI 的那一台。
pub(crate) fn host_details(
    host: &Host,
    data: &HostData,
    saved: &impl Fn(&SecretRef) -> bool,
) -> HostDetails {
    let credential = host
        .credential
        .and_then(|id| data.credential(id))
        .map(|credential| credential.secret_id.to_string());
    let auth = match (&credential, host.auth) {
        (Some(_), _) => AuthChoice::Credential,
        (None, AuthKind::Password) => AuthChoice::Password,
        (None, AuthKind::NoPassword) => AuthChoice::NoPassword,
    };
    // 只有自己用密码登录的主机才说存没存密码。
    let password_saved = (auth == AuthChoice::Password).then(|| saved(&host.password_secret()));
    let route = match &host.route {
        Route::Direct => RouteDetails::Direct,
        Route::Jump(hops) => RouteDetails::Jump {
            hosts: hops
                .iter()
                .map(|hop| {
                    hop.and_then(|id| data.host(id))
                        .map(|hop| hop.public_id.to_string())
                })
                .collect(),
        },
        Route::Proxy(proxy) => RouteDetails::Proxy {
            kind: match proxy.kind {
                ProxyKind::Http => ProxyChoice::Http,
                ProxyKind::Socks5 => ProxyChoice::Socks5,
            },
            host: proxy.host.to_string(),
            port: proxy.port,
            user: proxy.user.as_ref().map(ToString::to_string),
            password_saved: proxy.password_secret().map(|secret| saved(&secret)),
        },
    };
    HostDetails {
        id: host.public_id.to_string(),
        name: host.name.to_string(),
        host: host.address.to_string(),
        port: host.port,
        user: host.user.to_string(),
        auth,
        credential,
        password_saved,
        route,
        notes: host.notes.to_string(),
        os: host.os.map(|os| os.as_str().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use micaterm_hosts::{
        CredentialDraft, CredentialKind, HostData, HostDraft, ProxyKind, ProxySettings, Route,
    };

    use super::*;

    #[test]
    fn a_host_s_details_say_how_it_logs_in_and_which_passwords_are_saved() {
        let mut data = HostData::empty();
        let credential = data.insert_credential_unnotified(CredentialDraft::new(
            "运维",
            CredentialKind::Agent,
            "deploy",
        ));
        let bastion = data.insert_unnotified(
            HostDraft::new("bastion", "10.0.0.1", 22, "root", AuthKind::NoPassword)
                .with_credential(credential),
        );
        let mut web = HostDraft::new("web", "10.0.0.2", 2222, "root", AuthKind::Password);
        web.route = Route::Jump(vec![Some(bastion), None]);
        let web = data.insert_unnotified(web);
        let mut db = HostDraft::new("db", "10.0.0.3", 22, "root", AuthKind::NoPassword);
        db.route =
            Route::Proxy(ProxySettings::new(ProxyKind::Socks5, "proxy", 1080).with_user("me"));
        let db = data.insert_unnotified(db);

        let saved_secret = data.host(web).unwrap().password_secret();
        let all = targets(&data, |secret| *secret == saved_secret, false);
        let id = |host| data.host(host).unwrap().public_id.to_string();

        let bastion = &all[0].details;
        assert_eq!(bastion.auth, AuthChoice::Credential);
        assert_eq!(
            bastion.credential.as_deref(),
            Some(data.credential(credential).unwrap().secret_id.as_str())
        );
        assert_eq!(bastion.user, "deploy");
        assert_eq!(bastion.password_saved, None);

        let web_details = &all[1].details;
        assert_eq!(web_details.id, id(web));
        assert_eq!(web_details.auth, AuthChoice::Password);
        assert_eq!(web_details.password_saved, Some(true));
        assert_eq!(
            web_details.route,
            RouteDetails::Jump {
                hosts: vec![Some(all[0].info.id.clone()), None]
            }
        );

        let db_details = &all[2].details;
        assert_eq!(db_details.id, id(db));
        assert_eq!(
            db_details.route,
            RouteDetails::Proxy {
                kind: ProxyChoice::Socks5,
                host: "proxy".into(),
                port: 1080,
                user: Some("me".into()),
                password_saved: Some(false),
            }
        );
    }

    #[test]
    fn a_locked_vault_holds_back_only_the_hosts_whose_secrets_are_in_it() {
        let mut data = HostData::empty();
        let saved = data.insert_unnotified(HostDraft::new(
            "web",
            "10.0.0.2",
            22,
            "root",
            AuthKind::Password,
        ));
        data.insert_unnotified(HostDraft::new(
            "db",
            "10.0.0.3",
            22,
            "root",
            AuthKind::NoPassword,
        ));
        let secret = data.host(saved).unwrap().password_secret();
        let in_vault = |candidate: &SecretRef| *candidate == secret;

        let locked = targets(&data, in_vault, true);
        assert!(locked[0].vault_locked());
        assert!(!locked[1].vault_locked(), "无密码登录用不到保险库");
        assert!(!targets(&data, in_vault, false)[0].vault_locked());
        // 没存密码的主机也用不到保险库：登录时现场缺密码，回 missing_credential。
        assert!(!targets(&data, |_| false, true)[0].vault_locked());
    }

    #[test]
    fn the_search_is_the_host_list_s_plus_the_id() {
        let mut data = HostData::empty();
        data.insert_unnotified(HostDraft::new(
            "web-01",
            "10.0.0.2",
            22,
            "deploy",
            AuthKind::Password,
        ));
        let target = &targets(&data, |_| false, false)[0];
        assert!(target.matches("web"));
        assert!(target.matches("deploy 10.0"));
        assert!(target.matches(&format!(" {} ", target.info.id)));
        assert!(!target.matches("db"));
    }
}
