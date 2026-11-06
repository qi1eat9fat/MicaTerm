// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-ssh/src/testing.rs
// 裁剪：本 crate 自己的测试也用它（`cfg(test)`）。
// 改造：也接受公钥登录（`TestServer::authorize`），给私钥登录的测试用；
// 也能当跳板（转发 direct-tcpip 通道，记在 `Seen::forwards`），给跳板的测试用（工单 15）；
// 也能注入 SFTP 的故障（`Faults`，来自 shell-rs 的 `src/sftp/protocol_tests.rs`，工单 21）。
//! A real SSH server in the test's own process, for this crate's tests and
//! those of what is built on it: a password login, a login with the keys it
//! is told to accept, a shell that echoes what is typed, commands, and SFTP through this machine's OpenSSH
//! `sftp-server` (on Windows, the one Git for Windows carries). Only in tests
//! or with the `testing` feature.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey, PublicKey};
use russh::server::{self, Server as _};
use russh::{ChannelId, MethodKind, MethodSet};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
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
    single_channel: Arc<AtomicBool>,
    faults: Arc<Faults>,
    task: JoinHandle<()>,
}

/// 注入的故障和数的数，服务器和每条连接共用。
#[derive(Default)]
struct Faults {
    connections: AtomicUsize,
    /// 结束了的连接。
    ended: AtomicUsize,
    shells: AtomicUsize,
    /// 还要掐断几次传输。
    interrupts: AtomicUsize,
    /// 什么都没问时结束 SFTP 会话。
    hangup: tokio::sync::Notify,
    refuse_sftp: AtomicBool,
    bastion: AtomicBool,
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
        let single_channel = Arc::new(AtomicBool::new(false));
        let faults = Arc::new(Faults::default());
        let mut server = Listener {
            directory,
            seen: seen.clone(),
            replies: replies.clone(),
            keys: keys.clone(),
            single_channel: single_channel.clone(),
            faults: faults.clone(),
        };
        let task = tokio::spawn(async move {
            let _ = server.run_on_socket(config, &listener).await;
        });
        Ok(Self {
            port,
            seen,
            replies,
            keys,
            single_channel,
            faults,
            task,
        })
    }

    /// From now on every connection opens one session channel only and
    /// refuses the next ones, as some bastion hosts do.
    pub fn single_channel(&self) {
        self.single_channel.store(true, Ordering::SeqCst);
    }

    /// 接下来 `count` 个 SFTP 会话，各在第二个读或写请求上断开。
    pub fn interrupt_transfers(&self, count: usize) {
        self.faults.interrupts.store(count, Ordering::SeqCst);
    }

    /// 结束现在的 SFTP 会话，虽然没人问它什么。
    pub fn hang_up(&self) {
        self.faults.hangup.notify_one();
    }

    /// 从现在起不提供 SFTP 子系统。
    pub fn refuse_sftp(&self) {
        self.faults.refuse_sftp.store(true, Ordering::SeqCst);
    }

    /// 从现在起收到堡垒机拒绝的请求就结束 SFTP 会话，见 [`refused_by_bastion`]。
    pub fn act_as_bastion(&self) {
        self.faults.bastion.store(true, Ordering::SeqCst);
    }

    /// 连进来过几条连接。
    pub fn connections(&self) -> usize {
        self.faults.connections.load(Ordering::SeqCst)
    }

    /// 结束了几条连接。
    pub fn ended(&self) -> usize {
        self.faults.ended.load(Ordering::SeqCst)
    }

    /// 要过几次 shell。
    pub fn shells(&self) -> usize {
        self.faults.shells.load(Ordering::SeqCst)
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
    single_channel: Arc<AtomicBool>,
    faults: Arc<Faults>,
}

impl server::Server for Listener {
    type Handler = Handler;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Handler {
        self.faults.connections.fetch_add(1, Ordering::SeqCst);
        Handler {
            directory: self.directory.clone(),
            seen: self.seen.clone(),
            replies: self.replies.clone(),
            keys: self.keys.clone(),
            single_channel: self.single_channel.clone(),
            faults: self.faults.clone(),
            sessions: 0,
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
    single_channel: Arc<AtomicBool>,
    faults: Arc<Faults>,
    /// The session channels this connection opened.
    sessions: usize,
    channels: HashMap<ChannelId, russh::Channel<server::Msg>>,
    /// The channels running the shell: russh hands every channel's data to
    /// `data`, SFTP's too.
    shells: HashSet<ChannelId>,
}

impl Drop for Handler {
    fn drop(&mut self) {
        self.faults.ended.fetch_add(1, Ordering::SeqCst);
    }
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
        if self.sessions > 0 && self.single_channel.load(Ordering::SeqCst) {
            reply
                .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }
        self.sessions += 1;
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
        self.faults.shells.fetch_add(1, Ordering::SeqCst);
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
        let refused = self.faults.refuse_sftp.load(Ordering::SeqCst);
        let Some(stream) = self
            .channels
            .remove(&channel)
            .filter(|_| name == "sftp" && !refused)
        else {
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
            let mut command = tokio::process::Command::new(executable);
            // Git 的 `sftp-server.exe` 要 `usr\\bin` 里的 `msys-2.0.dll`。PATH 上常常只有 `cmd`，
            // 那样进程一启动就退出，看起来是通道断了。
            #[cfg(windows)]
            if let Some(usr) = executable.ancestors().nth(3) {
                let path = std::env::var_os("PATH").unwrap_or_default();
                let paths = std::iter::once(usr.join("bin")).chain(std::env::split_paths(&path));
                if let Ok(paths) = std::env::join_paths(paths) {
                    command.env("PATH", paths);
                }
            }
            command
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
        let faults = self.faults.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = forward_requests(&mut reader, &mut input, &faults) => {}
                _ = tokio::io::copy(&mut output, &mut writer) => {}
                _ = faults.hangup.notified() => {}
            }
            let _ = writer.shutdown().await;
            let _ = child.kill().await;
            let mut rest = Vec::new();
            let _ = output.read_to_end(&mut rest).await;
        });
        Ok(())
    }
}

/// 把客户端的 SFTP 请求一个个交给 `sftp-server`，按 `faults` 在半路断开。对面关了就关 `input`。
async fn forward_requests(
    reader: &mut (impl AsyncRead + Unpin),
    input: &mut (impl AsyncWrite + Unpin),
    faults: &Faults,
) -> std::io::Result<()> {
    let mut transfers = 0;
    loop {
        let Ok(size) = reader.read_u32().await else {
            return input.shutdown().await;
        };
        if size > 256 * 1024 {
            return Err(std::io::Error::other("invalid frame"));
        }
        let mut bytes = vec![0; size as usize];
        reader.read_exact(&mut bytes).await?;
        if faults.bastion.load(Ordering::SeqCst) && refused_by_bastion(&bytes) {
            return Err(std::io::Error::other("refused by the bastion host"));
        }
        // SSH_FXP_READ (5) 是下载，SSH_FXP_WRITE (6) 是上传。
        if matches!(bytes.first(), Some(5 | 6)) {
            transfers += 1;
            if transfers == 2
                && faults
                    .interrupts
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    .is_ok()
            {
                return Err(std::io::Error::other("injected disconnect"));
            }
        }
        input.write_u32(size).await?;
        input.write_all(&bytes).await?;
        input.flush().await?;
    }
}

/// 见过的堡垒机拒绝的请求，虽然它转发了 OpenSSH 提供的扩展：`SSH_FXP_FSETSTAT` (10) 回失败，
/// `fsync@openssh.com` 直接断开会话。这里所有 `SSH_FXP_EXTENDED` (200) 都拒绝：
/// 和 WinSCP 一样，传文件和保存都不用它们。
fn refused_by_bastion(packet: &[u8]) -> bool {
    matches!(packet.first(), Some(10 | 200))
}

/// Whether this machine has OpenSSH's `sftp-server` to serve SFTP with.
pub fn has_sftp_server() -> bool {
    sftp_server().is_some()
}

/// This machine's OpenSSH `sftp-server`, if it has one. On Windows, the one
/// Git for Windows carries, wherever Git is: its paths come out in MSYS form
/// (`/e/...`), so treat them as opaque.
fn sftp_server() -> Option<&'static std::path::Path> {
    static SERVER: OnceLock<Option<PathBuf>> = OnceLock::new();
    SERVER
        .get_or_init(|| {
            [
                "/usr/libexec/sftp-server",
                "/usr/libexec/openssh/sftp-server",
                "/usr/lib/openssh/sftp-server",
                "/usr/lib/ssh/sftp-server",
            ]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.exists())
            .or_else(git_sftp_server)
        })
        .as_deref()
}

/// `<git>\usr\lib\ssh\sftp-server.exe` of the first Git on the `PATH`, which
/// keeps `git.exe` in `cmd`, `bin` or `mingw64\bin`.
fn git_sftp_server() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| dir.join("git.exe").exists())
        .find_map(|dir| {
            dir.ancestors()
                .map(|root| root.join(r"usr\lib\ssh\sftp-server.exe"))
                .find(|server| server.exists())
        })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
