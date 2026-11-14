//! 一次登录要读保险库里的哪些秘密：连接之前据此决定要不要先弹解锁框。

use micaterm_hosts::{HostLogin, JumpLogin, LoginMethod, LoginRoute, SecretRef};

/// `login` 要读的秘密：密码登录读密码，密钥登录读私钥和口令；无密码和 Agent 登录什么都不读
/// （spec：不弹解锁框）。跳板主机各自的登录、代理的密码也算在内。
pub fn login_secrets(login: &HostLogin) -> Vec<SecretRef> {
    let mut secrets = Vec::new();
    collect(login, &mut secrets);
    secrets
}

fn collect(login: &HostLogin, secrets: &mut Vec<SecretRef>) {
    match login.method {
        LoginMethod::Password => secrets.extend(login.password.clone()),
        LoginMethod::Key => {
            if let Some(key) = &login.key {
                secrets.push(key.private_key());
                secrets.push(key.passphrase());
            }
        }
        LoginMethod::NoPassword | LoginMethod::Agent => {}
    }
    match &login.route {
        LoginRoute::Direct => {}
        LoginRoute::Jump(hops) => {
            for hop in hops {
                if let JumpLogin::Host { login, .. } = hop {
                    collect(login, secrets);
                }
            }
        }
        LoginRoute::Proxy(proxy) => secrets.extend(proxy.password()),
    }
}

#[cfg(test)]
mod tests {
    use micaterm_hosts::{
        HostLogin, JumpLogin, LoginMethod, LoginRoute, ProxyLogin, SavedKey, SecretRef,
    };
    use micaterm_ssh::ProxyKind;

    use super::login_secrets;

    #[test]
    fn a_password_login_reads_its_password() {
        let login = HostLogin::new("10.0.0.1", 22, "root", LoginMethod::Password)
            .with_password(SecretRef::password(1));
        assert_eq!(login_secrets(&login), vec![SecretRef::password(1)]);
    }

    #[test]
    fn agent_and_no_password_logins_read_nothing() {
        for method in [LoginMethod::Agent, LoginMethod::NoPassword] {
            let login = HostLogin::new("10.0.0.1", 22, "root", method)
                .with_password(SecretRef::password(1));
            assert!(login_secrets(&login).is_empty(), "{method:?}");
        }
    }

    #[test]
    fn a_key_login_reads_its_key_and_passphrase() {
        let login = HostLogin::new("10.0.0.1", 22, "root", LoginMethod::Key)
            .with_key(SavedKey::new("Jwg5rHvXCxw89paM"));
        assert_eq!(
            login_secrets(&login),
            vec![
                SecretRef::private_key("Jwg5rHvXCxw89paM"),
                SecretRef::passphrase("Jwg5rHvXCxw89paM")
            ]
        );
    }

    #[test]
    fn jump_hosts_and_proxies_bring_their_own() {
        let jump = HostLogin::new("10.0.0.9", 22, "jump", LoginMethod::Password)
            .with_password(SecretRef::password(9));
        let login = HostLogin::new("10.0.0.1", 22, "root", LoginMethod::Agent).with_route(
            LoginRoute::Jump(vec![
                JumpLogin::Deleted,
                JumpLogin::Host {
                    name: "jp-gateway".into(),
                    login: Box::new(jump),
                },
            ]),
        );
        assert_eq!(login_secrets(&login), vec![SecretRef::password(9)]);

        let proxied = HostLogin::new("10.0.0.1", 22, "root", LoginMethod::Agent).with_route(
            LoginRoute::Proxy(ProxyLogin {
                kind: ProxyKind::Socks5,
                host: "127.0.0.1".into(),
                port: 7890,
                user: Some("me".into()),
            }),
        );
        assert_eq!(
            login_secrets(&proxied),
            vec![SecretRef::proxy("me", "127.0.0.1", 7890)]
        );
    }
}
