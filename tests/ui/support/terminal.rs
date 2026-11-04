//! 假的终端传输：本地终端不起真的 shell，测试能看到它收到了什么。
// 来自 since2006/shell-rs @ b2a307a: tests/ui/support/terminal.rs
// （裁剪：去掉远程、Telnet、串口、ZMODEM、侧边执行、SFTP 通道；加了 `shutdowns`）

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use qishell_terminal::{
    FixedLocalTerminalTransportProvider, TerminalSize, TerminalTransport, TerminalTransportCommand,
    TerminalTransportEvent, TerminalTransportFactory,
};

/// 假 shell 启动以后做什么。
#[derive(Clone, Copy, Default)]
pub enum FakeBehavior {
    /// 打出提示符，然后一直开着，把写进来的回显出去。
    #[default]
    Running,
    /// 第一次启动就退出（退出码 9）。
    ExitFirst,
    /// 第一次启动就失败。
    FailFirst,
    /// 提示符后面再打出这些，像程序那样。
    Prints(&'static str),
    /// 本机说 shell 在这个目录里。
    InFolder(&'static str),
}

/// 每张卡片的终端都从这里建传输；记下启动了几次、写进来什么、收到几次关闭。
#[derive(Default)]
pub struct FakeTerminalFactory {
    pub starts: AtomicUsize,
    pub behavior: FakeBehavior,
    pub writes: Arc<Mutex<Vec<Vec<u8>>>>,
    pub resizes: Arc<Mutex<Vec<TerminalSize>>>,
    /// 收到 `Shutdown` 的次数：关卡片就是关它的 shell。
    pub shutdowns: Arc<AtomicUsize>,
    /// 每一次启动的传输往终端发事件的口子，按第几次启动（从 1 数）记（工单 06：测试替假 shell 报告前台程序）。
    pub senders: Arc<Mutex<HashMap<usize, async_channel::Sender<TerminalTransportEvent>>>>,
}

impl FakeTerminalFactory {
    pub fn exit_first() -> Self {
        Self {
            behavior: FakeBehavior::ExitFirst,
            ..Self::default()
        }
    }

    pub fn fail_first() -> Self {
        Self {
            behavior: FakeBehavior::FailFirst,
            ..Self::default()
        }
    }

    pub fn printing(text: &'static str) -> Self {
        Self {
            behavior: FakeBehavior::Prints(text),
            ..Self::default()
        }
    }

    pub fn in_folder(folder: &'static str) -> Self {
        Self {
            behavior: FakeBehavior::InFolder(folder),
            ..Self::default()
        }
    }

    /// 写进来的全部文字。
    pub fn written_text(&self) -> String {
        let bytes: Vec<_> = self
            .writes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .flatten()
            .copied()
            .collect();
        String::from_utf8(bytes).expect("fake transport receives UTF-8 test input")
    }

    pub fn starts(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    pub fn shutdowns(&self) -> usize {
        self.shutdowns.load(Ordering::SeqCst)
    }

    /// 等到收到 `count` 次关闭，最多两秒：传输跑在自己的线程上。
    pub fn wait_for_shutdowns(&self, count: usize) -> usize {
        let started = Instant::now();
        while self.shutdowns() < count && started.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.shutdowns()
    }
}

impl TerminalTransportFactory for FakeTerminalFactory {
    fn create(&self) -> Box<dyn TerminalTransport> {
        let run = self.starts.fetch_add(1, Ordering::SeqCst) + 1;
        Box::new(FakeTerminalTransport {
            run,
            behavior: self.behavior,
            writes: self.writes.clone(),
            resizes: self.resizes.clone(),
            shutdowns: self.shutdowns.clone(),
            senders: self.senders.clone(),
        })
    }
}

pub struct FakeTerminalTransport {
    pub run: usize,
    pub behavior: FakeBehavior,
    pub writes: Arc<Mutex<Vec<Vec<u8>>>>,
    pub resizes: Arc<Mutex<Vec<TerminalSize>>>,
    pub shutdowns: Arc<AtomicUsize>,
    pub senders: Arc<Mutex<HashMap<usize, async_channel::Sender<TerminalTransportEvent>>>>,
}

impl TerminalTransport for FakeTerminalTransport {
    fn run(
        self: Box<Self>,
        _: TerminalSize,
        commands: mpsc::Receiver<TerminalTransportCommand>,
        events: async_channel::Sender<TerminalTransportEvent>,
    ) -> anyhow::Result<()> {
        if matches!(self.behavior, FakeBehavior::FailFirst) && self.run == 1 {
            anyhow::bail!("测试启动失败");
        }
        events.send_blocking(TerminalTransportEvent::Started)?;
        // 在 `Started` 之后才交出去：测试发的事件总在启动之后到。
        self.senders
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(self.run, events.clone());
        if let FakeBehavior::InFolder(folder) = self.behavior {
            events.send_blocking(TerminalTransportEvent::WorkingDirectory(folder.into()))?;
        }
        events.send_blocking(TerminalTransportEvent::Output(
            format!("run:{}$ alpha.txt 会议纪要.md\r\n", self.run).into_bytes(),
        ))?;
        if let FakeBehavior::Prints(text) = self.behavior {
            events.send_blocking(TerminalTransportEvent::Output(text.as_bytes().to_vec()))?;
        }
        if matches!(self.behavior, FakeBehavior::ExitFirst) && self.run == 1 {
            events.send_blocking(TerminalTransportEvent::Exited {
                code: 9,
                signal: None,
            })?;
            return Ok(());
        }
        while let Ok(command) = commands.recv() {
            match command {
                TerminalTransportCommand::Write(bytes) => {
                    self.writes
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .push(bytes.clone());
                    events.send_blocking(TerminalTransportEvent::Output(bytes))?;
                }
                TerminalTransportCommand::Resize(size) => {
                    self.resizes
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .push(size);
                }
                TerminalTransportCommand::Shutdown => {
                    self.shutdowns.fetch_add(1, Ordering::SeqCst);
                    break;
                }
            }
        }
        Ok(())
    }
}

/// 本地终端都用 `factory` 的假传输；provider 记下每次怎么启动的。
pub fn fake_local_terminals_with(
    factory: Arc<FakeTerminalFactory>,
) -> Arc<FixedLocalTerminalTransportProvider> {
    Arc::new(FixedLocalTerminalTransportProvider::new(factory))
}

/// 本地终端启动以后只打一行提示符。
pub fn fake_local_terminals() -> Arc<FixedLocalTerminalTransportProvider> {
    fake_local_terminals_with(Arc::new(FakeTerminalFactory::default()))
}

/// 工单 03（终端交互）用到的查询。
impl FakeTerminalFactory {
    /// 终端最后一次告诉假 shell 的尺寸：有了它，终端就已经排好版。
    pub fn last_size(&self) -> Option<TerminalSize> {
        self.resizes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .last()
            .copied()
    }

    /// 忘掉已经写进来的东西，下一次只看新写的。
    pub fn clear_writes(&self) {
        self.writes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }
}

/// 工单 06（程序图块与状态点）用到的：替假 shell 往终端发事件。
impl FakeTerminalFactory {
    /// 第 `run` 次启动的传输（从 1 数；一张卡片一次，按开卡片的顺序）发出 `event`。
    /// 那次启动还没跑起来时，最多等两秒。
    pub fn send(&self, run: usize, event: TerminalTransportEvent) {
        let started = Instant::now();
        loop {
            let sender = self
                .senders
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(&run)
                .cloned();
            if let Some(sender) = sender {
                sender
                    .send_blocking(event)
                    .expect("the fake shell is running");
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "第 {run} 次启动的假 shell 没有跑起来"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// 第 `run` 次启动的假 shell 说它的前台程序是 `program`（`None`：回到提示符）。
    pub fn foreground(&self, run: usize, program: Option<&str>) {
        self.send(
            run,
            TerminalTransportEvent::ForegroundProgram(program.map(str::to_string)),
        );
    }

    /// 第 `run` 次启动的假 shell 打出 `text`。
    pub fn print(&self, run: usize, text: &str) {
        self.send(
            run,
            TerminalTransportEvent::Output(text.as_bytes().to_vec()),
        );
    }
}
