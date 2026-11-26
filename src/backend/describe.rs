// 来自 since2006/shell-rs @ b2a307a: src/ssh/describe.rs
// 裁剪：去掉英文文案和运行时切换语言（外部 CLI 的 `exec` 错误在 Agent CLI 工单 02 加回）；
// 改造：私钥进保险库，口令和读不出的私钥按 `PrivateKeySource` 说是哪一把（保险库里的，还是默认私钥文件），
// 加上私钥不在保险库里（`AuthError::KeyMissing`）。

//! 连接的错误和问题，用用户的话说出来。SSH 核心只说发生了什么；在这里、在要显示的时候翻成文案。
//!
//! 错误按 `{:#}` 的样子一环一环往下说，SSH 核心认得的每一环换成自己的话。

use std::error::Error;

use micaterm_ssh::{
    AgentProblem, AgentUnreachable, AuthError, AuthenticationPrompt, AuthenticationRequest,
    ChannelError, ConnectError, ConnectionPromptField, ExecError, ExecErrorKind, ExecFailure,
    ForwardProblem, HostKeyChangedPrompt, KnownHostsError, Latency, MissingCredential,
    PasswordWanted, PrivateKeySource, ProxyError, RouteFailure, UnknownHostPrompt, link_as,
};

use crate::i18n::t;

type Cause<'a> = &'a (dyn Error + 'static);

/// 错误自己这一环的话，不带下面的原因：`to_string()` 给的那句，翻过的。
pub fn describe_error(error: &anyhow::Error) -> String {
    let causes: Vec<Cause<'_>> = error.chain().collect();
    link(causes[0], &causes[1..])
}

/// 错误一直说到根上，像 `{:#}` 那样，翻过的。
pub fn describe_chain(error: &anyhow::Error) -> String {
    let causes: Vec<Cause<'_>> = error.chain().collect();
    chain_text(&causes)
}

/// 登录为什么失败，只看错误本身：在去主机的哪一段失败的、缺了什么凭据、网络自己的回答、
/// 和服务器谈不拢的算法，或者错误一路说到根上（光说「无法建立 SSH 连接」不知道该改什么）。
pub fn describe_login_error(error: &anyhow::Error) -> String {
    let causes: Vec<Cause<'_>> = error.chain().collect();
    login_reason(&causes)
}

/// 没人可问、缺了 `need`。
pub fn describe_missing_credential(need: MissingCredential) -> String {
    match need {
        MissingCredential::Password { rejected: true } => t!("ssh.missing.password_rejected"),
        MissingCredential::Password { rejected: false } => t!("ssh.missing.password"),
        MissingCredential::Passphrase { rejected: true } => t!("ssh.missing.passphrase_rejected"),
        MissingCredential::Passphrase { rejected: false } => t!("ssh.missing.passphrase"),
        MissingCredential::KeyboardInteractive => t!("ssh.missing.keyboard_interactive"),
    }
    .into()
}

/// 外部 CLI 的命令为什么没跑完（Agent CLI 工单 02）。没人可问，所以缺凭据时说去 MicaTerm 里连一次。
pub fn describe_exec_error(error: &ExecError) -> String {
    let causes: Vec<Cause<'_>> = error.error.chain().collect();
    if let Some(failure) = link_as::<ExecFailure>(causes[0]) {
        let under = || chain_text(&causes[1..]);
        return match failure {
            ExecFailure::RuntimeFailed => t!("ssh.exec.runtime_failed", error = under()),
            ExecFailure::CommandFailed => t!("ssh.exec.command_failed", error = under()),
            ExecFailure::Aborted => t!("ssh.exec.aborted"),
            ExecFailure::Refused => t!("ssh.exec.refused"),
            ExecFailure::Disconnected => t!("ssh.exec.disconnected"),
            ExecFailure::HostKeyChanged { endpoint } => {
                t!("ssh.exec.host_key_changed", endpoint = endpoint)
            }
            ExecFailure::HostKeyUnknown { endpoint } => {
                t!("ssh.exec.host_key_unknown", endpoint = endpoint)
            }
        }
        .into();
    }
    if error.kind == ExecErrorKind::MissingCredential
        && let Some(need) = causes
            .iter()
            .find_map(|cause| link_as::<MissingCredential>(*cause))
    {
        return match need {
            MissingCredential::Password { rejected: false } => t!("ssh.exec.no_saved_password"),
            MissingCredential::Password { rejected: true } => {
                t!("ssh.exec.saved_password_rejected")
            }
            MissingCredential::Passphrase { rejected: false } => {
                t!("ssh.exec.no_saved_passphrase")
            }
            MissingCredential::Passphrase { rejected: true } => {
                t!("ssh.exec.saved_passphrase_rejected")
            }
            MissingCredential::KeyboardInteractive => t!("ssh.exec.keyboard_interactive"),
        }
        .into();
    }
    login_reason(&causes)
}

/// 状态栏上的延迟：整毫秒。
pub fn latency_label(latency: Latency) -> String {
    match latency {
        Latency::Measured(rtt) => t!("ssh.latency.ms", ms = rtt.as_millis()).into(),
        Latency::TimedOut => t!("ssh.latency.timed_out").into(),
    }
}

/// 第一次见到的主机密钥：确认框里的那一句（跳板主机的要说是哪一台）。地址和端口写在弹窗头部，
/// 这句不再重复（26 号「连接提示」）。
pub fn unknown_host_lead(prompt: &UnknownHostPrompt) -> String {
    with_jump_host(t!("ssh.unknown_host.lead").into(), prompt.jump_host())
}

/// 主机密钥变了：告警框里的那一句（跳板主机的要说是哪一台）。地址和端口写在弹窗头部。
pub fn host_key_changed_lead(prompt: &HostKeyChangedPrompt) -> String {
    with_jump_host(t!("ssh.host_key_changed.lead").into(), prompt.jump_host())
}

/// 认证框的标题：服务器自己的问题用服务器给的名字，没有就叫「输入验证码」。
pub fn authentication_title(prompt: &AuthenticationPrompt) -> String {
    match prompt.request() {
        AuthenticationRequest::Password { .. } => t!("ssh.auth.prompt_title").into(),
        AuthenticationRequest::Passphrase { .. } => t!("ssh.key.prompt_title").into(),
        AuthenticationRequest::KeyboardInteractive { name, .. } if !name.trim().is_empty() => {
            name.clone()
        }
        AuthenticationRequest::KeyboardInteractive { .. } => t!("ssh.auth.code_title").into(),
    }
}

/// 认证框字段上面那段话。跳板主机的要说是哪一台：用户打开的是另一台主机。
pub fn authentication_instructions(prompt: &AuthenticationPrompt) -> String {
    let instructions = match prompt.request() {
        AuthenticationRequest::Password { saved_rejected } => if *saved_rejected {
            t!("ssh.auth.saved_password_rejected")
        } else {
            t!("ssh.auth.enter_password")
        }
        .to_string(),
        AuthenticationRequest::Passphrase {
            key,
            saved_rejected,
        } => {
            let key = key_name(key);
            if *saved_rejected {
                t!("ssh.key.saved_passphrase_rejected", key = key)
            } else {
                t!("ssh.key.enter_passphrase", key = key)
            }
            .to_string()
        }
        AuthenticationRequest::KeyboardInteractive {
            instructions,
            saved_password_rejected,
            ..
        } => {
            let lead = if *saved_password_rejected {
                t!("ssh.auth.saved_password_rejected")
            } else {
                t!("ssh.auth.keyboard_interactive")
            };
            if instructions.trim().is_empty() {
                lead.to_string()
            } else {
                format!("{lead}\n{instructions}")
            }
        }
    };
    with_jump_host(instructions, prompt.jump_host())
}

/// 认证框的字段，带标签。
pub fn authentication_fields(prompt: &AuthenticationPrompt) -> Vec<ConnectionPromptField> {
    match prompt.request() {
        AuthenticationRequest::Password { .. } => {
            vec![ConnectionPromptField::new(
                t!("ssh.auth.password_field"),
                false,
            )]
        }
        AuthenticationRequest::Passphrase { .. } => {
            vec![ConnectionPromptField::new(
                t!("ssh.key.passphrase_field"),
                false,
            )]
        }
        AuthenticationRequest::KeyboardInteractive { fields, .. } => fields.clone(),
    }
}

/// 一把私钥怎么称呼：保险库里的，或者默认私钥文件的路径。
fn key_name(key: &PrivateKeySource) -> String {
    match key {
        PrivateKeySource::Saved(_) => t!("ssh.key.saved_name").into(),
        PrivateKeySource::File(path) => path.display().to_string(),
    }
}

/// 跳板主机的问题前面写上是哪一台。
fn with_jump_host(text: String, jump_host: Option<&str>) -> String {
    let Some(name) = jump_host else {
        return text;
    };
    if text.trim().is_empty() {
        t!("ssh.jump_host.instructions", name = name).into()
    } else {
        t!(
            "ssh.jump_host.with_instructions",
            name = name,
            instructions = text
        )
        .into()
    }
}

/// `causes` 每一环的话，像 `{:#}` 那样连起来。
fn chain_text(causes: &[Cause<'_>]) -> String {
    (0..causes.len())
        .map(|ix| link(causes[ix], &causes[ix + 1..]))
        .collect::<Vec<_>>()
        .join(": ")
}

/// 见 [`describe_login_error`]；`causes` 是错误链。
fn login_reason(causes: &[Cause<'_>]) -> String {
    if let Some(ix) = causes
        .iter()
        .position(|&cause| link_as::<RouteFailure>(cause).is_some())
    {
        return link(causes[ix], &causes[ix + 1..]);
    }
    for &cause in causes {
        if let Some(need) = link_as::<MissingCredential>(cause) {
            return describe_missing_credential(*need);
        }
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            return connect_error(io);
        }
        match cause.downcast_ref::<russh::Error>() {
            // russh 把套接字的错误包在里面，不挂在链上。
            Some(russh::Error::IO(io)) => return connect_error(io),
            Some(russh::Error::ConnectionTimeout) => return t!("ssh.error.timeout").into(),
            Some(russh::Error::NoCommonAlgo { kind, theirs, .. }) => {
                return no_common_algorithm(kind, &theirs.join(&*t!("ssh.list.separator")));
            }
            _ => {}
        }
    }
    chain_text(causes)
}

/// 链上的一环。`rest` 是它下面的，说「在哪里失败」的那几环接着往下说。
pub(super) fn link(cause: Cause<'_>, rest: &[Cause<'_>]) -> String {
    if let Some(error) = link_as::<ConnectError>(cause) {
        return connect(error);
    }
    if let Some(error) = link_as::<RouteFailure>(cause) {
        return route(error, rest);
    }
    if let Some(error) = link_as::<KnownHostsError>(cause) {
        return known_hosts(error);
    }
    if let Some(error) = link_as::<AuthError>(cause) {
        return auth(error);
    }
    if let Some(error) = link_as::<AgentProblem>(cause) {
        return agent(error);
    }
    if let Some(error) = link_as::<ProxyError>(cause) {
        return proxy(error);
    }
    if let Some(error) = link_as::<ChannelError>(cause) {
        return describe_channel_error(error);
    }
    if let Some(need) = link_as::<MissingCredential>(cause) {
        return describe_missing_credential(*need);
    }
    if link_as::<PasswordWanted>(cause).is_some() {
        return t!("ssh.password_wanted").into();
    }
    if let Some(io) = cause.downcast_ref::<std::io::Error>() {
        return io_text(io);
    }
    cause.to_string()
}

/// I/O 错误自己的话：SSH 核心放进去的那一环的，或者系统的。
fn io_text(io: &std::io::Error) -> String {
    match io.get_ref() {
        Some(inner) => link(inner, &[]),
        None => io.to_string(),
    }
}

fn connect(error: &ConnectError) -> String {
    match error {
        ConnectError::Handshake => t!("ssh.connect.failed"),
        ConnectError::Unreachable { host, port } => {
            t!("ssh.route.connect_failed", host = host, port = port)
        }
        ConnectError::Cancelled => t!("ssh.connect.cancelled"),
        ConnectError::NoJumpHosts => t!("ssh.jump.none"),
        ConnectError::ViewClosed => t!("ssh.prompt.view_closed"),
        ConnectError::PromptCancelled => t!("ssh.prompt.request_cancelled"),
    }
    .into()
}

fn route(failure: &RouteFailure, rest: &[Cause<'_>]) -> String {
    match failure {
        RouteFailure::JumpDeleted { number } => t!("ssh.jump.deleted", number = number),
        RouteFailure::Jump { name } => {
            t!("ssh.jump.failed", name = name, reason = login_reason(rest))
        }
        RouteFailure::JumpForward {
            name,
            host,
            port,
            problem,
        } => match problem {
            ForwardProblem::ConnectFailed => t!(
                "ssh.jump.forward.connect_failed",
                name = name,
                host = host,
                port = port
            ),
            ForwardProblem::Prohibited => t!(
                "ssh.jump.forward.prohibited",
                name = name,
                host = host,
                port = port
            ),
            ForwardProblem::TimedOut => t!(
                "ssh.jump.forward.timed_out",
                name = name,
                host = host,
                port = port
            ),
            ForwardProblem::ChannelFailed => t!(
                "ssh.jump.forward.channel_failed",
                name = name,
                host = host,
                port = port
            ),
        },
        RouteFailure::ProxyUnreachable { host, port } => t!(
            "ssh.proxy.connect_failed",
            host = host,
            port = port,
            reason = login_reason(rest)
        ),
        // 代理自己的回答就说清楚了。
        RouteFailure::Proxy => {
            return match rest {
                [answer, under @ ..] => link(*answer, under),
                [] => String::new(),
            };
        }
    }
    .into()
}

fn known_hosts(error: &KnownHostsError) -> String {
    match error {
        KnownHostsError::ReadFailed { path } => {
            t!("ssh.known_hosts.read_failed", path = path.display())
        }
        KnownHostsError::WriteFailed { path } => {
            t!("ssh.known_hosts.write_failed", path = path.display())
        }
        KnownHostsError::Corrupt { path } => t!("ssh.known_hosts.corrupt", path = path.display()),
        KnownHostsError::Unreadable { path } => {
            t!("ssh.known_hosts.unreadable", path = path.display())
        }
        KnownHostsError::ChangedWhileSaving => t!("ssh.known_hosts.changed_while_saving"),
    }
    .into()
}

fn auth(error: &AuthError) -> String {
    match error {
        AuthError::MethodsUnknown => t!("ssh.auth.methods_unknown"),
        AuthError::AgentRejected => t!("ssh.auth.agent_rejected"),
        AuthError::AgentNoPublicKey => t!("ssh.auth.agent_no_publickey"),
        AuthError::RsaHash => t!("ssh.auth.rsa_hash"),
        AuthError::AgentHash => t!("ssh.agent.hash"),
        AuthError::KeyFailed => t!("ssh.auth.key_failed"),
        AuthError::KeyRejected => t!("ssh.auth.key_rejected"),
        AuthError::NoKeyAccepted => t!("ssh.auth.no_key_accepted"),
        AuthError::KeyNotChosen => t!("ssh.key.not_chosen"),
        AuthError::KeyMissing => t!("ssh.key.missing"),
        AuthError::KeyUnreadable { key } => t!("ssh.key.unreadable", key = key_name(key)),
        AuthError::KeyTooManyAttempts => t!("ssh.key.too_many_attempts"),
        AuthError::PasswordFailed => t!("ssh.auth.password_failed"),
        AuthError::Cancelled => t!("ssh.auth.cancelled"),
        AuthError::InvalidReply => t!("ssh.auth.invalid_reply"),
        AuthError::KeyboardStartFailed => t!("ssh.auth.keyboard_start_failed"),
        AuthError::KeyboardCancelled => t!("ssh.auth.keyboard_cancelled"),
        AuthError::KeyboardFailed => t!("ssh.auth.keyboard_failed"),
        AuthError::Failed => t!("ssh.auth.failed"),
    }
    .into()
}

fn agent(problem: &AgentProblem) -> String {
    match problem {
        AgentProblem::Unreachable(reason) => {
            let reason = match reason {
                AgentUnreachable::NoSocket => t!("ssh.agent.no_socket").to_string(),
                AgentUnreachable::MissingSocket { path } => {
                    t!("ssh.agent.missing_socket", path = path.display()).to_string()
                }
                AgentUnreachable::NoResponse => t!("ssh.agent.no_response").to_string(),
                AgentUnreachable::ServiceNotRunning => {
                    t!("ssh.agent.service_not_running").to_string()
                }
                AgentUnreachable::Unsupported => t!("ssh.agent.unsupported").to_string(),
                AgentUnreachable::Other(reason) => reason.clone(),
            };
            t!("ssh.agent.unreachable", reason = reason)
        }
        AgentProblem::Empty => t!("ssh.agent.empty"),
        AgentProblem::Unsigned => t!("ssh.agent.unsigned"),
    }
    .into()
}

fn proxy(error: &ProxyError) -> String {
    match error {
        ProxyError::BadCredentials => t!("ssh.proxy.bad_credentials"),
        ProxyError::AuthRequired => t!("ssh.proxy.auth_required"),
        ProxyError::Forbidden { target } => t!("ssh.proxy.forbidden", target = target),
        ProxyError::ReplyTooLong => t!("ssh.proxy.reply_too_long"),
        ProxyError::Closed => t!("ssh.proxy.closed"),
        ProxyError::HttpBadGateway { target } => {
            t!("ssh.proxy.http.bad_gateway", target = target)
        }
        ProxyError::HttpGatewayTimeout { target } => {
            t!("ssh.proxy.http.gateway_timeout", target = target)
        }
        ProxyError::HttpRefused { status } => t!("ssh.proxy.http.refused", status = status),
        ProxyError::HttpUnrecognized => t!("ssh.proxy.http.unrecognized"),
        ProxyError::SocksUnrecognized => t!("ssh.proxy.socks.unrecognized"),
        ProxyError::SocksCredentialsTooLong => t!("ssh.proxy.socks.credentials_too_long"),
        ProxyError::SocksNoPasswordAuth => t!("ssh.proxy.socks.no_password_auth"),
        ProxyError::SocksHostTooLong => t!("ssh.proxy.socks.host_too_long"),
        ProxyError::SocksRulesForbid { target } => {
            t!("ssh.proxy.socks.rules_forbid", target = target)
        }
        ProxyError::SocksNetworkUnreachable { target } => {
            t!("ssh.proxy.socks.network_unreachable", target = target)
        }
        ProxyError::SocksHostUnreachable { target } => {
            t!("ssh.proxy.socks.host_unreachable", target = target)
        }
        ProxyError::SocksRefused { target } => t!("ssh.proxy.socks.refused", target = target),
        ProxyError::SocksTimeout { target } => t!("ssh.proxy.socks.timeout", target = target),
        ProxyError::SocksInternal { target } => t!("ssh.proxy.socks.internal", target = target),
        ProxyError::SocksNoConnect => t!("ssh.proxy.socks.no_connect"),
        ProxyError::SocksAddressType => t!("ssh.proxy.socks.address_type"),
    }
    .into()
}

/// 连着的连接上开通道失败。
pub fn describe_channel_error(error: &ChannelError) -> String {
    match error {
        ChannelError::SessionFailed => t!("ssh.terminal.session_failed"),
        ChannelError::PtyRefused => t!("ssh.terminal.pty_refused"),
        ChannelError::ShellRefused => t!("ssh.terminal.shell_refused"),
        ChannelError::WriteFailed => t!("ssh.terminal.write_failed"),
        ChannelError::ResizeFailed => t!("ssh.terminal.resize_failed"),
        ChannelError::ExecChannelFailed => t!("ssh.exec.channel_failed"),
        ChannelError::ExecSendFailed => t!("ssh.exec.send_failed"),
        ChannelError::ExecRefused => t!("ssh.exec.refused"),
        ChannelError::ExecTimeout => t!("ssh.exec.timeout"),
        ChannelError::SftpRefused => t!("ssh.terminal.sftp_refused"),
        ChannelError::SftpTimeout => t!("ssh.terminal.sftp_timeout"),
        ChannelError::ShellOnly => t!("ssh.terminal.shell_only"),
        ChannelError::ShellOnlySftp => t!("ssh.terminal.shell_only_sftp"),
    }
    .into()
}

/// 服务器没有一个 `kind` 类的算法和 MicaTerm 的一样，它有的是 `theirs`。
fn no_common_algorithm(kind: &russh::AlgorithmKind, theirs: &str) -> String {
    match kind {
        russh::AlgorithmKind::Kex => t!("ssh.error.no_common.kex", theirs = theirs),
        russh::AlgorithmKind::Key => t!("ssh.error.no_common.host_key", theirs = theirs),
        russh::AlgorithmKind::Cipher => t!("ssh.error.no_common.cipher", theirs = theirs),
        russh::AlgorithmKind::Mac => t!("ssh.error.no_common.mac", theirs = theirs),
        russh::AlgorithmKind::Compression => {
            t!("ssh.error.no_common.compression", theirs = theirs)
        }
    }
    .into()
}

/// 连不上的原因，常见的几种说明白，其余用系统自己的说法。
fn connect_error(error: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::ConnectionRefused => t!("ssh.error.refused").into(),
        ErrorKind::TimedOut => t!("ssh.error.timeout").into(),
        ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable => {
            t!("ssh.error.unreachable").into()
        }
        _ => t!("ssh.error.connect_failed", error = io_text(error)).into(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Error, ErrorKind};
    use std::time::Duration;

    use anyhow::anyhow;
    use micaterm_ssh::{ConnectionPromptKind, SavedKey, find_cause, wrap};

    use super::*;

    fn reason(error: anyhow::Error) -> String {
        describe_login_error(&error)
    }

    #[test]
    fn latency_labels_show_whole_milliseconds() {
        assert_eq!(
            latency_label(Latency::Measured(Duration::from_micros(32_900))),
            "32 ms"
        );
        assert_eq!(latency_label(Latency::TimedOut), "Timed Out");
    }

    #[test]
    fn a_new_host_key_says_where_and_through_which_jump_host() {
        let lead = |jump_host| match ConnectionPromptKind::unknown_host(
            "10.0.0.5",
            22,
            "ssh-ed25519",
            "SHA256:abc",
        )
        .at_jump_host(jump_host)
        {
            ConnectionPromptKind::UnknownHost(prompt) => unknown_host_lead(&prompt),
            _ => unreachable!("an unknown host stays one"),
        };
        assert_eq!(
            lead(None),
            "First connection to this host; verify its host key:"
        );
        assert_eq!(
            lead(Some("堡垒机")),
            "Jump host \"堡垒机\": First connection to this host; verify its host key:"
        );
    }

    fn prompt(request: AuthenticationRequest, jump_host: Option<&str>) -> AuthenticationPrompt {
        match ConnectionPromptKind::authentication(request).at_jump_host(jump_host) {
            ConnectionPromptKind::Authentication(prompt) => prompt,
            _ => unreachable!("a credential question stays one"),
        }
    }

    #[test]
    fn credential_questions_are_worded_for_what_they_ask() {
        let password = prompt(
            AuthenticationRequest::Password {
                saved_rejected: false,
            },
            None,
        );
        assert_eq!(authentication_title(&password), "Enter Password");
        assert_eq!(
            authentication_instructions(&password),
            "Enter the login password"
        );
        assert_eq!(authentication_fields(&password)[0].label(), "Password");

        let passphrase = prompt(
            AuthenticationRequest::Passphrase {
                key: PrivateKeySource::Saved(SavedKey::new("k")),
                saved_rejected: true,
            },
            Some("堡垒机"),
        );
        assert_eq!(
            authentication_title(&passphrase),
            "Enter Private Key Passphrase"
        );
        assert_eq!(
            authentication_instructions(&passphrase),
            "Jump host \"堡垒机\": The saved passphrase cannot unlock Saved Private Key; enter it again"
        );

        let code = prompt(
            AuthenticationRequest::KeyboardInteractive {
                name: String::new(),
                instructions: String::new(),
                fields: vec![ConnectionPromptField::new("Verification code: ", true)],
                saved_password_rejected: false,
            },
            None,
        );
        assert_eq!(authentication_title(&code), "Enter Verification Code");
        assert_eq!(
            authentication_instructions(&code),
            "The server requires keyboard-interactive authentication (passwords are answered automatically):"
        );
        assert_eq!(
            authentication_fields(&code)[0].label(),
            "Verification code: "
        );
    }

    #[test]
    fn missing_credentials_name_what_was_wrong() {
        assert_eq!(
            reason(MissingCredential::Password { rejected: true }.into()),
            "Incorrect username or password"
        );
        assert_eq!(
            reason(MissingCredential::Password { rejected: false }.into()),
            "No password was entered"
        );
    }

    #[test]
    fn network_errors_are_explained_under_any_context() {
        let refused = wrap(
            ConnectError::Handshake,
            Error::from(ErrorKind::ConnectionRefused),
        );
        assert_eq!(
            reason(refused),
            "The connection was refused; nothing is listening on that port"
        );
        assert_eq!(
            reason(Error::from(ErrorKind::HostUnreachable).into()),
            "The host is unreachable"
        );
        assert_eq!(
            reason(russh::Error::ConnectionTimeout.into()),
            "The connection timed out"
        );
    }

    #[test]
    fn a_failure_on_the_way_says_where_it_happened() {
        let missing = wrap(
            RouteFailure::Jump {
                name: "阿里云99".into(),
            },
            MissingCredential::Password { rejected: false },
        );
        assert_eq!(reason(missing), "Jump host \"阿里云99\": No password was entered");
    }

    #[test]
    fn a_server_of_other_algorithms_says_which_it_has() {
        let mac = wrap(
            ConnectError::Handshake,
            russh::Error::NoCommonAlgo {
                kind: russh::AlgorithmKind::Mac,
                ours: vec!["hmac-sha2-256".into()],
                theirs: vec!["hmac-sha1".into(), "hmac-md5".into()],
            },
        );
        assert_eq!(
            reason(mac),
            "Cannot establish an SSH connection: no common message authentication (MAC) algorithm with the server, which offers hmac-sha1, hmac-md5"
        );
    }

    #[test]
    fn errors_word_every_link_of_their_chain() {
        let corrupt = anyhow::Error::from(KnownHostsError::Corrupt {
            path: "/data/known_hosts".into(),
        });
        assert_eq!(
            describe_error(&corrupt),
            "The host trust file is damaged: /data/known_hosts"
        );
        let channel = wrap(ConnectError::Cancelled, ChannelError::PtyRefused);
        assert_eq!(
            describe_chain(&channel),
            "The connection was cancelled: The server refused to create a terminal"
        );
        let deep = channel.context("outer");
        assert_eq!(
            find_cause::<ConnectError>(&deep),
            Some(&ConnectError::Cancelled)
        );
        assert_eq!(
            reason(AuthError::KeyMissing.into()),
            "This private key is not in the vault (it may have been deleted, or the vault was copied from another computer). Import it again"
        );
        assert_eq!(reason(anyhow!("anything")), "anything");
    }
}
