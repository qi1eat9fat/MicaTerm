// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-ssh/src/sftp/client.rs
// 裁剪：去掉 `on_channel` 和 `TerminalNotConnected`（终端连接上的 SFTP 通道）。

use super::{DirectoryEntry, DirectoryListing, EntryKind, FileMetadata, RemotePath, SftpError};
use crate::{Latency, SshConnectionConfig, SshConnector, SshHandle, SshPrompts};
use anyhow::{Result, anyhow, bail};
use futures::StreamExt as _;
use russh_sftp::{
    client::{RawSftpSession, error::Error},
    protocol::{FileAttributes, OpenFlags, StatusCode},
};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::watch,
};

/// Small protocol seam, also implemented by deterministic fault-injection tests.
pub(crate) trait RemoteFs {
    /// `lstat`: a link describes itself. `None` when the path does not exist.
    async fn metadata(&self, path: &RemotePath) -> Result<Option<FileMetadata>>;
    /// `stat`: follows links. `None` when the path or the link target is missing.
    async fn stat(&self, path: &RemotePath) -> Result<Option<FileMetadata>>;
    /// The entries of a directory without `.` and `..`, kinds as `lstat` reports them.
    async fn read_dir(&self, path: &RemotePath) -> Result<Vec<DirectoryEntry>>;
    async fn open(&self, path: &RemotePath, create: bool) -> Result<String>;
    /// Open a file to write it over in place: truncated, or created when it
    /// is missing. Follows links, so the file a link points at is written.
    async fn open_replace(&self, path: &RemotePath) -> Result<String>;
    /// Open an existing file for reading.
    async fn open_read(&self, path: &RemotePath) -> Result<String>;
    /// Up to `len` bytes at `offset`; may return fewer. `None` at end of file.
    async fn read(&self, handle: &str, offset: u64, len: u32) -> Result<Option<Vec<u8>>>;
    async fn write(&self, handle: &str, offset: u64, bytes: Vec<u8>) -> Result<()>;
    async fn close(&self, handle: &str) -> Result<()>;
    /// Set the modification time and permission bits of a closed file by its
    /// path (`setstat`), as WinSCP does: a bastion host has been seen to
    /// refuse `fsetstat` on a file still open.
    async fn attributes(
        &self,
        path: &RemotePath,
        modified: Option<u32>,
        permissions: Option<u32>,
    ) -> Result<()>;
    async fn mkdir(&self, path: &RemotePath) -> Result<()>;
    async fn symlink(&self, target: &str, path: &RemotePath) -> Result<()>;
    async fn readlink(&self, path: &RemotePath) -> Result<String>;
    /// A plain `rename`, which fails when `to` is there: replacing a file
    /// moves the old one aside first, as WinSCP never sends
    /// `posix-rename@openssh.com` either.
    async fn rename(&self, from: &RemotePath, to: &RemotePath) -> Result<()>;
    async fn remove(&self, path: &RemotePath) -> Result<()>;
    /// Remove an empty directory.
    async fn rmdir(&self, path: &RemotePath) -> Result<()>;
    /// Set the permission bits of a path (`setstat`, which follows links).
    async fn set_permissions(&self, path: &RemotePath, permissions: u32) -> Result<()>;
}

pub(crate) struct SftpClient {
    raw: RawSftpSession,
    ssh: Option<SshHandle>,
    /// 连接断了就置 true：SFTP 流读到了头，或者有个请求因为连接出错失败了。
    lost: Arc<watch::Sender<bool>>,
    /// Turns true once the connection is gone, as `lost` says.
    closed: watch::Receiver<bool>,
    fingerprint: String,
}
impl SftpClient {
    pub async fn connect(
        connector: &SshConnector,
        config: &SshConnectionConfig,
        prompts: Arc<SshPrompts>,
    ) -> Result<Self> {
        let (connector, config) = (connector.clone(), config.clone());
        on_own_thread(async move {
            let (ssh, fingerprint) = connector.connect(&config, prompts).await?;
            tokio::time::timeout(std::time::Duration::from_secs(15), async {
                let channel = ssh.channel_open_session().await?;
                channel.request_subsystem(true, "sftp").await?;
                Self::initialize(channel.into_stream(), Some(ssh), fingerprint).await
            })
            .await
            .map_err(|_| anyhow!(SftpError::StartTimeout))?
        })
        .await
    }
    async fn initialize<S>(stream: S, ssh: Option<SshHandle>, fingerprint: String) -> Result<Self>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let lost = Arc::new(watch::Sender::new(false));
        let closed = lost.subscribe();
        let raw = RawSftpSession::new(WatchedStream {
            inner: stream,
            closed: lost.clone(),
        });
        let version = raw.init().await?;
        if version.version != 3 {
            bail!(SftpError::NoV3);
        }
        // No extension is used, even when offered: a bastion host that offers
        // `fsync@openssh.com` has been seen to end the whole session on it.
        // WinSCP sends none of them for a transfer.
        Ok(Self {
            raw,
            ssh,
            lost,
            closed,
            fingerprint,
        })
    }
    /// 发一个请求。连接已经断了就不发，马上失败；请求因为连接出错失败了，就记下连接断了。
    /// 卡住的连接（网断了，没有 FIN 也没有 RST）只有请求超时才看得出来：一个请求等满了超时，
    /// 后面的请求（比如出错后关文件句柄）就不该在同一条死连接上再各等一遍，引擎好马上说断了、去重连。
    async fn request<T>(&self, request: impl Future<Output = Result<T>>) -> Result<T> {
        if *self.closed.borrow() {
            return Err(Error::UnexpectedBehavior("session closed".into()).into());
        }
        let result = request.await;
        if let Err(error) = &result
            && is_network_error(error)
        {
            self.lost.send_replace(true);
        }
        result
    }
    /// Only the Unix tests have a local `sftp-server` to talk to.
    #[cfg(all(test, unix))]
    pub(super) async fn local_test_server(
        directory: &std::path::Path,
    ) -> Result<(Self, tokio::process::Child)> {
        let executable = ["/usr/libexec/sftp-server", "/usr/lib/openssh/sftp-server"]
            .into_iter()
            .find(|p| std::path::Path::new(p).exists())
            .ok_or_else(|| anyhow!("no OpenSSH sftp-server on this machine"))?;
        let mut child = tokio::process::Command::new(executable)
            .current_dir(directory)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stream = tokio::io::join(child.stdout.take().unwrap(), child.stdin.take().unwrap());
        let client = Self::initialize(stream, None, "local-test-key".into()).await?;
        Ok((client, child))
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    /// The SSH connection's round trip, measured as a terminal's is. `None`
    /// for the tests' local sftp-server, which has no SSH connection.
    pub async fn round_trip(&self) -> Option<Latency> {
        Some(crate::round_trip(self.ssh.as_ref()?).await)
    }
    /// Resolves once the server side of the SFTP session has gone, which
    /// russh-sftp otherwise only tells the next request, or once a request
    /// found the connection lost. It does not hold on to the client, so
    /// dropping the client still closes the connection.
    pub fn closed(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut closed = self.closed.clone();
        async move {
            // An error means the stream itself is gone: closed too.
            let _ = closed.wait_for(|closed| *closed).await;
        }
    }
    pub async fn canonicalize(&self, path: &RemotePath) -> Result<RemotePath> {
        self.request(async {
            let response = self.raw.realpath(path.as_str()).await?;
            RemotePath::new(checked_text(
                &response
                    .files
                    .first()
                    .ok_or_else(|| anyhow!(SftpError::NoPath))?
                    .filename,
            )?)
        })
        .await
    }
    /// A directory for the browser: canonical path, owners, and what each
    /// link points to (resolved concurrently, one `stat` per link).
    pub async fn list(&self, path: &RemotePath) -> Result<DirectoryListing> {
        let path = self.canonicalize(path).await?;
        let entries = self.read_dir(&path).await?;
        let entries = futures::stream::iter(entries)
            .map(|entry| {
                let path = &path;
                async move {
                    if entry.metadata().kind() != EntryKind::Symlink {
                        return Ok(entry);
                    }
                    let target = self
                        .stat(&path.join(entry.name())?)
                        .await
                        .or_else(|error| {
                            if is_network_error(&error) {
                                Err(error)
                            } else {
                                Ok(None)
                            }
                        })?;
                    Ok::<_, anyhow::Error>(entry.with_target_kind(target.map(|m| m.kind())))
                }
            })
            .buffered(16)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        Ok(DirectoryListing::new(path.as_str(), entries))
    }
}

/// 在一条自己的线程上建连接：SSH 会话和 SFTP 流的任务都跑在那里，等连接断了（或者客户端放下了）线程才结束。
///
/// 引擎（命令、计时、事件）留在调用的线程上，不跟着套接字一起卡住。套接字照理不会挡住线程，
/// 可是有的机器上（装了过滤网络的驱动，比如 AdGuard）往一条对面不收的连接写，非阻塞的 `send`
/// 也会一直挡着；会话任务和引擎在同一条线程上的话，整个 SFTP 工作线程就停了：请求超时、
/// 「停止」「断开」都没人管，界面上一点动静都没有。
async fn on_own_thread(
    start: impl Future<Output = Result<SftpClient>> + Send + 'static,
) -> Result<SftpClient> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("micaterm-sftp-io".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = sender.send(Err(error.into()));
                    return;
                }
            };
            runtime.block_on(async move {
                let mut sender = sender;
                // 没人等了（停止、关标签）就别再连下去。
                let client = tokio::select! {
                    client = start => client,
                    () = sender.closed() => return,
                };
                let mut closed = match &client {
                    Ok(client) => client.closed.clone(),
                    Err(_) => {
                        let _ = sender.send(client);
                        return;
                    }
                };
                if sender.send(client).is_err() {
                    return;
                }
                // 客户端放下了，发送端跟着没了，`wait_for` 也就返回了。
                let _ = closed.wait_for(|closed| *closed).await;
            });
        })?;
    receiver
        .await
        .map_err(|_| anyhow!("the SFTP connection thread ended"))?
}

impl RemoteFs for SftpClient {
    async fn metadata(&self, path: &RemotePath) -> Result<Option<FileMetadata>> {
        self.request(async {
            match self.raw.lstat(path.as_str()).await {
                Ok(attrs) => Ok(Some(metadata(&attrs.attrs))),
                Err(Error::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
                    Ok(None)
                }
                Err(error) => Err(error.into()),
            }
        })
        .await
    }
    async fn stat(&self, path: &RemotePath) -> Result<Option<FileMetadata>> {
        self.request(async {
            match self.raw.stat(path.as_str()).await {
                Ok(attrs) => Ok(Some(metadata(&attrs.attrs))),
                Err(Error::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
                    Ok(None)
                }
                Err(error) => Err(error.into()),
            }
        })
        .await
    }
    async fn read_dir(&self, path: &RemotePath) -> Result<Vec<DirectoryEntry>> {
        self.request(async {
            let handle = self.raw.opendir(path.as_str()).await?.handle;
            let result: Result<Vec<DirectoryEntry>> = async {
                let mut entries = Vec::new();
                loop {
                    match self.raw.readdir(&handle).await {
                        Ok(page) => {
                            for entry in page.files {
                                if matches!(entry.filename.as_str(), "." | "..") {
                                    continue;
                                }
                                checked_text(&entry.filename)?;
                                path.join(&entry.filename)?;
                                let (owner, group) = parse_longname(&entry.longname)
                                    .map(|(owner, group)| (Some(owner), Some(group)))
                                    .unwrap_or_else(|| {
                                        (
                                            entry.attrs.uid.map(|id| id.to_string()),
                                            entry.attrs.gid.map(|id| id.to_string()),
                                        )
                                    });
                                entries.push(
                                    DirectoryEntry::new(entry.filename, metadata(&entry.attrs))
                                        .with_owner(owner, group),
                                );
                            }
                        }
                        Err(Error::Status(status)) if status.status_code == StatusCode::Eof => {
                            break;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                Ok(entries)
            }
            .await;
            let closed = self.raw.close(handle).await;
            let entries = result?;
            closed?;
            Ok(entries)
        })
        .await
    }
    async fn open(&self, path: &RemotePath, create: bool) -> Result<String> {
        self.request(async {
            let flags = if create {
                OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE
            } else {
                OpenFlags::WRITE
            };
            Ok(self
                .raw
                .open(path.as_str(), flags, FileAttributes::empty())
                .await?
                .handle)
        })
        .await
    }
    async fn open_replace(&self, path: &RemotePath) -> Result<String> {
        self.request(async {
            Ok(self
                .raw
                .open(
                    path.as_str(),
                    OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
                    FileAttributes::empty(),
                )
                .await?
                .handle)
        })
        .await
    }
    async fn open_read(&self, path: &RemotePath) -> Result<String> {
        self.request(async {
            Ok(self
                .raw
                .open(path.as_str(), OpenFlags::READ, FileAttributes::empty())
                .await?
                .handle)
        })
        .await
    }
    async fn read(&self, handle: &str, offset: u64, len: u32) -> Result<Option<Vec<u8>>> {
        self.request(async {
            match self.raw.read(handle, offset, len).await {
                Ok(data) => Ok(Some(data.data)),
                Err(Error::Status(status)) if status.status_code == StatusCode::Eof => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
        .await
    }
    async fn write(&self, handle: &str, offset: u64, bytes: Vec<u8>) -> Result<()> {
        self.request(async {
            self.raw.write(handle, offset, bytes).await?;
            Ok(())
        })
        .await
    }
    async fn close(&self, handle: &str) -> Result<()> {
        self.request(async {
            self.raw.close(handle).await?;
            Ok(())
        })
        .await
    }

    async fn attributes(
        &self,
        path: &RemotePath,
        modified: Option<u32>,
        permissions: Option<u32>,
    ) -> Result<()> {
        self.request(async {
            self.raw
                .setstat(
                    path.as_str(),
                    FileAttributes {
                        mtime: modified,
                        atime: modified,
                        permissions: permissions.map(|p| p & 0o777),
                        ..FileAttributes::empty()
                    },
                )
                .await?;
            Ok(())
        })
        .await
    }
    async fn mkdir(&self, path: &RemotePath) -> Result<()> {
        self.request(async {
            self.raw
                .mkdir(path.as_str(), FileAttributes::empty())
                .await?;
            Ok(())
        })
        .await
    }
    async fn symlink(&self, target: &str, path: &RemotePath) -> Result<()> {
        self.request(async {
            self.raw.symlink(target, path.as_str()).await?;
            Ok(())
        })
        .await
    }
    async fn readlink(&self, path: &RemotePath) -> Result<String> {
        self.request(async {
            checked_text(
                &self
                    .raw
                    .readlink(path.as_str())
                    .await?
                    .files
                    .first()
                    .ok_or_else(|| anyhow!(SftpError::NoLinkTarget))?
                    .filename,
            )
        })
        .await
    }
    async fn rename(&self, from: &RemotePath, to: &RemotePath) -> Result<()> {
        self.request(async {
            self.raw.rename(from.as_str(), to.as_str()).await?;
            Ok(())
        })
        .await
    }
    async fn remove(&self, path: &RemotePath) -> Result<()> {
        self.request(async {
            match self.raw.remove(path.as_str()).await {
                Ok(_) => Ok(()),
                Err(Error::Status(s)) if s.status_code == StatusCode::NoSuchFile => Ok(()),
                Err(error) => Err(error.into()),
            }
        })
        .await
    }
    async fn rmdir(&self, path: &RemotePath) -> Result<()> {
        self.request(async {
            self.raw.rmdir(path.as_str()).await?;
            Ok(())
        })
        .await
    }
    async fn set_permissions(&self, path: &RemotePath, permissions: u32) -> Result<()> {
        self.request(async {
            self.raw
                .setstat(
                    path.as_str(),
                    FileAttributes {
                        permissions: Some(permissions & 0o7777),
                        ..FileAttributes::empty()
                    },
                )
                .await?;
            Ok(())
        })
        .await
    }
}
fn metadata(attrs: &FileAttributes) -> FileMetadata {
    // Compare the whole type field: russh-sftp's `is_dir()` tests one bit,
    // which sockets and block devices also carry.
    let kind = match attrs.permissions.map(|mode| mode & 0o170_000) {
        Some(0o040_000) => EntryKind::Directory,
        Some(0o120_000) => EntryKind::Symlink,
        Some(0o100_000) => EntryKind::File,
        _ => EntryKind::Other,
    };
    FileMetadata::new(
        kind,
        attrs.size.unwrap_or(0),
        attrs.mtime,
        attrs.permissions,
    )
}
/// The SFTP stream, telling when the server side has ended. russh ends it
/// when the channel or the whole SSH connection closes, including when
/// keepalives go unanswered after the network drops; russh-sftp keeps that
/// to itself until the next request fails. Watching it lets an idle SFTP tab
/// notice at the same moment a terminal on the same host does.
struct WatchedStream<S> {
    inner: S,
    /// 和客户端的 `lost` 是同一个。
    closed: Arc<watch::Sender<bool>>,
}

impl<S: AsyncRead + Unpin> AsyncRead for WatchedStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let wanted = buf.remaining() > 0;
        let before = buf.filled().len();
        let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
        // Nothing read into room for something is the end of the stream.
        if let Poll::Ready(result) = &poll
            && (result.is_err() || (wanted && buf.filled().len() == before))
        {
            this.closed.send_replace(true);
        }
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for WatchedStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Whether `error` is the connection failing under the SFTP session, which
/// a transfer reconnects for, as opposed to the server refusing a request.
pub fn is_network_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        if cause.downcast_ref::<russh::Error>().is_some_and(|e| {
            matches!(
                e,
                russh::Error::Disconnect
                    | russh::Error::HUP
                    | russh::Error::ConnectionTimeout
                    | russh::Error::KeepaliveTimeout
                    | russh::Error::InactivityTimeout
            )
        }) {
            return true;
        }
        cause.downcast_ref::<std::io::Error>().is_some_and(|e| {
            matches!(
                e.kind(),
                std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::BrokenPipe
            )
        }) || cause
            .downcast_ref::<Error>()
            .is_some_and(|error| match error {
                Error::IO(_) | Error::Timeout => true,
                // `session closed`: the SSH channel under the SFTP session is
                // gone, as when the network drops; russh-sftp says so before
                // sending anything more.
                Error::UnexpectedBehavior(message) => {
                    message == "sender dropped"
                        || message == "session closed"
                        || message.contains("SendError")
                        || message.contains("RecvError")
                }
                Error::Status(status) => matches!(
                    status.status_code,
                    StatusCode::ConnectionLost | StatusCode::NoConnection
                ),
                _ => false,
            })
    })
}

/// Owner and group from an SFTP v3 `longname`, the server's `ls -l` line:
/// `drwxr-xr-x    2 root     root         4096 Jan  1 12:00 name`. `None` when
/// the line does not have that shape; callers fall back to numeric ids.
pub(crate) fn parse_longname(line: &str) -> Option<(String, String)> {
    let mut fields = line.split_whitespace();
    let mode = fields.next()?;
    let mut chars = mode.chars();
    if !chars.next().is_some_and(|c| "-dlcbpsD".contains(c))
        || chars.by_ref().take(9).count() != 9
        || !mode
            .chars()
            .skip(1)
            .take(9)
            .all(|c| "rwxsStTl-".contains(c))
    {
        return None;
    }
    fields.next()?.parse::<u64>().ok()?;
    let owner = fields.next()?;
    let group = fields.next()?;
    fields.next()?.parse::<u64>().ok()?;
    Some((owner.to_string(), group.to_string()))
}

// russh-sftp v3 decodes wire strings lossily. Refuse ambiguous names before
// they can become a path for navigation or mutation.
fn checked_text(value: &str) -> Result<String> {
    if value.contains('\u{fffd}') {
        bail!(SftpError::AmbiguousName);
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod longname_tests {
    use super::parse_longname;

    #[test]
    fn parses_openssh_and_rejects_other_shapes() {
        assert_eq!(
            parse_longname("drwxr-xr-x    2 root     wheel        4096 Jan  1 12:00 bin"),
            Some(("root".into(), "wheel".into()))
        );
        assert_eq!(
            parse_longname("-rw-r--r--+   1 用户 staff 12 Sep 24 10:36 文件 甲.txt"),
            Some(("用户".into(), "staff".into()))
        );
        assert_eq!(
            parse_longname("lrwxrwxrwx 1 0 0 7 Apr 22  2024 bin -> usr/bin"),
            Some(("0".into(), "0".into()))
        );
        assert_eq!(parse_longname(""), None);
        assert_eq!(parse_longname("bin"), None);
        assert_eq!(parse_longname("drwxr-xr-x root root 4096 Jan 1 bin"), None);
        assert_eq!(
            parse_longname("xrwxr-xr-x 1 root root 1 Jan 1 12:00 x"),
            None
        );
    }
}

#[cfg(test)]
mod network_error_tests {
    use super::{Error, is_network_error};

    #[test]
    fn a_closed_session_is_a_lost_connection() {
        let closed = anyhow::Error::new(Error::UnexpectedBehavior("session closed".into()))
            .context("无法读取目录");
        assert!(is_network_error(&closed));
        let other = anyhow::Error::new(Error::UnexpectedBehavior("bad packet".into()));
        assert!(!is_network_error(&other));
    }
}

#[cfg(test)]
mod lost_connection_tests {
    use super::{RemoteFs as _, SftpClient, is_network_error, on_own_thread};
    use std::{
        pin::Pin,
        sync::{Arc, Condvar, Mutex, mpsc},
        task::{Context, Poll},
        time::Duration,
    };
    use tokio::io::{
        AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, DuplexStream, ReadBuf,
    };

    /// 关着的时候，往流里写的线程就挡在那里：像有的机器上往一条对面不收的连接写，非阻塞的 `send` 也不返回。
    #[derive(Clone, Default)]
    struct Gate(Arc<(Mutex<bool>, Condvar)>);

    impl Gate {
        fn close(&self) {
            *self.0.0.lock().unwrap() = true;
        }
        fn open(&self) {
            *self.0.0.lock().unwrap() = false;
            self.0.1.notify_all();
        }
        fn pass(&self) {
            let mut closed = self.0.0.lock().unwrap();
            while *closed {
                closed = self.0.1.wait(closed).unwrap();
            }
        }
    }

    struct Gated {
        inner: DuplexStream,
        gate: Gate,
    }

    impl AsyncRead for Gated {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for Gated {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.gate.pass();
            Pin::new(&mut self.inner).poll_write(cx, buf)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    /// 对面：答一个 SFTP v3 的 VERSION，之后收下什么都不答，像网断了。
    async fn silent_server(mut stream: DuplexStream) {
        let mut init = [0; 9];
        if stream.read_exact(&mut init).await.is_err() {
            return;
        }
        if stream
            .write_all(&[0, 0, 0, 5, 2, 0, 0, 0, 3])
            .await
            .is_err()
        {
            return;
        }
        let mut sink = vec![0; 64 * 1024];
        while matches!(stream.read(&mut sink).await, Ok(read) if read > 0) {}
    }

    /// 网卡住了，写连接的线程也挡住了：引擎不跟着停，一个请求等满超时就知道连接断了，
    /// 之后出错关句柄不在死连接上再等一遍，`closed` 也马上说断了，工作线程好去重连。
    #[test]
    fn a_stuck_connection_is_given_up_after_one_request_timeout_without_stopping_the_engine() {
        let gate = Gate::default();
        // 引擎真被挡住的话，过一阵放开，测试失败而不是挂死。
        let (done, watchdog) = mpsc::channel::<()>();
        let opener = gate.clone();
        std::thread::spawn(move || {
            let _ = watchdog.recv_timeout(Duration::from_secs(20));
            opener.open();
        });
        let engine = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap();
        engine.block_on(async {
            let io_gate = gate.clone();
            let client = on_own_thread(async move {
                let (near, far) = tokio::io::duplex(64 * 1024);
                tokio::spawn(silent_server(far));
                let stream = Gated {
                    inner: near,
                    gate: io_gate,
                };
                SftpClient::initialize(stream, None, "key".into()).await
            })
            .await
            .unwrap();
            gate.close();
            let real = std::time::Instant::now();
            let started = tokio::time::Instant::now();

            let written = client.write("handle", 0, vec![0; 1024]).await;
            assert!(written.as_ref().is_err_and(is_network_error), "{written:?}");
            let gave_up = started.elapsed();
            assert!(gave_up >= Duration::from_secs(10), "{gave_up:?}");
            let closed = client.close("handle").await;
            assert!(closed.as_ref().is_err_and(is_network_error), "{closed:?}");
            assert_eq!(started.elapsed(), gave_up, "关句柄不再等一遍超时");
            client.closed().await;
            assert!(
                real.elapsed() < Duration::from_secs(20),
                "the engine was held up by the stuck connection"
            );
        });
        let _ = done.send(());
        gate.open();
    }
}
