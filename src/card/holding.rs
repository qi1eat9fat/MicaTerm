//! 等保险库时远程卡片里的占位传输（工单 33）：不连接、不输出，一直停在「启动中」，等到被换掉
//! （解锁后换成真的连接）或卡片关掉。这样卡片可以先建出来，终端照常有尺寸、能画，只是还没去连。

use std::sync::{Arc, mpsc};

use anyhow::Result;
use async_channel::Sender;
use qishell_terminal::{
    SharedTerminalTransportFactory, TerminalSize, TerminalTransport, TerminalTransportCommand,
    TerminalTransportEvent, TerminalTransportFactory,
};

/// 占位传输的工厂：每次都建一个什么都不做的传输。
pub(super) fn holding_factory() -> SharedTerminalTransportFactory {
    Arc::new(HoldingFactory)
}

struct HoldingFactory;

impl TerminalTransportFactory for HoldingFactory {
    fn create(&self) -> Box<dyn TerminalTransport> {
        Box::new(HoldingTransport)
    }
}

struct HoldingTransport;

impl TerminalTransport for HoldingTransport {
    fn run(
        self: Box<Self>,
        _: TerminalSize,
        commands: mpsc::Receiver<TerminalTransportCommand>,
        _: Sender<TerminalTransportEvent>,
    ) -> Result<()> {
        // 不发 `Started`：终端一直是「启动中」。被关掉（或终端没了）就结束。
        while let Ok(command) = commands.recv() {
            if matches!(command, TerminalTransportCommand::Shutdown) {
                break;
            }
        }
        Ok(())
    }
}
