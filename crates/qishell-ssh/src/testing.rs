// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-ssh/src/testing.rs
// 裁剪：本 crate 自己的测试也用它（`cfg(test)`）。
// 改造：也接受公钥登录（`TestServer::authorize`），给私钥登录的测试用；
// 也能当跳板（转发 direct-tcpip 通道，记在 `Seen::forwards`），给跳板的测试用（工单 15）。
//! A real SSH server in the test's own process, for this crate's tests and
//! those of what is built on it: a password login, a login with the keys it
//! is told to accept, a shell that echoes what is typed, commands, and SFTP through this machine's OpenSSH
//! `sftp-server` (none on Windows). Only in tests or with the `testing`
//! feature.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey, PublicKey};
use russh::server::{self, Server as _};
use russh::{ChannelId, MethodKind, MethodSet};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::task::JoinHandle;

pub const TEST_USER: &str = "tester";
pub const TEST_PASSWORD: &str = "fixture-password";

static FORKS: Mutex<()> = Mutex::new(());

/// Hold while starting a process, or while making a socket that must refuse
/// connections once it is closed: on macOS a process started in between
/// keeps a copy of the socket, and a closed listener still takes
/// connections. As the app's tests do.
pub fn no_forks() -> MutexGuard<'static, ()> {
    FORKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A port on 127.0.0.1 nothing listens on: connecting is refused.
pub fn closed_port() -> u16 {
    let _forks = no_forks();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("loopback");
    listener.local_addr().expect("bound").port()
}

/// The server, until dropped.
pub struct TestServer {
    pub port: u16,
    seen: Arc<Mutex<Seen>>,
    replies: Replies,
    keys: AuthorizedKeys,
    task: JoinHandle<()>,
}

/// The public keys [`TEST_USER`] logs in with, as `authorized_keys`.
type AuthorizedKeys = Arc<Mutex<Vec<PublicKey>>>;

/// What a command prints, when not itself.
type Replies = Arc<Mutex<HashMap<String, String>>>;

/// What the server was told.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// The terminal's size in cells: from the pty request, then from the
    /// last window change.
    pub size: Option<(u32, u32)>,
    pub commands: Vec<String>,
    /// 当作跳板时，每个 direct-tcpip 通道要去的 `(host, port)`，按顺序（工单 15）。
    pub forwards: Vec<(String, u32)>,
}

impl TestServer {
    /// Start on the current runtime, serving SFTP in `directory`.
    pub async fn start(directory: PathBuf) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let key = PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519)
            .map_err(std::io::Error::other)?;
        let config = Arc::new(server::Config {
            methods: MethodSet::from(&[MethodKind::Password, MethodKind::PublicKey][..]),
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let replies = Replies::default();
        let keys = AuthorizedKeys::default();
        let mut server = Listener {
            directory,
            seen: seen.clone(),
            replies: replies.clone(),
            keys: keys.clone(),
        };
        let task = tokio::spawn(async move {
            let _ = server.run_on_socket(config, &listener).await;
        });
        Ok(Self {
            port,
            seen,
            replies,
            keys,
            task,
        })
    }

    pub fn seen(&self) -> Seen {
        lock(&self.seen).clone()
    }

    /// From now on [`TEST_USER`] may log in with the private key of `key`.
    pub fn authorize(&self, key: PublicKey) {
        lock(&self.keys).push(key);
    }

    /// From now on `command` prints `output` rather than itself.
    pub fn reply(&self, command: &str, output: &str) {
        lock(&self.replies).insert(command.into(), output.into());
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Listener {
    directory: PathBuf,
    seen: Arc<Mutex<Seen>>,
    replies: Replies,
    keys: AuthorizedKeys,
}

impl server::Server for Listener {
    type Handler = Handler;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Handler {
        Handler {
            directory: self.directory.clone(),
            seen: self.seen.clone(),
            replies: self.replies.clone(),
            keys: self.keys.clone(),
            channels: HashMap::new(),
            shells: HashSet::new(),
        }
    }
}

struct Handler {
    directory: PathBuf,
    seen: Arc<Mutex<Seen>>,
    replies: Replies,
    keys: AuthorizedKeys,
    channels: HashMap<ChannelId, russh::Channel<server::Msg>>,
    /// The channels running the shell: russh hands every channel's data to
    /// `data`, SFTP's too.
    shells: HashSet<ChannelId>,
}

impl server::Handler for Handler {
    type Error = anyhow::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> anyhow::Result<server::Auth> {
        Ok(if user == TEST_USER && password == TEST_PASSWORD {
            server::Auth::Accept
        } else {
            // As OpenSSH: asked again, not dropped.
            server::Auth::Reject {
                proceed_with_methods: Some(MethodSet::from(
                    &[MethodKind::Password, MethodKind::PublicKey][..],
                )),
                partial_success: false,
            }
        })
    }

    /// Called once the client proved it holds the key.
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> anyhow::Result<server::Auth> {
        Ok(
            if user == TEST_USER && lock(&self.keys).iter().any(|known| known == key) {
                server::Auth::Accept
            } else {
                server::Auth::Reject {
                    proceed_with_methods: Some(MethodSet::from(
                        &[MethodKind::Password, MethodKind::PublicKey][..],
                    )),
                    partial_success: false,
                }
            },
        )
    }

    async fn channel_open_session(
        &mut self,
        channel: russh::Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> anyhow::Result<()> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    /// 当作跳板：连到要去的地方，把通道和那个 socket 接起来；连不上就拒绝通道（工单 15）。
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<server::Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> anyhow::Result<()> {
        lock(&self.seen)
            .forwards
            .push((host_to_connect.to_string(), port_to_connect));
        let Ok(port) = u16::try_from(port_to_connect) else {
            reply.reject(russh::ChannelOpenFailure::ConnectFailed).await;
            return Ok(());
        };
        match tokio::net::TcpStream::connect((host_to_connect, port)).await {
            Ok(mut socket) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
                });
            }
            Err(_) => reply.reject(russh::ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _: &str,
        columns: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> anyhow::Result<()> {
        lock(&self.seen).size = Some((columns, rows));
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> anyhow::Result<()> {
        self.shells.insert(channel);
        session.channel_success(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _: ChannelId,
        columns: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &mut server::Session,
    ) -> anyhow::Result<()> {
        lock(&self.seen).size = Some((columns, rows));
        Ok(())
    }

    /// The shell echoes what is typed; `exit` and a newline end it with
    /// status 3.
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> anyhow::Result<()> {
        if !self.shells.contains(&channel) {
            return Ok(());
        }
        if data == b"exit\n" {
            session.exit_status_request(channel, 3)?;
            session.eof(channel)?;
            session.close(channel)?;
        } else {
            session.data(channel, data.to_vec())?;
        }
        Ok(())
    }

    /// A command prints itself, or its reply, and succeeds.
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        command: &[u8],
        session: &mut server::Session,
    ) -> anyhow::Result<()> {
        let command = String::from_utf8_lossy(command).into_owned();
        lock(&self.seen).commands.push(command.clone());
        let output = lock(&self.replies)
            .get(&command)
            .cloned()
            .unwrap_or_else(|| format!("{command}\n"));
        session.channel_success(channel)?;
        session.data(channel, output.into_bytes())?;
        session.exit_status_request(channel, 0)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> anyhow::Result<()> {
        let Some(stream) = self.channels.remove(&channel).filter(|_| name == "sftp") else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        let Some(executable) = sftp_server() else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        session.channel_success(channel)?;
        let mut child = {
            let _forks = no_forks();
            tokio::process::Command::new(executable)
                .current_dir(&self.directory)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()?
        };
        let (mut input, mut output) = (
            child.stdin.take().expect("piped"),
            child.stdout.take().expect("piped"),
        );
        let (mut reader, mut writer) = tokio::io::split(stream.into_stream());
        tokio::spawn(async move {
            tokio::select! {
                _ = async {
                    tokio::io::copy(&mut reader, &mut input).await?;
                    input.shutdown().await
                } => {}
                _ = tokio::io::copy(&mut output, &mut writer) => {}
            }
            let _ = writer.shutdown().await;
            let _ = child.kill().await;
            let mut rest = Vec::new();
            let _ = output.read_to_end(&mut rest).await;
        });
        Ok(())
    }
}

/// Whether this machine has OpenSSH's `sftp-server` to serve SFTP with.
pub fn has_sftp_server() -> bool {
    sftp_server().is_some()
}

/// This machine's OpenSSH `sftp-server`, if it has one.
fn sftp_server() -> Option<&'static str> {
    [
        "/usr/libexec/sftp-server",
        "/usr/lib/openssh/sftp-server",
        "/usr/lib/ssh/sftp-server",
    ]
    .into_iter()
    .find(|path| std::path::Path::new(path).exists())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
