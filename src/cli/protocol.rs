// 来自 since2006/shell-rs @ b2a307a: src/cli/protocol.rs
// 裁剪（Agent CLI 工单 01）：只留 `hosts list` / `hosts show` 的请求和回复（工单 02 加回 `exec`，
// 工单 03 加回 `upload` / `download` / `sync`，工单 04 加回主机和凭据的增删改查：字段照 MicaTerm 的模型，
// 没有分组、协议；凭据的私钥进保险库，`key_path` 只读一次）；去掉分组、Telnet、串口、
// 临时主机、`Activate`（单实例唤起）和输出帧之外 exec、传输、增删改的类型，后面的工单再按需加回来。
// 协议版本从 1 起，和 shell-rs 的不互通。

//! What travels over the CLI socket. Every message is a frame: one byte
//! saying what it holds, four bytes of big-endian length, then the payload.
//! Control messages are JSON; a command's output is passed on as raw bytes,
//! so binary output needs no encoding.
//!
//! One connection carries one request: the client sends it as the first
//! frame, the app answers with any number of frames and closes.

use std::{
    io::{self, Read, Write},
    path::PathBuf,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use zeroize::Zeroizing;

/// Bumped when a request or reply changes shape. The command and the app
/// come from the same build, so they differ only while an older copy still
/// runs, or on Windows while the copy on the PATH has not been updated.
///
/// New requests and replies are added without bumping it, so a command
/// and an app of different builds still agree on what both know; an app
/// asked for a request it does not know answers `bad_request`, which the
/// command explains.
pub const PROTOCOL_VERSION: u32 = 1;

/// Frames larger than this are refused, so a confused peer cannot make the
/// other side allocate without bound.
const MAX_FRAME: u32 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    Json,
    Stdout,
    Stderr,
}

impl FrameKind {
    fn tag(self) -> u8 {
        match self {
            FrameKind::Json => b'J',
            FrameKind::Stdout => b'1',
            FrameKind::Stderr => b'2',
        }
    }

    fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            b'J' => Some(FrameKind::Json),
            b'1' => Some(FrameKind::Stdout),
            b'2' => Some(FrameKind::Stderr),
            _ => None,
        }
    }
}

pub fn write_frame(writer: &mut impl Write, kind: FrameKind, payload: &[u8]) -> io::Result<()> {
    let length = u32::try_from(payload.len())
        .ok()
        .filter(|length| *length <= MAX_FRAME)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    let mut header = [0; 5];
    header[0] = kind.tag();
    header[1..].copy_from_slice(&length.to_be_bytes());
    writer.write_all(&header)?;
    writer.write_all(payload)?;
    writer.flush()
}

/// The next frame, or `None` when the peer closed the connection cleanly
/// between frames.
pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<(FrameKind, Vec<u8>)>> {
    let mut tag = [0; 1];
    match reader.read_exact(&mut tag) {
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        result => result?,
    }
    let kind = FrameKind::from_tag(tag[0])
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown frame"))?;
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length);
    if length > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut payload = vec![0; length as usize];
    reader.read_exact(&mut payload)?;
    Ok(Some((kind, payload)))
}

pub fn write_json(writer: &mut impl Write, message: &impl Serialize) -> io::Result<()> {
    let payload = serde_json::to_vec(message).map_err(io::Error::other)?;
    write_frame(writer, FrameKind::Json, &payload)
}

pub fn parse_json<T: DeserializeOwned>(payload: &[u8]) -> io::Result<T> {
    serde_json::from_slice(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// The first frame of every connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u32,
    pub request: Request,
}

/// The version alone, read before the request: a request this build does
/// not know is then a version mismatch rather than a puzzle.
#[derive(Deserialize)]
pub struct VersionOnly {
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    List {
        query: Option<String>,
    },
    ShowHost {
        host: String,
    },
    /// Run `command` on the host over a connection of its own. Its output
    /// comes as stdout and stderr frames, then [`Reply::Exit`].
    Exec {
        host: String,
        command: String,
    },
    /// `source` is absolute: the app's working directory is not the
    /// caller's.
    Upload {
        host: String,
        source: PathBuf,
        destination: String,
    },
    Download {
        host: String,
        source: String,
        destination: PathBuf,
    },
    /// Copy what the local folder `source` holds into `destination` on the
    /// host, skipping what has not changed; with `delete`, remove what is
    /// there and not here first.
    Sync {
        host: String,
        source: PathBuf,
        destination: String,
        #[serde(default)]
        delete: bool,
    },
    CreateHost {
        fields: HostFields,
    },
    UpdateHost {
        host: String,
        fields: HostFields,
    },
    /// The host's open cards stay as they are.
    DeleteHost {
        host: String,
    },
    ListCredentials {
        query: Option<String>,
    },
    ShowCredential {
        credential: String,
    },
    CreateCredential {
        fields: CredentialFields,
    },
    UpdateCredential {
        credential: String,
        fields: CredentialFields,
    },
    DeleteCredential {
        credential: String,
    },
    /// The remote cards the user handed to the agent (Agent CLI 工单 06).
    ListCards,
    /// `exec --card`: type `command` into a card the user handed to the
    /// agent. Its output comes as stdout, then [`Reply::Context`], then
    /// [`Reply::Exit`].
    ExecInCard {
        card: String,
        command: String,
    },
    /// The last `lines` logical lines of a handed card's screen and
    /// scrollback.
    ReadCard {
        card: String,
        #[serde(default)]
        lines: Option<usize>,
    },
    /// A named key (`ctrl-c`, `q`, `esc`, …) typed into a handed card.
    SendKey {
        card: String,
        key: String,
        #[serde(default)]
        repeat: Option<usize>,
    },
    /// An agent's status hook (`micaterm hook`, Agent CLI 工单 09): what the
    /// agent in local card `card` (its `MICATERM_SELF_CARD`) is doing. The
    /// caller does not wait for the reply.
    Hook {
        card: String,
        event: crate::agent::HookEvent,
        #[serde(default)]
        message: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    Hosts {
        hosts: Vec<HostInfo>,
    },
    /// A host as it is now.
    Host(Box<HostDetails>),
    /// The remote command ended with this exit code.
    Exit {
        code: i32,
    },
    /// How far a transfer has come.
    Progress(TransferCounters),
    TransferDone(TransferSummary),
    HostDeleted(HostDeleted),
    Credentials {
        credentials: Vec<CredentialDetails>,
    },
    Credential(Box<CredentialDetails>),
    CredentialDeleted(CredentialDeleted),
    Cards {
        cards: Vec<CardInfo>,
    },
    /// Where an `exec --card` command runs, once it started.
    Context(TerminalContext),
    Screen(CardScreen),
    KeySent {
        key: String,
        repeat: usize,
    },
    /// A status hook was taken.
    Accepted,
    Error {
        code: ErrorCode,
        message: String,
    },
}

/// A host as the CLI lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    /// The host's public ID, the one 复制 ID copies.
    pub id: String,
    pub name: String,
    pub user: String,
    pub host: String,
    pub port: u16,
    /// The detected operating system, as stored (`ubuntu`, `macos`, …).
    pub os: Option<String>,
}

impl HostInfo {
    /// `user@host:port`.
    pub fn address(&self) -> String {
        format!("{}@{}:{}", self.user, self.host, self.port)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferCounters {
    pub files: u64,
    pub total_files: u64,
    pub bytes: u64,
    pub total_bytes: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferSummary {
    pub files: u64,
    pub bytes: u64,
    /// For a sync, the files left alone because they had not changed.
    pub skipped: u64,
    pub failed: u64,
    /// What a sync with `--delete` removed from the host.
    #[serde(default)]
    pub deleted: u64,
    /// Why each failed item failed.
    pub failures: Vec<String>,
}

/// A remote card the user handed to the agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardInfo {
    /// The card's ID for the CLI, new each time the card is handed over.
    pub id: String,
    /// The ID of the host the card is connected to.
    pub host: String,
    pub host_name: String,
    pub connected: bool,
    /// An `exec --card` command is running there now.
    pub running: bool,
}

/// Where an `exec --card` command ran: the card may have gone to another
/// user, folder or machine since it was opened.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalContext {
    pub user: String,
    pub host: String,
    pub cwd: String,
    /// The interactive shell the command was typed into.
    pub shell: String,
    /// What ran the command: `bash`, or `sh` where there is none.
    pub interp: String,
}

/// What `cards read` read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardScreen {
    pub text: String,
    pub returned_lines: usize,
    /// How many lines of scrollback the card still holds.
    pub history_available: usize,
    /// Earlier lines, or bytes past the limit, were left out.
    pub truncated: bool,
    /// A full-screen program has the screen: this is its screen.
    pub alternate_screen: bool,
}

/// Stable names for what went wrong, for an agent to act on whatever
/// language the message is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The app is not running. Only the command itself reports this.
    NotRunning,
    /// 启用外部 Agent is off.
    NotEnabled,
    VersionMismatch,
    HostNotFound,
    /// The host key is not in MicaTerm's known_hosts yet.
    HostKeyUnknown,
    /// The host key is not the one trusted before.
    HostKeyChanged,
    /// Logging in needs a password, passphrase or answer MicaTerm has not
    /// saved, or the saved one was refused.
    MissingCredential,
    /// What logging in needs is in the vault, and the vault is locked; or
    /// a change would write a secret into a locked vault.
    VaultLocked,
    /// A change would write a secret, and there is no vault that can take
    /// it yet: none set up, or one from another computer.
    VaultUnavailable,
    CredentialNotFound,
    /// The change was made, but not all of it could be written to disk or
    /// the vault.
    SaveFailed,
    /// The command could not talk to the app, the app hung up, or the
    /// connection to the host failed.
    ConnectFailed,
    /// A transfer could not start or did not finish.
    TransferFailed,
    /// `exec --card`, `cards read`, `cards send-key`: no handed card has
    /// this ID (never handed, taken back, or closed).
    NoTerminal,
    /// The card is not connected.
    NotConnected,
    /// The card could not take the command: a full-screen program has it,
    /// another command still runs there, or the shell did not answer.
    TerminalBusy,
    /// The card's shell is not bash, dash or busybox ash.
    UnsupportedShell,
    /// The command line is longer than the shell takes.
    TooLong,
    BadRequest,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::NotRunning => "not_running",
            ErrorCode::NotEnabled => "not_enabled",
            ErrorCode::VersionMismatch => "version_mismatch",
            ErrorCode::HostNotFound => "host_not_found",
            ErrorCode::HostKeyUnknown => "host_key_unknown",
            ErrorCode::HostKeyChanged => "host_key_changed",
            ErrorCode::MissingCredential => "missing_credential",
            ErrorCode::VaultLocked => "vault_locked",
            ErrorCode::VaultUnavailable => "vault_unavailable",
            ErrorCode::CredentialNotFound => "credential_not_found",
            ErrorCode::SaveFailed => "save_failed",
            ErrorCode::ConnectFailed => "connect_failed",
            ErrorCode::TransferFailed => "transfer_failed",
            ErrorCode::NoTerminal => "no_terminal",
            ErrorCode::NotConnected => "not_connected",
            ErrorCode::TerminalBusy => "terminal_busy",
            ErrorCode::UnsupportedShell => "unsupported_shell",
            ErrorCode::TooLong => "too_long",
            ErrorCode::BadRequest => "bad_request",
        }
    }
}

/// A failure with its code, as the app reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliError {
    pub code: ErrorCode,
    pub message: String,
}

impl CliError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// A password, passphrase or private key on its way to the app, never the
/// other way. `Debug` leaves it out, so a request can be printed or
/// compared in a test without it showing.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(***)")
    }
}

impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

/// For a field that can be left out, set or set to `null`, three different
/// things: `None` when left out, `Some(None)` for `null`.
fn present<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// How a host logs in, as the CLI spells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthChoice {
    Password,
    Credential,
    NoPassword,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyChoice {
    #[default]
    Http,
    Socks5,
}

/// A saved host in full, as `hosts show` prints it. Never a secret: only
/// whether one is saved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDetails {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: AuthChoice,
    /// The credential's ID, for a host that logs in with one.
    pub credential: Option<String>,
    /// For a host with its own password, whether one is saved in the vault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_saved: Option<bool>,
    pub route: RouteDetails,
    pub notes: String,
    pub os: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RouteDetails {
    Direct,
    /// The jump hosts' IDs in order; `None` for one that was deleted.
    Jump {
        hosts: Vec<Option<String>>,
    },
    Proxy {
        kind: ProxyChoice,
        host: String,
        port: u16,
        user: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password_saved: Option<bool>,
    },
}

/// What `hosts create` sets and `hosts update` changes: every field may be
/// left out (`update` leaves it as it is), and those that can be empty may
/// be `null`. What `hosts show` prints and nobody sets is taken out before
/// these are read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, alias = "address", skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthChoice>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub credential: Option<Option<String>>,
    /// `null` deletes the saved one.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub password: Option<Option<Secret>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<RouteFields>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// A route as `hosts create` and `hosts update` take it: always whole.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RouteFields {
    Direct,
    Jump {
        hosts: Vec<Option<String>>,
    },
    Proxy {
        #[serde(default)]
        kind: ProxyChoice,
        host: String,
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user: Option<String>,
        /// `null` deletes the saved one.
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        password: Option<Option<Secret>>,
    },
}

/// Deleted, with what went with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDeleted {
    pub host: HostDetails,
    /// Other hosts that went through it, which now have a deleted jump
    /// host in its place.
    pub jump_users: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKindChoice {
    Password,
    Key,
    Agent,
}

/// A saved credential, as `credentials show` prints it. Never a secret:
/// only whether one is saved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialDetails {
    pub id: String,
    pub name: String,
    pub kind: CredentialKindChoice,
    pub user: String,
    /// A key credential's public key, `ssh-ed25519 AAAA… comment`; not a
    /// secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    /// The IDs of the hosts that log in with it.
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_saved: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_saved: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase_saved: Option<bool>,
}

/// What `credentials create` sets and `credentials update` changes, as for
/// [`HostFields`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<CredentialKindChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// A password credential's; `null` deletes the saved one.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub password: Option<Option<Secret>>,
    /// A private key file on this machine, absolute by the time it reaches
    /// the app: read once into the vault, as the credential form's 本机文件.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<PathBuf>,
    /// The private key's text instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<Secret>,
    /// The key's; `null` deletes the saved one.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub passphrase: Option<Option<Secret>>,
}

/// Deleted; the hosts that logged in with it now log in on their own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialDeleted {
    pub credential: CredentialDetails,
    pub released: Vec<HostInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_including_binary_output() {
        let mut wire = Vec::new();
        write_json(
            &mut wire,
            &Envelope {
                version: PROTOCOL_VERSION,
                request: Request::ShowHost {
                    host: "VmLkf1snMOuPKJ07".into(),
                },
            },
        )
        .unwrap();
        write_frame(&mut wire, FrameKind::Stderr, &[0, 159, 146, 150, b'\n']).unwrap();

        let mut reader = wire.as_slice();
        let (kind, payload) = read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(kind, FrameKind::Json);
        let envelope: Envelope = parse_json(&payload).unwrap();
        assert_eq!(
            envelope.request,
            Request::ShowHost {
                host: "VmLkf1snMOuPKJ07".into(),
            }
        );
        assert_eq!(
            read_frame(&mut reader).unwrap(),
            Some((FrameKind::Stderr, vec![0, 159, 146, 150, b'\n']))
        );
        // A clean close between frames.
        assert_eq!(read_frame(&mut reader).unwrap(), None);
    }

    #[test]
    fn a_truncated_or_foreign_frame_is_an_error() {
        let mut wire = Vec::new();
        write_frame(&mut wire, FrameKind::Stdout, b"hello").unwrap();
        wire.truncate(wire.len() - 1);
        assert!(read_frame(&mut wire.as_slice()).is_err());
        assert!(read_frame(&mut &b"X\0\0\0\0"[..]).is_err());
        assert!(read_frame(&mut &b"J\xff\xff\xff\xff"[..]).is_err());
    }

    #[test]
    fn error_codes_are_spelled_as_documented() {
        for code in [
            ErrorCode::NotRunning,
            ErrorCode::NotEnabled,
            ErrorCode::VersionMismatch,
            ErrorCode::HostNotFound,
            ErrorCode::HostKeyUnknown,
            ErrorCode::HostKeyChanged,
            ErrorCode::MissingCredential,
            ErrorCode::VaultLocked,
            ErrorCode::VaultUnavailable,
            ErrorCode::CredentialNotFound,
            ErrorCode::SaveFailed,
            ErrorCode::ConnectFailed,
            ErrorCode::TransferFailed,
            ErrorCode::NoTerminal,
            ErrorCode::NotConnected,
            ErrorCode::TerminalBusy,
            ErrorCode::UnsupportedShell,
            ErrorCode::TooLong,
            ErrorCode::BadRequest,
        ] {
            assert_eq!(
                serde_json::to_string(&code).unwrap(),
                format!("\"{}\"", code.as_str())
            );
        }
    }

    #[test]
    fn a_secret_never_shows_in_debug_and_null_differs_from_left_out() {
        let fields: HostFields =
            serde_json::from_str(r#"{"password": "hunter2", "credential": null}"#).unwrap();
        assert!(!format!("{fields:?}").contains("hunter2"));
        assert_eq!(
            fields
                .password
                .as_ref()
                .map(|password| password.as_ref().map(Secret::expose)),
            Some(Some("hunter2"))
        );
        assert_eq!(fields.credential, Some(None));
        assert_eq!(HostFields::default().credential, None);
        assert!(serde_json::from_str::<HostFields>(r#"{"group": "x"}"#).is_err());
    }

    #[test]
    fn requests_are_spelled_as_the_skill_documents_them() {
        let list = Envelope {
            version: PROTOCOL_VERSION,
            request: Request::List {
                query: Some("web".into()),
            },
        };
        assert_eq!(
            serde_json::to_string(&list).unwrap(),
            r#"{"version":1,"request":{"type":"list","query":"web"}}"#
        );
    }
}
