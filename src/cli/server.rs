// 来自 since2006/shell-rs @ b2a307a: src/cli/server.rs
// 裁剪（Agent CLI 工单 01）：只答 `hosts list` / `hosts show`（工单 02 加回后端和 `exec`，03 加回传输，
// 04 加回凭据和改动队列，06 把 `exec --terminal` 改成只认交给 Agent 的卡片的 `exec --card`）；
// 去掉单实例唤起、应用锁和匿名统计。
// 主机和凭据的快照由窗口壳在 UI 线程上算好交进来（`set_targets`、`set_credentials`），
// 「存没存密码」也在那时问好。

//! The app's end of the CLI socket. Listens for as long as the app runs and
//! answers each connection on a thread of its own; the work itself is the
//! backend's, so tests can put a fake behind the same server.

use std::{
    collections::VecDeque,
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};

#[cfg(unix)]
use tokio::sync::watch;

use super::hosts::CliTarget;
use super::manage::{CliChange, credential_matches, credential_not_found, host_not_found};
use super::protocol::{
    CardInfo, CliError, CredentialDetails, Envelope, ErrorCode, FrameKind, PROTOCOL_VERSION, Reply,
    Request, TerminalContext, TransferCounters, TransferSummary, VersionOnly, parse_json,
    read_frame, write_frame, write_json,
};
use crate::agent::HookEvent;
use crate::i18n::t;
use micaterm_ssh::ExecStream;
use micaterm_terminal::{
    BusyReason, RunEvent, RunFailure, RunHandle, RunProgress, RunTiming, TerminalRun,
};

/// Does what a CLI request asks. Every method blocks: each request has a
/// thread of its own.
pub trait CliBackend: Send + Sync + 'static {
    /// Run `command` on the host, passing its output on as it comes; the
    /// remote exit code. When `output` fails, the caller is gone and the
    /// command should be abandoned.
    fn exec(
        &self,
        target: &CliTarget,
        command: &str,
        output: &mut dyn FnMut(ExecStream, &[u8]) -> io::Result<()>,
    ) -> Result<i32, CliError>;

    /// Copy the local `source` to `destination` on the host, the way scp
    /// does. When `progress` fails, the caller is gone and the transfer
    /// should stop.
    fn upload(
        &self,
        target: &CliTarget,
        source: &Path,
        destination: &str,
        progress: &mut dyn FnMut(TransferCounters) -> io::Result<()>,
    ) -> Result<TransferSummary, CliError>;

    /// Copy `source` on the host to the local `destination`, the way scp
    /// does.
    fn download(
        &self,
        target: &CliTarget,
        source: &str,
        destination: &Path,
        progress: &mut dyn FnMut(TransferCounters) -> io::Result<()>,
    ) -> Result<TransferSummary, CliError>;

    /// Bring the folder `destination` on the host in line with the local
    /// folder `source`: what changed is copied, what did not is left alone,
    /// and with `delete` what is not in `source` goes.
    fn sync(
        &self,
        target: &CliTarget,
        source: &Path,
        destination: &str,
        delete: bool,
        progress: &mut dyn FnMut(TransferCounters) -> io::Result<()>,
    ) -> Result<TransferSummary, CliError>;
}

/// What the listener and the request threads share with the app.
struct Shared {
    enabled: AtomicBool,
    targets: RwLock<Vec<CliTarget>>,
    backend: RwLock<Option<Arc<dyn CliBackend>>>,
    /// 有请求因为保险库锁着没做（工单 02）：窗口壳定时来取，提醒用户解锁。
    vault_wanted: AtomicBool,
    credentials: RwLock<Vec<CredentialDetails>>,
    /// Changes to hosts and credentials, oldest first, until the app takes
    /// them up: only the UI thread can change the store.
    changes: Mutex<VecDeque<PendingChange>>,
    next_change: AtomicU64,
    /// 交给 Agent 的卡片（工单 06），窗口壳定时交进来。
    cards: RwLock<Vec<CardInfo>>,
    /// `exec --card` runs, oldest first, until the app takes them up: only
    /// the UI thread can reach a card.
    runs: Mutex<VecDeque<PendingRun>>,
    /// 状态 hook 报来的事（工单 09），等 UI 线程取走：哪张卡片、什么事、带来的那句话。
    hooks: Mutex<VecDeque<(String, HookEvent, Option<String>)>>,
}

/// 排着没取走的状态 hook 最多留多少件。
const MAX_HOOKS: usize = 256;

/// An `exec --card` run waiting for the app to find the card.
struct PendingRun {
    id: u64,
    card: String,
    run: RunHandle,
}

/// How an `exec --card` run is paced; quick in the tests.
const RUN_TIMING: RunTiming = if cfg!(test) {
    RunTiming {
        settle: Duration::ZERO,
        probe: Duration::from_millis(300),
        start: Duration::from_millis(300),
        tick: Duration::from_millis(20),
    }
} else {
    RunTiming::STANDARD
};

/// How long a change may wait for the app to take it up. Only the wait in
/// line counts: once taken, the change is being made and its outcome is
/// waited for however long the vault takes, or an agent would try again
/// and make it twice.
const QUEUE_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(10)
};

/// Where the app sends the outcome of a change it took up. Dropping it
/// unsent tells the request it will not come.
pub struct ChangeReply(mpsc::Sender<Result<Reply, CliError>>);

impl ChangeReply {
    pub fn send(self, outcome: Result<Reply, CliError>) {
        let _ = self.0.send(outcome);
    }
}

/// A change waiting for the app, with where its outcome goes.
struct PendingChange {
    id: u64,
    change: CliChange,
    reply: ChangeReply,
}

/// Listens on the CLI socket until dropped. Always listening, even with
/// 启用外部 Agent off, so a caller hears "not enabled" rather than "not
/// running" and can tell the user which of the two to fix.
pub struct CliServer {
    shared: Arc<Shared>,
    _listener: Listener,
}

impl CliServer {
    /// Take over `endpoint` (from [`crate::app::cli_endpoint`]), which must
    /// not be another running MicaTerm's: then the error is
    /// [`io::ErrorKind::AddrInUse`]. Only the user running the app can
    /// connect.
    pub fn start(endpoint: PathBuf) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            enabled: AtomicBool::new(false),
            targets: RwLock::new(Vec::new()),
            backend: RwLock::new(None),
            vault_wanted: AtomicBool::new(false),
            credentials: RwLock::new(Vec::new()),
            changes: Mutex::new(VecDeque::new()),
            next_change: AtomicU64::new(0),
            cards: RwLock::new(Vec::new()),
            runs: Mutex::new(VecDeque::new()),
            hooks: Mutex::new(VecDeque::new()),
        });
        let listener = listen(endpoint, shared.clone())?;
        Ok(Self {
            shared,
            _listener: listener,
        })
    }

    /// Who carries out `exec`. Until there is one, `exec` is refused.
    pub fn set_backend(&self, backend: Arc<dyn CliBackend>) {
        *self
            .shared
            .backend
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(backend);
    }

    /// 自上次问过以来，有没有请求因为保险库锁着而没做。请求线程碰不到窗口，窗口壳定时来问。
    pub fn take_vault_wanted(&self) -> bool {
        self.shared.vault_wanted.swap(false, Ordering::AcqRel)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.shared.enabled.store(enabled, Ordering::Release);
        if !enabled {
            let changes = std::mem::take(
                &mut *self
                    .shared
                    .changes
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()),
            );
            for pending in changes {
                pending.reply.send(Err(not_enabled()));
            }
            let runs = std::mem::take(
                &mut *self
                    .shared
                    .runs
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()),
            );
            for pending in runs {
                pending.run.fail(RunFailure::NotConnected);
            }
            self.shared
                .hooks
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clear();
        }
    }

    /// 最早一件没取走的状态 hook（工单 09）：哪张卡片、什么事、带来的那句话。
    pub fn take_hook(&self) -> Option<(String, HookEvent, Option<String>)> {
        self.shared
            .hooks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
    }

    /// 交给 Agent 的卡片，窗口壳定时交进来。
    pub fn set_cards(&self, cards: Vec<CardInfo>) {
        *self
            .shared
            .cards
            .write()
            .unwrap_or_else(|error| error.into_inner()) = cards;
    }

    /// The oldest `exec --card` run waiting, with the card it is for, for
    /// the app to hand to that card, or say why it cannot.
    pub fn take_run(&self) -> Option<(String, RunHandle)> {
        self.shared
            .runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
            .and_then(|pending| {
                if self.shared.enabled.load(Ordering::Acquire) {
                    Some((pending.card, pending.run))
                } else {
                    pending.run.fail(RunFailure::NotConnected);
                    None
                }
            })
    }

    /// What the requests see of the credentials: the store as it is now.
    pub fn set_credentials(&self, credentials: Vec<CredentialDetails>) {
        *self
            .shared
            .credentials
            .write()
            .unwrap_or_else(|error| error.into_inner()) = credentials;
    }

    /// The oldest change waiting, for the app to make and answer. One at a
    /// time: the next is only taken once this one is answered.
    pub fn take_change(&self) -> Option<(CliChange, ChangeReply)> {
        self.shared
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
            .and_then(|pending| {
                if self.shared.enabled.load(Ordering::Acquire) {
                    Some((pending.change, pending.reply))
                } else {
                    pending.reply.send(Err(not_enabled()));
                    None
                }
            })
    }

    /// What the requests see of the hosts: the store as it is now. Called
    /// whenever the hosts or the vault change.
    pub fn set_targets(&self, targets: Vec<CliTarget>) {
        *self
            .shared
            .targets
            .write()
            .unwrap_or_else(|error| error.into_inner()) = targets;
    }
}

impl Drop for CliServer {
    /// The app is going: whatever waits for it to take up a change hears
    /// that it never will.
    fn drop(&mut self) {
        self.shared
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        for pending in self
            .shared
            .runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
        {
            pending.run.fail(RunFailure::NotConnected);
        }
    }
}

/// The listening thread, stopped when dropped. On Unix, the socket file
/// goes with it.
#[cfg(unix)]
struct Listener {
    socket: PathBuf,
    shutdown: watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        self.shutdown.send_replace(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// The socket is the user's alone, and every caller's user id is checked
/// again.
#[cfg(unix)]
fn listen(socket: PathBuf, shared: Arc<Shared>) -> io::Result<Listener> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    use std::os::unix::net::{UnixListener, UnixStream};

    if std::fs::symlink_metadata(&socket).is_ok() {
        if UnixStream::connect(&socket).is_ok() {
            return Err(another_app());
        }
        // Left behind by a MicaTerm that did not get to clean up.
        std::fs::remove_file(&socket)?;
    }
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let owner = std::fs::metadata(&socket)?.uid();
    listener.set_nonblocking(true)?;

    let (shutdown, mut stop) = watch::channel(false);
    let thread = std::thread::Builder::new()
        .name("micaterm-cli".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                let Ok(listener) = tokio::net::UnixListener::from_std(listener) else {
                    return;
                };
                loop {
                    tokio::select! {
                        accepted = listener.accept() => {
                            if let Ok((stream, _)) = accepted {
                                accept(stream, owner, &shared);
                            }
                        }
                        _ = stop.changed() => break,
                    }
                }
            });
        })?;
    Ok(Listener {
        socket,
        shutdown,
        thread: Some(thread),
    })
}

/// Hand a connection from the same user to a thread of its own.
#[cfg(unix)]
fn accept(stream: tokio::net::UnixStream, owner: u32, shared: &Arc<Shared>) {
    if !stream
        .peer_cred()
        .is_ok_and(|credentials| credentials.uid() == owner)
    {
        return;
    }
    let Ok(stream) = stream.into_std() else {
        return;
    };
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let shared = shared.clone();
    let _ = std::thread::Builder::new()
        .name("micaterm-cli-request".into())
        .spawn(move || {
            // A caller that hangs up mid-answer is no one's problem.
            let _ = serve(&mut BufReader::new(&stream), &mut &stream, &shared);
        });
}

/// The listening thread, stopped when dropped.
#[cfg(windows)]
struct Listener {
    pipe: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(windows)]
impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let Some(thread) = self.thread.take() else {
            return;
        };
        // The thread waits in ConnectNamedPipe; a caller wakes it. Until it
        // has gone: between two pipe instances there is nothing to connect
        // to, so one knock may not be heard.
        while !thread.is_finished() {
            let _ = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.pipe);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = thread.join();
    }
}

/// A named pipe only the current user can open, and only from this
/// machine. Its name is the machine's, not a file of the user's, so it is
/// created as the first instance: if anyone already holds it, the server
/// does not start.
#[cfg(windows)]
fn listen(pipe: PathBuf, shared: Arc<Shared>) -> io::Result<Listener> {
    use super::pipe_windows;

    let first = pipe_windows::create_instance(&pipe, true).map_err(|error| {
        if pipe_windows::is_taken(&error) {
            another_app()
        } else {
            error
        }
    })?;
    let stop = Arc::new(AtomicBool::new(false));
    let thread = std::thread::Builder::new()
        .name("micaterm-cli".into())
        .spawn({
            let (pipe, stop) = (pipe.clone(), stop.clone());
            move || {
                let mut waiting = first;
                loop {
                    let connected = pipe_windows::wait_for_client(&waiting);
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    // The next instance first, so a caller arriving now
                    // finds the pipe busy rather than gone.
                    let next = loop {
                        match pipe_windows::create_instance(&pipe, false) {
                            Ok(next) => break Some(next),
                            Err(_) if !stop.load(Ordering::Acquire) => {
                                std::thread::sleep(std::time::Duration::from_millis(100))
                            }
                            Err(_) => break None,
                        }
                    };
                    let Some(next) = next else {
                        break;
                    };
                    let current = std::mem::replace(&mut waiting, next);
                    // Otherwise the caller came and went already.
                    if connected.is_ok() {
                        answer(std::fs::File::from(current), &shared);
                    }
                }
            }
        })?;
    Ok(Listener {
        pipe,
        stop,
        thread: Some(thread),
    })
}

/// Answer a connected caller on a thread of its own. Closing the pipe
/// afterwards keeps what the caller has not read yet: only
/// `DisconnectNamedPipe` would throw it away.
#[cfg(windows)]
fn answer(pipe: std::fs::File, shared: &Arc<Shared>) {
    let shared = shared.clone();
    let _ = std::thread::Builder::new()
        .name("micaterm-cli-request".into())
        .spawn(move || {
            // A caller that hangs up mid-answer is no one's problem.
            let _ = serve(&mut BufReader::new(&pipe), &mut &pipe, &shared);
        });
}

#[cfg(not(any(unix, windows)))]
struct Listener;

#[cfg(not(any(unix, windows)))]
fn listen(_: PathBuf, _: Arc<Shared>) -> io::Result<Listener> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        t!("cli.unsupported").to_string(),
    ))
}

fn another_app() -> io::Error {
    io::Error::new(
        io::ErrorKind::AddrInUse,
        t!("cli.server.another_app").to_string(),
    )
}

/// Read one request and answer it.
fn serve(
    reader: &mut impl io::Read,
    writer: &mut impl io::Write,
    shared: &Shared,
) -> io::Result<()> {
    let Some((FrameKind::Json, payload)) = read_frame(reader)? else {
        return Ok(());
    };
    let reply = respond(&payload, writer, shared).unwrap_or_else(|error| Reply::Error {
        code: error.code,
        message: error.message,
    });
    write_json(writer, &reply)
}

/// Carry out one request. Output goes to `writer` as it comes; what comes
/// back is the reply that ends the connection, or the error sent in its
/// place.
fn respond(
    payload: &[u8],
    writer: &mut impl io::Write,
    shared: &Shared,
) -> Result<Reply, CliError> {
    let bad_request = |error: io::Error| CliError::new(ErrorCode::BadRequest, error.to_string());
    // The version before the request, which another version may spell in
    // a way this one cannot read.
    let version = parse_json::<VersionOnly>(payload).map_err(bad_request)?;
    if version.version != PROTOCOL_VERSION {
        return Err(CliError::new(
            ErrorCode::VersionMismatch,
            t!("cli.server.version_mismatch"),
        ));
    }
    let envelope = parse_json::<Envelope>(payload).map_err(bad_request)?;
    if !shared.enabled.load(Ordering::Acquire) {
        return Err(CliError::new(
            ErrorCode::NotEnabled,
            t!("cli.server.not_enabled"),
        ));
    }
    match envelope.request {
        Request::List { query } => Ok(Reply::Hosts {
            hosts: shared
                .targets
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .iter()
                .filter(|target| query.as_deref().is_none_or(|query| target.matches(query)))
                .map(|target| target.info.clone())
                .collect(),
        }),
        Request::ShowHost { host } => {
            find(shared, &host).map(|target| Reply::Host(Box::new(target.details)))
        }
        Request::ListCredentials { query } => Ok(Reply::Credentials {
            credentials: shared
                .credentials
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .iter()
                .filter(|credential| {
                    query
                        .as_deref()
                        .is_none_or(|query| credential_matches(credential, query))
                })
                .cloned()
                .collect(),
        }),
        Request::ShowCredential { credential } => shared
            .credentials
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .find(|known| known.id == credential.trim())
            .map(|found| Reply::Credential(Box::new(found.clone())))
            .ok_or_else(|| credential_not_found(&credential)),
        Request::CreateHost { fields } => change(shared, CliChange::CreateHost(fields)),
        Request::UpdateHost { host, fields } => {
            change(shared, CliChange::UpdateHost { host, fields })
        }
        Request::DeleteHost { host } => change(shared, CliChange::DeleteHost { host }),
        Request::CreateCredential { fields } => change(shared, CliChange::CreateCredential(fields)),
        Request::UpdateCredential { credential, fields } => {
            change(shared, CliChange::UpdateCredential { credential, fields })
        }
        Request::DeleteCredential { credential } => {
            change(shared, CliChange::DeleteCredential { credential })
        }
        Request::ListCards => Ok(Reply::Cards {
            cards: shared
                .cards
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
        }),
        Request::ExecInCard { card, command } => run_in_card(shared, writer, card, command),
        Request::ReadCard { card, lines } => change(shared, CliChange::ReadCard { card, lines }),
        Request::SendKey { card, key, repeat } => {
            change(shared, CliChange::SendKey { card, key, repeat })
        }
        Request::Hook {
            card,
            event,
            message,
        } => {
            let mut hooks = shared
                .hooks
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            // 应用一直没取（卡住了）时不无限堆：留最近的。
            if hooks.len() >= MAX_HOOKS {
                hooks.pop_front();
            }
            hooks.push_back((card, event, message));
            Ok(Reply::Accepted)
        }
        Request::Exec { host, command } => {
            let (target, backend) = connectable(shared, &host)?;
            let code = backend.exec(&target, &command, &mut |stream, bytes| {
                let kind = match stream {
                    ExecStream::Stdout => FrameKind::Stdout,
                    ExecStream::Stderr => FrameKind::Stderr,
                };
                write_frame(writer, kind, bytes)
            });
            // 快照之后才锁上、或者读秘密时才发现锁着（后端认出 `SecretsLocked`）：一样提醒。
            if matches!(&code, Err(error) if error.code == ErrorCode::VaultLocked) {
                shared.vault_wanted.store(true, Ordering::Release);
            }
            code.map(|code| Reply::Exit { code })
        }
        Request::Upload {
            host,
            source,
            destination,
        } => {
            let (target, backend) = connectable(shared, &host)?;
            if !source.is_absolute() {
                return Err(not_absolute(&source));
            }
            let done = backend.upload(&target, &source, &destination, &mut |counters| {
                write_json(writer, &Reply::Progress(counters))
            });
            transfer_done(shared, done)
        }
        Request::Download {
            host,
            source,
            destination,
        } => {
            let (target, backend) = connectable(shared, &host)?;
            if !destination.is_absolute() {
                return Err(not_absolute(&destination));
            }
            let done = backend.download(&target, &source, &destination, &mut |counters| {
                write_json(writer, &Reply::Progress(counters))
            });
            transfer_done(shared, done)
        }
        Request::Sync {
            host,
            source,
            destination,
            delete,
        } => {
            let (target, backend) = connectable(shared, &host)?;
            if !source.is_absolute() {
                return Err(not_absolute(&source));
            }
            let done = backend.sync(&target, &source, &destination, delete, &mut |counters| {
                write_json(writer, &Reply::Progress(counters))
            });
            transfer_done(shared, done)
        }
    }
}

/// Hand a change to the app and wait for its outcome. Taken back, so
/// certainly not made, when the app does not take it up in time.
fn change(shared: &Shared, change: CliChange) -> Result<Reply, CliError> {
    let gone = || {
        CliError::new(
            ErrorCode::ConnectFailed,
            t!("cli.server.closed_during_change"),
        )
    };
    let (sender, outcome) = mpsc::channel();
    let id = shared.next_change.fetch_add(1, Ordering::Relaxed);
    shared
        .changes
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push_back(PendingChange {
            id,
            change,
            reply: ChangeReply(sender),
        });
    match outcome.recv_timeout(QUEUE_TIMEOUT) {
        Ok(outcome) => return outcome,
        Err(mpsc::RecvTimeoutError::Disconnected) => return Err(gone()),
        Err(mpsc::RecvTimeoutError::Timeout) => {}
    }
    {
        let mut changes = shared
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(index) = changes.iter().position(|pending| pending.id == id) {
            changes.remove(index);
            return Err(CliError::new(
                ErrorCode::ConnectFailed,
                t!("cli.server.change_timed_out"),
            ));
        }
    }
    // Taken up just now: being made.
    outcome.recv().unwrap_or_else(|_| Err(gone()))
}

/// Hand a run to the app, which finds the card, and see it through: the
/// command's output as stdout, its context, its exit code. Taken back, so
/// certainly not typed, when the app does not take it up in time. While
/// the command prints nothing, an empty frame now and then finds out
/// whether the caller is still there; once it is not, the command gets a
/// ^C.
// 来自 since2006/shell-rs @ b2a307a: src/cli/server.rs 的 `run_in_terminal`、`run_error`
fn run_in_card(
    shared: &Shared,
    writer: &mut impl io::Write,
    card: String,
    command: String,
) -> Result<Reply, CliError> {
    let (run, events) = TerminalRun::new(command);
    let id = shared.next_change.fetch_add(1, Ordering::Relaxed);
    shared
        .runs
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push_back(PendingRun {
            id,
            card,
            run: run.handle(),
        });
    let first = match events.recv_timeout(QUEUE_TIMEOUT) {
        Ok(event) => event,
        Err(_) => {
            let mut runs = shared
                .runs
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(index) = runs.iter().position(|pending| pending.id == id) {
                runs.remove(index);
                return Err(CliError::new(
                    ErrorCode::ConnectFailed,
                    t!("cli.card.timed_out"),
                ));
            }
            drop(runs);
            // Taken up just now: its answer is on the way.
            events
                .recv()
                .map_err(|_| run_error(RunFailure::NotConnected))?
        }
    };
    if let RunEvent::Failed(failure) = first {
        return Err(run_error(failure));
    }
    let code = run
        .drive(&events, RUN_TIMING, &mut |progress| match progress {
            RunProgress::Context(context) => write_json(
                writer,
                &Reply::Context(TerminalContext {
                    user: context.user,
                    host: context.host,
                    cwd: context.cwd,
                    shell: context.shell,
                    interp: context.interp,
                }),
            ),
            RunProgress::Output(bytes) => write_frame(writer, FrameKind::Stdout, &bytes),
            RunProgress::Idle => write_frame(writer, FrameKind::Stdout, &[]),
        })
        .map_err(run_error)?;
    Ok(Reply::Exit { code })
}

/// Why a run did not run, in the CLI's words.
fn run_error(failure: RunFailure) -> CliError {
    let busy = |message| CliError::new(ErrorCode::TerminalBusy, message);
    match failure {
        RunFailure::HostNotFound | RunFailure::NoTerminal => {
            CliError::new(ErrorCode::NoTerminal, t!("cli.card.not_found"))
        }
        RunFailure::NotConnected => {
            CliError::new(ErrorCode::NotConnected, t!("cli.card.not_connected"))
        }
        RunFailure::UnsupportedShell(shell) if shell.is_empty() => {
            CliError::new(ErrorCode::UnsupportedShell, t!("cli.card.unknown_shell"))
        }
        RunFailure::UnsupportedShell(shell) => CliError::new(
            ErrorCode::UnsupportedShell,
            t!("cli.card.unsupported_shell", shell = shell),
        ),
        RunFailure::TooLong { limit } => {
            CliError::new(ErrorCode::TooLong, t!("cli.card.too_long", limit = limit))
        }
        RunFailure::Busy(BusyReason::FullScreen) => busy(t!("cli.card.full_screen")),
        RunFailure::Busy(BusyReason::AnotherRun) => busy(t!("cli.card.another_run")),
        RunFailure::Busy(BusyReason::NoAnswer) => busy(t!("cli.card.no_answer")),
        RunFailure::Busy(BusyReason::NotStarted) => busy(t!("cli.card.not_started")),
        RunFailure::Busy(BusyReason::Transfer) => busy(t!("cli.card.another_run")),
        RunFailure::Disconnected => {
            CliError::new(ErrorCode::NotConnected, t!("cli.card.disconnected"))
        }
    }
}

fn not_enabled() -> CliError {
    CliError::new(ErrorCode::NotEnabled, t!("cli.server.not_enabled"))
}

/// 要连主机的请求先过这一关：主机在、保险库没挡着、有后端。保险库锁着时记下来，窗口壳会提醒用户。
fn connectable(shared: &Shared, id: &str) -> Result<(CliTarget, Arc<dyn CliBackend>), CliError> {
    let target = find(shared, id)?;
    if target.vault_locked() {
        shared.vault_wanted.store(true, Ordering::Release);
        return Err(vault_locked());
    }
    let backend = shared
        .backend
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .ok_or_else(|| CliError::new(ErrorCode::NotEnabled, t!("cli.server.not_enabled")))?;
    Ok((target, backend))
}

fn transfer_done(
    shared: &Shared,
    done: Result<TransferSummary, CliError>,
) -> Result<Reply, CliError> {
    if matches!(&done, Err(error) if error.code == ErrorCode::VaultLocked) {
        shared.vault_wanted.store(true, Ordering::Release);
    }
    done.map(Reply::TransferDone)
}

fn not_absolute(path: &Path) -> CliError {
    CliError::new(
        ErrorCode::BadRequest,
        t!("cli.server.not_absolute", path = path.display()),
    )
}

/// 一台主机的快照，按 ID 找。拷出来，不攥着锁：`exec` 一跑就是很久，窗口壳那期间还要更新快照。
fn find(shared: &Shared, id: &str) -> Result<CliTarget, CliError> {
    shared
        .targets
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .find(|target| target.info.id == id.trim())
        .cloned()
        .ok_or_else(|| host_not_found(id))
}

pub(super) fn vault_locked() -> CliError {
    CliError::new(ErrorCode::VaultLocked, t!("cli.error.vault_locked"))
}
