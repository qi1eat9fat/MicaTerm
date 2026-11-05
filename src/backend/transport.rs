// 参考 since2006/shell-rs @ b2a307a: src/ssh/transport.rs 的 `SshTerminalTransport::run_async`
// 和 src/ssh/worker.rs 的 `run_terminal_worker` 重写：
// 裁剪：去掉侧边执行、SFTP 通道、外部链接（`shell_only`）；
// 改造：连接从连接表借（共享运行时上的，见 `pool`），终端线程只 `block_on` 进去，不再自建运行时；
// 问答、探测、延迟归连接表管；连接被断开时报「连接已断开」，不再当作 shell 正常退出。

//! 远程终端的传输：在连接表借来的连接上开一个 shell 通道，把字节在通道和终端之间搬运。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_channel::Sender;
use qishell_ssh::{
    ChannelError, PtySize, SshHandle, find_cause, open_shell, resize_pty, write_shell,
};
use qishell_terminal::{
    TERM_NAME, TerminalSize, TerminalTransport, TerminalTransportCommand, TerminalTransportEvent,
    TerminalTransportFactory,
};
use russh::{Channel, ChannelMsg, client};
use tokio::sync::mpsc as tokio_mpsc;

use super::describe::describe_error;
use super::pool::{ConnectionPool, Lease};
use super::remote::{RemoteTarget, RemoteTerminalId};
use crate::i18n::t;

/// 关通道最多等多久。
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// shell 通道没说退出码就关了的时候，等多久看连接是不是断了。
const LOST_GRACE: Duration = Duration::from_millis(100);

/// 一个远程终端的传输工厂：每次建传输（打开、Enter 重连）都去连接表借连接。
pub(super) struct RemoteTransportFactory {
    pool: Arc<ConnectionPool>,
    terminal: RemoteTerminalId,
    target: RemoteTarget,
}

impl RemoteTransportFactory {
    pub(super) fn new(
        pool: Arc<ConnectionPool>,
        terminal: RemoteTerminalId,
        target: RemoteTarget,
    ) -> Self {
        Self {
            pool,
            terminal,
            target,
        }
    }
}

impl TerminalTransportFactory for RemoteTransportFactory {
    fn create(&self) -> Box<dyn TerminalTransport> {
        Box::new(RemoteTransport {
            pool: self.pool.clone(),
            terminal: self.terminal,
            target: self.target.clone(),
        })
    }
}

struct RemoteTransport {
    pool: Arc<ConnectionPool>,
    terminal: RemoteTerminalId,
    target: RemoteTarget,
}

impl TerminalTransport for RemoteTransport {
    /// 跑在终端自己的传输线程上：把终端的命令接进共享运行时，在那里 `block_on`。
    fn run(
        self: Box<Self>,
        initial_size: TerminalSize,
        commands: mpsc::Receiver<TerminalTransportCommand>,
        events: Sender<TerminalTransportEvent>,
    ) -> Result<()> {
        let (sender, receiver) = tokio_mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let bridge_stop = stop.clone();
        let bridge = thread::Builder::new()
            .name("qishell-remote-commands".into())
            .spawn(move || {
                while !bridge_stop.load(Ordering::Acquire) {
                    match commands.recv_timeout(Duration::from_millis(20)) {
                        Ok(command) => {
                            if sender.send(command).is_err() {
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })?;
        let runtime = self.pool.runtime().clone();
        let result = runtime.block_on(self.serve(initial_size, receiver, events));
        stop.store(true, Ordering::Release);
        let _ = bridge.join();
        result
    }
}

impl RemoteTransport {
    async fn serve(
        self: Box<Self>,
        initial_size: TerminalSize,
        mut commands: tokio_mpsc::UnboundedReceiver<TerminalTransportCommand>,
        events: Sender<TerminalTransportEvent>,
    ) -> Result<()> {
        // 连接时（可能正在等用户回答）终端还会改尺寸，也可能被关掉。
        let mut size = initial_size;
        let opening = self.open(initial_size);
        tokio::pin!(opening);
        let (lease, handle, channel) = loop {
            tokio::select! {
                opened = &mut opening => break opened.map_err(|reason| anyhow!(reason))?,
                command = commands.recv() => match command {
                    Some(TerminalTransportCommand::Resize(resized)) => size = resized,
                    Some(TerminalTransportCommand::Write(_)) => {}
                    // 关卡片：放手就是取消（是最后一个在等的终端时，问题也作废）。
                    Some(TerminalTransportCommand::Shutdown) | None => return Ok(()),
                },
            }
        };
        if size != initial_size {
            resize_pty(&channel, pty_size(size))
                .await
                .map_err(|error| anyhow!(describe_error(&error)))?;
        }
        events
            .send(TerminalTransportEvent::Started)
            .await
            .map_err(|_| anyhow!(t!("ssh.terminal.card_closed")))?;
        lease.shell_opened(&handle);
        let result = pump(&handle, channel, &mut commands, &events).await;
        drop(lease);
        result
    }

    /// 借连接、开 shell 通道。复用的连接开不出通道（只给一个通道的堡垒机）时，改成新建一条连接。
    async fn open(
        &self,
        size: TerminalSize,
    ) -> Result<(Lease, Arc<SshHandle>, Channel<client::Msg>), String> {
        let mut lease = self.pool.attach(self.terminal, &self.target, false);
        let handle = lease.ready().await?;
        match open_shell(&handle, TERM_NAME, pty_size(size)).await {
            Ok(channel) => return Ok((lease, handle, channel)),
            Err(error)
                if lease.reused()
                    && find_cause::<ChannelError>(&error) == Some(&ChannelError::SessionFailed) =>
            {
                self.pool.mark_single_channel(&self.target);
            }
            Err(error) => return Err(describe_error(&error)),
        }
        drop(lease);
        let mut lease = self.pool.attach(self.terminal, &self.target, true);
        let handle = lease.ready().await?;
        let channel = open_shell(&handle, TERM_NAME, pty_size(size))
            .await
            .map_err(|error| describe_error(&error))?;
        Ok((lease, handle, channel))
    }
}

/// 在 shell 通道和终端之间搬字节，直到 shell 退出、连接断开或终端关掉。
async fn pump(
    handle: &SshHandle,
    mut channel: Channel<client::Msg>,
    commands: &mut tokio_mpsc::UnboundedReceiver<TerminalTransportCommand>,
    events: &Sender<TerminalTransportEvent>,
) -> Result<()> {
    let mut exit_code = None;
    let mut exit_signal = None;
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(TerminalTransportCommand::Write(bytes)) => {
                    write_shell(&channel, bytes)
                        .await
                        .map_err(|error| anyhow!(describe_error(&error)))?;
                }
                Some(TerminalTransportCommand::Resize(size)) => {
                    resize_pty(&channel, pty_size(size))
                        .await
                        .map_err(|error| anyhow!(describe_error(&error)))?;
                }
                Some(TerminalTransportCommand::Shutdown) | None => {
                    let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, async {
                        let _ = channel.eof().await;
                        let _ = channel.close().await;
                    })
                    .await;
                    return Ok(());
                }
            },
            message = channel.wait() => match message {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                    if events.send(TerminalTransportEvent::Output(data.to_vec())).await.is_err() {
                        return Ok(());
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                Some(ChannelMsg::ExitSignal { signal_name, .. }) => {
                    exit_signal = Some(format!("{signal_name:?}"));
                }
                Some(ChannelMsg::Eof | ChannelMsg::Close) | None => break,
                _ => {}
            }
        }
    }
    // 没说怎么退出的就关了：多半是连接断了（网络断开、服务器重启），这是断开，不是 shell 退出。
    if exit_code.is_none() && exit_signal.is_none() {
        tokio::time::sleep(LOST_GRACE).await;
        if handle.is_closed() {
            return Err(anyhow!(t!("ssh.terminal.connection_lost")));
        }
    }
    let _ = events
        .send(TerminalTransportEvent::Exited {
            code: exit_code.unwrap_or(0),
            signal: exit_signal,
        })
        .await;
    Ok(())
}

fn pty_size(size: TerminalSize) -> PtySize {
    let dimension = |value: usize| value.min(u32::MAX as usize) as u32;
    let pixels = |cells: usize, cell: u16| {
        cells
            .saturating_mul(usize::from(cell))
            .min(u32::MAX as usize) as u32
    };
    PtySize {
        columns: dimension(size.columns()),
        rows: dimension(size.rows()),
        width: pixels(size.columns(), size.cell_width()),
        height: pixels(size.rows(), size.cell_height()),
    }
}
