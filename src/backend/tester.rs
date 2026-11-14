// 来自 since2006/shell-rs @ b2a307a: src/ssh/tester.rs 和 src/connection.rs 的 `LoginTest`、`ConnectionTester`
// 裁剪：去掉 Telnet、串口、凭据登录的 `saved`、私钥口令的表单值（14 加）；代理密码的表单值工单 15 加回；
// 改造：测试是一个 future（在共享运行时上跑，窗口壳的执行器上等），确认主机密钥也是异步问的，
// 这样 UI 测试的假测试器可以完全跑在测试执行器上。

//! 主机对话框的「测试连接」：用表单里现在的值真的登录一次再挂断，主机密钥走同一个 `known_hosts`。
//! 没人回答密码：表单里密码留空就照「无密码」那样试（Agent、默认私钥），服务器要密码就说「未填写密码」；
//! 第一次见到的主机密钥问用户信不信；密钥变了只报错不问（换密钥留给真正的连接）。

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Duration;

use futures::future::BoxFuture;
use micaterm_ssh::{
    ConnectionPrompt, ConnectionPromptKind, ConnectionPromptReply, HostLogin, JumpLogin,
    LoginMethod, LoginRoute, MissingCredential, PasswordWanted, RouteFailure, SecretRef,
    SecretSource, SharedSecretSource, SshConnectionConfig, SshConnector, SshPrompts,
    UnknownHostPrompt, find_cause, timeout_excluding_prompts,
};
use tokio::sync::watch;
use zeroize::Zeroizing;

use super::describe::{describe_login_error, describe_missing_credential};
use crate::i18n::t;

/// 一次测试最多多久，不算用户决定信不信主机密钥的时间。
const TEST_TIMEOUT: Duration = Duration::from_secs(20);

/// 测试用什么登录：表单里现在的值，保存没保存都行。密码清零、不打印。
pub struct LoginTest {
    login: HostLogin,
    /// 表单里的密码；留空是 `None`。
    password: Option<Zeroizing<String>>,
    /// 表单里的代理密码；留空是 `None`（工单 15）。
    proxy_password: Option<Zeroizing<String>>,
}

impl LoginTest {
    /// 按表单的值登录；`password` 是密码框里的字，空的就是没填。
    pub fn new(login: HostLogin, password: &str) -> Self {
        Self {
            login,
            password: typed(password),
            proxy_password: None,
        }
    }

    /// 代理密码也用表单里填的（存没存过都一样）；空的就是没填。
    pub fn with_proxy_password(mut self, password: &str) -> Self {
        self.proxy_password = typed(password);
        self
    }

    pub fn login(&self) -> &HostLogin {
        &self.login
    }

    /// 表单里填的密码。
    pub fn password(&self) -> Option<&str> {
        self.password.as_deref().map(String::as_str)
    }

    /// 选了「密码」却没填：测试照「无密码」去试，不卡在缺密码上。
    pub fn password_left_empty(&self) -> bool {
        self.login.method == LoginMethod::Password && self.password.is_none()
    }
}

impl std::fmt::Debug for LoginTest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginTest")
            .field("login", &self.login)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field(
                "proxy_password",
                &self.proxy_password.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// 输入框里的字：空的就是没填。
fn typed(text: &str) -> Option<Zeroizing<String>> {
    (!text.is_empty()).then(|| Zeroizing::new(text.to_string()))
}

/// 问用户信不信第一次见到的主机密钥；关了对话框也算不信。
pub type TrustAsker = Arc<dyn Fn(UnknownHostPrompt) -> BoxFuture<'static, bool> + Send + Sync>;

/// 登录一次再挂断，让主机对话框在保存前检查表单的值。生产是 [`SshConnectionTester`]，测试注入假的。
pub trait ConnectionTester: Send + Sync + 'static {
    /// 结果的 `Err` 是给用户看的原因。返回的 future 可以在任何执行器上等。
    fn test(&self, request: LoginTest, trust: TrustAsker)
    -> BoxFuture<'static, Result<(), String>>;
}

pub type SharedConnectionTester = Arc<dyn ConnectionTester>;

/// 和远程终端用同一个连接器（同一个 `known_hosts` 和锁），在共享运行时上登录。
pub struct SshConnectionTester {
    connector: SshConnector,
}

impl SshConnectionTester {
    pub fn new(known_hosts: impl Into<std::path::PathBuf>, secrets: SharedSecretSource) -> Self {
        Self::with_connector(SshConnector::new(known_hosts, secrets))
    }

    pub fn with_connector(connector: SshConnector) -> Self {
        Self { connector }
    }
}

impl ConnectionTester for SshConnectionTester {
    fn test(
        &self,
        request: LoginTest,
        trust: TrustAsker,
    ) -> BoxFuture<'static, Result<(), String>> {
        let connector = self.connector.clone();
        let (sender, receiver) = futures::channel::oneshot::channel();
        super::runtime::shared().spawn(async move {
            let _ = sender.send(run(connector, request, trust).await);
        });
        Box::pin(async move {
            receiver
                .await
                .unwrap_or_else(|_| Err(t!("ssh.connect.cancelled").into()))
        })
    }
}

/// 一路上主机密钥检查说了什么：光看连接的错误分不出来。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct HostTrust {
    declined: bool,
    key_changed: bool,
}

async fn run(connector: SshConnector, request: LoginTest, trust: TrustAsker) -> Result<(), String> {
    let login = request.login();
    resolve_first_leg(&login.route, &login.host, login.port).await?;
    let without_password = request.password_left_empty();
    let config = if without_password {
        let mut login = login.clone();
        login.method = LoginMethod::NoPassword;
        SshConnectionConfig::from(&login)
    } else {
        SshConnectionConfig::from(login)
    };
    let secrets: SharedSecretSource =
        Arc::new(FormSecrets::new(&request, connector.secrets().clone()));
    let (broker, host_trust, _shutdown) = test_broker(trust);
    let connected = timeout_excluding_prompts(
        connector.connect_with(&config, broker.clone(), &secrets),
        broker.prompt_activity_receiver(),
        TEST_TIMEOUT,
    )
    .await
    .and_then(|result| result);
    match connected {
        Ok((handle, _)) => {
            let _ = handle
                .disconnect(russh::Disconnect::ByApplication, "connection test", "zh-CN")
                .await;
            Ok(())
        }
        // 服务器要的是留空的密码：对选了「密码」的人，该说的是这个，而不是「无密码」失败的原因。
        Err(error)
            if without_password
                && find_cause::<RouteFailure>(&error).is_none()
                && error.chain().any(|cause| cause.is::<PasswordWanted>()) =>
        {
            Err(describe_missing_credential(MissingCredential::Password {
                rejected: false,
            }))
        }
        Err(error) => Err(describe_test_failure(&error, *lock(&host_trust))),
    }
}

/// 第一段的地址先单独解析，打错的名字直接说解析不了，而不是解析器自己的说法。
async fn resolve_first_leg(route: &LoginRoute, host: &str, port: u16) -> Result<(), String> {
    let first = match route {
        LoginRoute::Direct => Some((host, port)),
        LoginRoute::Proxy(proxy) => Some((proxy.host.as_str(), proxy.port)),
        LoginRoute::Jump(hops) => match hops.first() {
            Some(JumpLogin::Host { login, .. }) => Some((login.host.as_str(), login.port)),
            _ => None,
        },
    };
    match first {
        Some((host, port)) if tokio::net::lookup_host((host, port)).await.is_err() => {
            Err(t!("ssh.test.unresolved", host = host).into())
        }
        _ => Ok(()),
    }
}

/// 测试用的问答：不问密码（缺了就失败），第一次见到的主机密钥经 `trust` 问用户，密钥变了直接拒绝。
fn test_broker(trust: TrustAsker) -> (Arc<SshPrompts>, Arc<Mutex<HostTrust>>, watch::Sender<bool>) {
    let host_trust = Arc::new(Mutex::new(HostTrust::default()));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    // 回答要经 broker，而 broker 这时还没建好。
    let broker_slot: Arc<OnceLock<Weak<SshPrompts>>> = Arc::new(OnceLock::new());
    let events = {
        let host_trust = host_trust.clone();
        let broker_slot = broker_slot.clone();
        Arc::new(move |prompt: ConnectionPrompt| match prompt.kind() {
            ConnectionPromptKind::UnknownHost(question) => {
                let answer = trust(question.clone());
                let (host_trust, broker_slot) = (host_trust.clone(), broker_slot.clone());
                let request_id = prompt.request_id();
                tokio::spawn(async move {
                    let trusted = answer.await;
                    if !trusted {
                        lock(&host_trust).declined = true;
                    }
                    if let Some(broker) = broker_slot.get().and_then(Weak::upgrade) {
                        broker.respond(
                            request_id,
                            if trusted {
                                ConnectionPromptReply::TrustAndSave
                            } else {
                                ConnectionPromptReply::Cancel
                            },
                        );
                    }
                });
                true
            }
            ConnectionPromptKind::HostKeyChanged(_) => {
                lock(&host_trust).key_changed = true;
                false
            }
            // 不会到这里：broker 不问人，缺凭据直接失败。
            ConnectionPromptKind::Authentication(_) => false,
        })
    };
    let broker = Arc::new(SshPrompts::new(events, shutdown_rx).non_interactive());
    let _ = broker_slot.set(Arc::downgrade(&broker));
    (broker, host_trust, shutdown_tx)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// 测试失败的原因，通知里显示的那句。
fn describe_test_failure(error: &anyhow::Error, host_trust: HostTrust) -> String {
    if host_trust.key_changed {
        return t!("ssh.test.host_key_changed").into();
    }
    if host_trust.declined {
        return t!("ssh.test.host_key_declined").into();
    }
    describe_login_error(error)
}

/// 测试用的秘密：登录密码和代理密码以表单为准（存没存过都一样），其余（跳板主机的密码等）
/// 从保险库读。什么都不写。
struct FormSecrets {
    password: Option<SecretRef>,
    typed: Option<Zeroizing<String>>,
    proxy_password: Option<SecretRef>,
    typed_proxy_password: Option<Zeroizing<String>>,
    fallback: SharedSecretSource,
}

impl FormSecrets {
    fn new(request: &LoginTest, fallback: SharedSecretSource) -> Self {
        Self {
            password: request.login().password.clone(),
            typed: request.password.clone(),
            proxy_password: request.login().route.proxy_password(),
            typed_proxy_password: request.proxy_password.clone(),
            fallback,
        }
    }
}

impl SecretSource for FormSecrets {
    fn secret(&self, secret: &SecretRef) -> anyhow::Result<Option<Zeroizing<String>>> {
        if self.password.as_ref() == Some(secret) {
            return Ok(self.typed.clone());
        }
        if self.proxy_password.as_ref() == Some(secret) {
            return Ok(self.typed_proxy_password.clone());
        }
        self.fallback.secret(secret)
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::{HostTrust, describe_test_failure};

    #[test]
    fn host_trust_outranks_the_connect_error() {
        let error = anyhow!("无法建立 SSH 连接");
        let changed = HostTrust {
            key_changed: true,
            ..HostTrust::default()
        };
        assert!(describe_test_failure(&error, changed).starts_with("主机密钥与已保存的不一致"));
        let declined = HostTrust {
            declined: true,
            ..HostTrust::default()
        };
        assert_eq!(
            describe_test_failure(&error, declined),
            "未信任该主机的密钥"
        );
    }
}
