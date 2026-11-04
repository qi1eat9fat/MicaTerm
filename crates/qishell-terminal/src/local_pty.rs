// 来自 since2006/shell-rs @ b2a307a: src/terminal/local_pty.rs
// 裁剪：去掉保留终端内容（`replay`）；`TERM_PROGRAM` 改为 QiShell；加了 Windows 上的测试。

use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow};
use async_channel::Sender;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::i18n::t;

use crate::environment::strip_launcher_vars;
use crate::foreground::foreground_program;
use crate::transport::send_event;
use crate::{
    LocalLaunch, LocalShell, LocalTerminalTransportProvider, SharedTerminalTransportFactory,
    TerminalSize, TerminalTransport, TerminalTransportCommand, TerminalTransportEvent,
    TerminalTransportFactory,
};

/// 本地终端告诉程序的终端类型（`TERM`），卡片状态栏也显示它。
pub const TERM_NAME: &str = "xterm-256color";

/// 本地终端收发的编码：ConPTY 和 Unix 的 PTY 都按 UTF-8 交给终端。
pub const TERMINAL_ENCODING: &str = "UTF-8";

/// Local terminals in a PTY of this computer.
#[derive(Default)]
pub struct LocalPtyTransportProvider;

impl LocalTerminalTransportProvider for LocalPtyTransportProvider {
    fn factory_for(&self, launch: &LocalLaunch) -> SharedTerminalTransportFactory {
        Arc::new(LocalPtyTransportFactory {
            launch: launch.clone(),
        })
    }
}

struct LocalPtyTransportFactory {
    launch: LocalLaunch,
}

impl TerminalTransportFactory for LocalPtyTransportFactory {
    fn create(&self) -> Box<dyn TerminalTransport> {
        Box::new(LocalPtyTransport {
            launch: self.launch.clone(),
        })
    }
}

struct LocalPtyTransport {
    launch: LocalLaunch,
}

/// How often at most the shell's folder is read, while it writes: it moves
/// with a `cd`, which a prompt follows.
const LOOK_FOR_THE_FOLDER: Duration = Duration::from_millis(200);

/// shell 不出声时，多久看一次它的前台程序（工单 06）。
const LOOK_FOR_THE_PROGRAM_QUIETLY: Duration = Duration::from_secs(1);

impl TerminalTransport for LocalPtyTransport {
    fn run(
        self: Box<Self>,
        initial_size: TerminalSize,
        commands: mpsc::Receiver<TerminalTransportCommand>,
        events: Sender<TerminalTransportEvent>,
    ) -> Result<()> {
        let LocalLaunch { shell, directory } = self.launch;
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(to_pty_size(initial_size))
            .context(t!("terminal.pty.open_failed"))?;

        let mut command = shell_command(&shell)?;
        // A folder gone since leaves the home folder, as with none.
        if let Some(directory) = directory.filter(|directory| directory.is_dir()) {
            command.cwd(directory);
        }

        let mut child =
            pair.slave
                .spawn_command(command)
                .map_err(|error| match shell.command() {
                    // Why, too: the path or the arguments are the user's.
                    Some(command) => anyhow!(t!(
                        "terminal.pty.command_failed",
                        command = command,
                        error = format!("{error:#}")
                    )),
                    None => error.context(t!("terminal.pty.shell_failed")),
                })?;
        drop(pair.slave);
        let pid = child.process_id();

        let mut reader = pair
            .master
            .try_clone_reader()
            .context(t!("terminal.pty.reader_failed"))?;
        let mut writer = pair
            .master
            .take_writer()
            .context(t!("terminal.pty.writer_failed"))?;

        let output_events = events.clone();
        let wrote = Arc::new(AtomicBool::new(false));
        let output_seen = wrote.clone();
        let program_wrote = Arc::new(AtomicBool::new(false));
        let program_output_seen = program_wrote.clone();
        let reader_thread = thread::Builder::new()
            .name("qishell-pty-reader".into())
            .spawn(move || {
                let mut buffer = [0_u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => {
                            output_seen.store(true, Ordering::Relaxed);
                            program_output_seen.store(true, Ordering::Relaxed);
                            if !send_event(
                                &output_events,
                                TerminalTransportEvent::Output(buffer[..count].to_vec()),
                            ) {
                                break;
                            }
                        }
                        Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                        // Unix PTYs commonly report EIO when the slave closes.
                        Err(error) if error.raw_os_error() == Some(5) => break,
                        Err(error) => {
                            send_event(
                                &output_events,
                                TerminalTransportEvent::Failed(
                                    t!("terminal.pty.read_error", error = error).into(),
                                ),
                            );
                            break;
                        }
                    }
                }
            })
            .context(t!("terminal.pty.reader_thread"))?;

        send_event(&events, TerminalTransportEvent::Started);
        let mut shutting_down = false;
        // The shell's folder as last said, and when it was last read.
        let mut folder: Option<PathBuf> = None;
        let mut looked: Option<Instant> = None;
        // 前台程序，同样是上次说的和上次看的时候（工单 06）。一开始没说过：
        // 第一次看到的就说，哪怕是 shell 自己（`None`）。
        let mut program: Option<Option<String>> = None;
        let mut program_looked: Option<Instant> = None;
        let exit = loop {
            match commands.recv_timeout(Duration::from_millis(20)) {
                Ok(TerminalTransportCommand::Write(bytes)) if !shutting_down => {
                    if let Err(error) = writer.write_all(&bytes).and_then(|_| writer.flush()) {
                        send_event(
                            &events,
                            TerminalTransportEvent::Failed(
                                t!("terminal.pty.write_error", error = error).into(),
                            ),
                        );
                        let _ = child.kill();
                        shutting_down = true;
                    }
                }
                Ok(TerminalTransportCommand::Resize(size)) if !shutting_down => {
                    if let Err(error) = pair.master.resize(to_pty_size(size)) {
                        send_event(
                            &events,
                            TerminalTransportEvent::Failed(
                                t!("terminal.pty.resize_error", error = error).into(),
                            ),
                        );
                    }
                }
                Ok(TerminalTransportCommand::Shutdown)
                | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if !shutting_down {
                        shutting_down = true;
                        let _ = child.kill();
                    }
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }

            // After the shell wrote, and no sooner than a while after the
            // last look: so the prompt after a `cd` is always looked after.
            if let Some(pid) = pid
                && !shutting_down
                && looked.is_none_or(|at| at.elapsed() >= LOOK_FOR_THE_FOLDER)
                && wrote.swap(false, Ordering::Relaxed)
            {
                looked = Some(Instant::now());
                if let Some(now) = working_directory(pid)
                    && folder.as_ref() != Some(&now)
                {
                    folder = Some(now.clone());
                    send_event(&events, TerminalTransportEvent::WorkingDirectory(now));
                }
            }

            // 前台程序：写了输出就最多每 200ms 看一次（程序启动、结束时都有输出），
            // 不写也每秒看一次（不出声的程序，比如 `sleep`）。
            if let Some(pid) = pid
                && !shutting_down
                && program_looked.is_none_or(|at| {
                    let since = at.elapsed();
                    since >= LOOK_FOR_THE_PROGRAM_QUIETLY
                        || (since >= LOOK_FOR_THE_FOLDER
                            && program_wrote.swap(false, Ordering::Relaxed))
                })
            {
                program_looked = Some(Instant::now());
                let now = foreground_program(pid, pair.master.as_ref());
                if program.as_ref() != Some(&now) {
                    program = Some(now.clone());
                    send_event(&events, TerminalTransportEvent::ForegroundProgram(now));
                }
            }

            if let Some(status) = child.try_wait().context(t!("terminal.pty.exit_status"))? {
                break status;
            }
        };

        drop(writer);
        drop(pair.master);
        let _ = reader_thread.join();
        if !shutting_down {
            send_event(
                &events,
                TerminalTransportEvent::Exited {
                    code: exit.exit_code(),
                    signal: exit.signal().map(str::to_owned),
                },
            );
        }
        Ok(())
    }
}

/// The command a local terminal's shell runs: the shell the settings name,
/// in the environment QiShell itself was started in, with the few
/// variables a terminal needs and none of the packaging's.
fn shell_command(shell: &LocalShell) -> Result<CommandBuilder> {
    let mut command = shell.command_builder()?;
    // The packaging that put QiShell on this computer left variables in
    // the environment; they are none of the user's shell's business.
    strip_launcher_vars(&mut command);
    command.env("TERM", TERM_NAME);
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "QiShell");
    Ok(command)
}

/// The folder process `pid` is in, where the system tells it.
#[cfg(target_os = "linux")]
fn working_directory(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// The folder process `pid` is in, where the system tells it.
#[cfg(target_os = "macos")]
fn working_directory(pid: u32) -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt as _;

    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: plain data, for which all zeroes is a value.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: the buffer is `info`, and `size` is how big it is.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    let path = &info.pvi_cdir.vip_path;
    // SAFETY: the path's rows of characters lie one after another.
    let bytes = unsafe {
        std::slice::from_raw_parts(path.as_ptr().cast::<u8>(), std::mem::size_of_val(path))
    };
    let path = CStr::from_bytes_until_nul(bytes).ok()?;
    (!path.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes())))
}

/// 进程 `pid` 的当前目录：Windows 没有现成的接口，读它自己内存里 PEB 的
/// `ProcessParameters.CurrentDirectory.DosPath`（工单 05，参考 gorex 的模型重写：
/// `internal/rex/proc_windows.go` 的 `processDir`）。
///
/// 只读 64 位进程（偏移按 x64 的布局）；32 位进程（Wow64）的参数在别处，跳过。
/// 打不开（提权的进程）、读不了就是 `None`，卡片退回启动时的目录。
#[cfg(all(windows, target_pointer_width = "64"))]
fn working_directory(pid: u32) -> Option<PathBuf> {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt as _;

    use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::{
        IsWow64Process, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    /// x64 上 `PEB.ProcessParameters` 的偏移。
    const PROCESS_PARAMETERS: usize = 0x20;
    /// x64 上 `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory`（开头就是 `DosPath`）的偏移。
    const CURRENT_DIRECTORY: usize = 0x38;

    /// `PROCESS_BASIC_INFORMATION`：地址都是对方进程里的，当数字看。
    #[repr(C)]
    #[derive(Default)]
    struct BasicInformation {
        exit_status: usize,
        peb_base_address: usize,
        affinity_mask: usize,
        base_priority: usize,
        unique_process_id: usize,
        inherited_from_unique_process_id: usize,
    }

    /// 对方进程里的 `UNICODE_STRING`：`buffer` 是对方的地址。
    #[repr(C)]
    #[derive(Default)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: usize,
    }

    /// 用完关掉的进程句柄。
    struct Process(HANDLE);

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: 句柄是 OpenProcess 打开的，只关这一次。
            unsafe { CloseHandle(self.0) };
        }
    }

    impl Process {
        /// 从对方进程的 `address` 读 `size` 字节到 `buffer`，读全了才算。
        fn read(&self, address: usize, buffer: *mut c_void, size: usize) -> bool {
            let mut read = 0;
            // SAFETY: `buffer` 指向调用方至少 `size` 字节的可写内存。
            let ok = unsafe {
                ReadProcessMemory(self.0, address as *const c_void, buffer, size, &mut read)
            };
            ok != 0 && read == size
        }
    }

    // SAFETY: 只读权限打开，失败返回空句柄。
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let process = Process(handle);

    let mut wow64 = 0;
    // SAFETY: 句柄有效，`wow64` 是可写的 BOOL。
    if unsafe { IsWow64Process(process.0, &mut wow64) } == 0 || wow64 != 0 {
        return None;
    }

    let mut info = BasicInformation::default();
    // SAFETY: `info` 的布局和大小与 PROCESS_BASIC_INFORMATION 一致。
    let status = unsafe {
        NtQueryInformationProcess(
            process.0,
            ProcessBasicInformation,
            (&mut info as *mut BasicInformation).cast(),
            size_of::<BasicInformation>() as u32,
            std::ptr::null_mut(),
        )
    };
    if status < 0 || info.peb_base_address == 0 {
        return None;
    }

    let mut parameters = 0_usize;
    if !process.read(
        info.peb_base_address + PROCESS_PARAMETERS,
        (&mut parameters as *mut usize).cast(),
        size_of::<usize>(),
    ) || parameters == 0
    {
        return None;
    }
    let mut directory = UnicodeString::default();
    if !process.read(
        parameters + CURRENT_DIRECTORY,
        (&mut directory as *mut UnicodeString).cast(),
        size_of::<UnicodeString>(),
    ) || directory.length == 0
        || directory.buffer == 0
    {
        return None;
    }
    let mut text = vec![0_u16; usize::from(directory.length) / 2];
    if !process.read(
        directory.buffer,
        text.as_mut_ptr().cast(),
        text.len() * size_of::<u16>(),
    ) {
        return None;
    }
    // 当前目录总以 `\` 结尾；`C:\` 这样的根目录留着它。
    if text.len() > 3 && text.last() == Some(&u16::from(b'\\')) {
        text.pop();
    }
    Some(PathBuf::from(OsString::from_wide(&text)))
}

/// 别的系统上读不到别的进程的目录：卡片退回启动时的目录。
#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    all(windows, target_pointer_width = "64")
)))]
fn working_directory(_: u32) -> Option<PathBuf> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::environment::LAUNCHER_VARS;

    /// `/bin/sh -c script`, through the command line the settings hold.
    fn shell_transport(script: &str) -> Box<dyn TerminalTransport> {
        let command = format!("/bin/sh -c {}", crate::quote_word(script));
        LocalPtyTransportProvider
            .factory_for(&LocalLaunch::new(LocalShell::new(&command)))
            .create()
    }

    fn drain_until_exit(
        events: &async_channel::Receiver<TerminalTransportEvent>,
        commands: &mpsc::Sender<TerminalTransportCommand>,
    ) -> (String, Option<u32>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut code = None;
        while Instant::now() < deadline {
            match events.try_recv() {
                Ok(TerminalTransportEvent::Output(bytes)) => output.extend(bytes),
                Ok(TerminalTransportEvent::Exited {
                    code: exit_code, ..
                }) => {
                    code = Some(exit_code);
                    break;
                }
                Ok(
                    TerminalTransportEvent::Started
                    | TerminalTransportEvent::Failed(_)
                    | TerminalTransportEvent::WorkingDirectory(_),
                ) => {}
                Err(async_channel::TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
                Err(async_channel::TryRecvError::Closed) => break,
            }
        }
        if code.is_none() {
            let _ = commands.send(TerminalTransportCommand::Shutdown);
        }
        (String::from_utf8_lossy(&output).into_owned(), code)
    }

    /// The command a local terminal's shell runs in: the environment
    /// QiShell was started in, without what the packaging left in it.
    #[test]
    fn the_shell_starts_without_the_packaging() {
        let command = shell_command(&LocalShell::new("/bin/sh")).unwrap();
        for name in LAUNCHER_VARS {
            assert_eq!(command.get_env(name), None, "{name} is left in");
        }
        // The rest of the environment is still there.
        assert!(command.get_env("PATH").is_some());
    }

    /// The same test in a child process whose environment holds what the
    /// AppImage runtime leaves in it.
    ///
    /// The parent cannot put a variable into its own environment while
    /// the other tests run, and the variable has to be in the environment
    /// before the shell starts for the shell to find it at all.
    ///
    /// The shell is zsh, the one that takes `ARGV0` for a special
    /// parameter and writes it into the argv[0] of every command it runs.
    #[test]
    fn the_shell_does_not_see_the_packaging() {
        const CHILD: &str = "QISHELL_TEST_WITH_THE_PACKAGING";
        const APPIMAGE: &str = "/tmp/ShellRS-0.1.14-linux-x86_64.AppImage";
        if std::env::var_os(CHILD).is_some() {
            // The child: the shell runs and says what it sees.
            let shell = if PathBuf::from("/bin/zsh").exists() {
                "/bin/zsh"
            } else {
                "/bin/sh"
            };
            let script = "printf '%s' \"${ARGV0-gone}|${APPIMAGE-gone}|${APPDIR-gone}|${OWD-gone}\"; \
                 if command -v rustup >/dev/null 2>&1; then \
                 printf '|rustup=%s' \"$(rustup --version 2>&1 | head -1)\"; fi";
            let (commands, command_receiver) = mpsc::channel();
            let (events, event_receiver) = async_channel::unbounded();
            let transport = shell_transport(&format!("{shell} -c {}", crate::quote_word(script)));
            let worker = thread::spawn(move || {
                transport.run(TerminalSize::DEFAULT, command_receiver, events)
            });
            let (output, code) = drain_until_exit(&event_receiver, &commands);
            assert_eq!(code, Some(0), "{output}");
            // What the shell saw, said out loud: this is the acceptance,
            // readable without a window.
            println!("the shell saw: {output}");
            assert!(
                output.starts_with("gone|gone|gone|gone"),
                "the shell sees the packaging: {output}"
            );
            // And rustup, which is one binary that tells its tools apart
            // by its own argv[0], still knows what it is. `ARGV0` names the
            // commands the shell starts, not the shell: its `$0` would not
            // tell.
            assert!(
                !output.contains("unknown proxy name"),
                "rustup does not know itself: {output}"
            );
            worker.join().unwrap().unwrap();
            // The parent looks for this line, to know the test really ran.
            println!("the shell did not see the packaging");
            return;
        }
        // The parent: the same test, in a child whose environment has them.
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "local_pty::tests::the_shell_does_not_see_the_packaging",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("ARGV0", APPIMAGE)
            .env("APPIMAGE", APPIMAGE)
            .env("APPDIR", "/tmp/.mount_ShellRSxxxx")
            .env("OWD", "/tmp");
        let output = {
            let _forks = crate::testing::no_forks();
            child.output().expect("the test runs again")
        };
        let text = String::from_utf8_lossy(&output.stdout);
        // What the shell saw, said out loud: this is the acceptance,
        // readable without a window.
        for line in text.lines() {
            if let Some(report) = line.trim().strip_prefix("the shell saw: ") {
                println!("{report}");
            }
        }
        assert!(output.status.success(), "the shell saw them:\n{text}");
        assert!(
            text.contains("the shell did not see the packaging"),
            "the test did not run in the child:\n{text}"
        );
    }

    #[test]
    fn runs_resizes_writes_and_reaps_a_deterministic_shell() {
        let (commands, command_receiver) = mpsc::channel();
        let (events, event_receiver) = async_channel::unbounded();
        let transport = shell_transport(
            "printf 'ready\\n'; read value; printf 'size:'; stty size; printf 'got:%s\\n' \"$value\"; exit 7",
        );
        let worker = thread::spawn(move || {
            transport.run(TerminalSize::new(80, 24, 8, 16), command_receiver, events)
        });

        commands
            .send(TerminalTransportCommand::Resize(TerminalSize::new(
                100, 31, 9, 18,
            )))
            .unwrap();
        commands
            .send(TerminalTransportCommand::Write(b"hello\n".to_vec()))
            .unwrap();

        let (output, code) = drain_until_exit(&event_receiver, &commands);
        assert_eq!(code, Some(7));
        assert!(output.contains("ready"));
        assert!(output.contains("size:31 100"));
        assert!(output.contains("got:hello"));
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn a_command_that_cannot_start_says_which() {
        let (_commands, command_receiver) = mpsc::channel();
        let (events, _event_receiver) = async_channel::unbounded();
        let error = LocalPtyTransportProvider
            .factory_for(&LocalLaunch::new(LocalShell::new("/no/such/shell -l")))
            .create()
            .run(TerminalSize::DEFAULT, command_receiver, events)
            .unwrap_err()
            .to_string();
        assert!(error.contains("无法启动 /no/such/shell -l"), "{error}");
    }

    /// `/bin/sh -c script` started in `directory`.
    fn shell_in(script: &str, directory: &std::path::Path) -> Box<dyn TerminalTransport> {
        let launch = LocalLaunch {
            directory: Some(directory.to_path_buf()),
            ..LocalLaunch::new(LocalShell::new(&format!(
                "/bin/sh -c {}",
                crate::quote_word(script)
            )))
        };
        LocalPtyTransportProvider.factory_for(&launch).create()
    }

    #[test]
    fn a_saved_terminal_starts_in_its_folder_or_home_when_it_is_gone() {
        let folder = tempfile::tempdir().unwrap();
        let script = "printf 'in:%s\\n' \"$(pwd -P)\"";
        for (directory, starts_in) in [
            (folder.path().to_path_buf(), Some(folder.path())),
            ("/no/such/folder".into(), None),
        ] {
            let (commands, command_receiver) = mpsc::channel();
            let (events, event_receiver) = async_channel::unbounded();
            let transport = shell_in(script, &directory);
            let worker = thread::spawn(move || {
                transport.run(TerminalSize::DEFAULT, command_receiver, events)
            });
            let (output, code) = drain_until_exit(&event_receiver, &commands);
            assert_eq!(code, Some(0), "{output}");
            match starts_in {
                Some(folder) => {
                    let folder = folder.canonicalize().unwrap();
                    assert!(
                        output.contains(&format!("in:{}", folder.display())),
                        "{output}"
                    );
                }
                None => assert!(!output.contains("/no/such"), "{output}"),
            }
            worker.join().unwrap().unwrap();
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_shells_folder_is_told_as_it_moves() {
        let start = tempfile::tempdir().unwrap();
        let next = tempfile::tempdir().unwrap();
        let script = format!(
            "printf 'a\\n'; sleep 0.5; cd {}; printf 'b\\n'; sleep 0.5",
            crate::quote_word(&next.path().display().to_string())
        );
        let (commands, command_receiver) = mpsc::channel();
        let (events, event_receiver) = async_channel::unbounded();
        let transport = shell_in(&script, start.path());
        let worker =
            thread::spawn(move || transport.run(TerminalSize::DEFAULT, command_receiver, events));

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut folders = Vec::new();
        while Instant::now() < deadline {
            match event_receiver.try_recv() {
                Ok(TerminalTransportEvent::WorkingDirectory(folder)) => folders.push(folder),
                Ok(TerminalTransportEvent::Exited { .. })
                | Err(async_channel::TryRecvError::Closed) => break,
                Ok(_) => {}
                Err(async_channel::TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
            }
        }
        let _ = commands.send(TerminalTransportCommand::Shutdown);
        worker.join().unwrap().unwrap();
        assert_eq!(
            folders,
            [
                start.path().canonicalize().unwrap(),
                next.path().canonicalize().unwrap()
            ]
        );
    }

    #[test]
    fn shutdown_terminates_and_reaps_the_child() {
        let (commands, command_receiver) = mpsc::channel();
        let (events, _event_receiver) = async_channel::unbounded();
        let transport = shell_transport("printf 'ready\\n'; sleep 30");
        let worker =
            thread::spawn(move || transport.run(TerminalSize::DEFAULT, command_receiver, events));

        commands.send(TerminalTransportCommand::Shutdown).unwrap();
        worker.join().unwrap().unwrap();
    }
}

/// Windows 上经 ConPTY 跑真实的 `cmd.exe`：输出、退出码、关闭时结束 shell。
#[cfg(all(test, windows))]
mod windows_tests {
    use std::time::Instant;

    use super::*;

    fn cmd_transport(script: &str) -> Box<dyn TerminalTransport> {
        let command = format!("cmd.exe /d /c {script}");
        LocalPtyTransportProvider
            .factory_for(&LocalLaunch::new(LocalShell::new(&command)))
            .create()
    }

    /// 读到退出为止，最多 10 秒：输出和退出码。ConPTY 一开始会问光标位置（`ESC [6n`），
    /// 等到回答才让程序跑，这里像终端那样答一句。
    fn drain_until_exit(
        events: &async_channel::Receiver<TerminalTransportEvent>,
        commands: &mpsc::Sender<TerminalTransportCommand>,
    ) -> (String, Option<u32>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut output = Vec::new();
        while Instant::now() < deadline {
            match events.try_recv() {
                Ok(TerminalTransportEvent::Output(bytes)) => {
                    if bytes.windows(4).any(|window| window == b"\x1b[6n") {
                        let _ =
                            commands.send(TerminalTransportCommand::Write(b"\x1b[1;1R".to_vec()));
                    }
                    output.extend(bytes);
                }
                Ok(TerminalTransportEvent::Exited { code, .. }) => {
                    return (String::from_utf8_lossy(&output).into_owned(), Some(code));
                }
                Ok(_) => {}
                Err(async_channel::TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
                Err(async_channel::TryRecvError::Closed) => break,
            }
        }
        (String::from_utf8_lossy(&output).into_owned(), None)
    }

    #[test]
    fn a_shell_runs_prints_and_exits_with_its_code() {
        let (commands, command_receiver) = mpsc::channel();
        let (events, event_receiver) = async_channel::unbounded();
        let transport = cmd_transport("echo ready& exit 7");
        let worker =
            thread::spawn(move || transport.run(TerminalSize::DEFAULT, command_receiver, events));
        let (output, code) = drain_until_exit(&event_receiver, &commands);
        assert_eq!(code, Some(7), "{output}");
        assert!(output.contains("ready"), "{output}");
        worker.join().unwrap().unwrap();
    }

    /// 关闭卡片发的就是 `Shutdown`：shell 被结束、等到，传输线程跟着返回。
    #[test]
    fn shutdown_ends_the_shell() {
        let (commands, command_receiver) = mpsc::channel();
        let (events, event_receiver) = async_channel::unbounded();
        // 不关就会等 30 秒。
        let transport = cmd_transport("echo ready& ping -n 30 127.0.0.1 >nul");
        let started = Instant::now();
        let worker =
            thread::spawn(move || transport.run(TerminalSize::DEFAULT, command_receiver, events));
        assert_eq!(
            event_receiver.recv_blocking().unwrap(),
            TerminalTransportEvent::Started
        );
        commands.send(TerminalTransportCommand::Shutdown).unwrap();
        worker.join().unwrap().unwrap();
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    /// 别的进程的当前目录读的是它自己 PEB 里的 `CurrentDirectory`（工单 05：在同一个目录分屏）。
    #[test]
    fn the_folder_of_another_process_is_read_from_its_memory() {
        let folder = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "ping -n 30 127.0.0.1 >nul"])
            .current_dir(folder.path())
            .spawn()
            .unwrap();
        let read = working_directory(child.id());
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(
            read.as_deref().map(comparable),
            Some(comparable(folder.path()))
        );
        // 自己的也读得到；不存在的进程读不到。
        assert_eq!(
            working_directory(std::process::id())
                .as_deref()
                .map(comparable),
            Some(comparable(&std::env::current_dir().unwrap()))
        );
        assert_eq!(working_directory(u32::MAX - 3), None);
    }

    /// 比较路径时不管大小写和末尾的 `\`：Windows 的路径本来就不分大小写。
    fn comparable(path: &std::path::Path) -> String {
        path.display()
            .to_string()
            .trim_end_matches('\\')
            .to_lowercase()
    }

    /// shell 换了目录，传输就告诉卡片：Windows 上和 Linux、macOS 一样发 `WorkingDirectory`。
    #[test]
    fn the_shells_folder_is_told_as_it_moves() {
        let start = tempfile::tempdir().unwrap();
        let next = tempfile::tempdir().unwrap();
        let script = format!(
            "echo a& ping -n 2 127.0.0.1 >nul& cd /d {}& echo b& ping -n 2 127.0.0.1 >nul",
            next.path().display()
        );
        let launch = LocalLaunch {
            directory: Some(start.path().to_path_buf()),
            ..LocalLaunch::new(LocalShell::new(&format!("cmd.exe /d /c {script}")))
        };
        let transport = LocalPtyTransportProvider.factory_for(&launch).create();
        let (commands, command_receiver) = mpsc::channel();
        let (events, event_receiver) = async_channel::unbounded();
        let worker =
            thread::spawn(move || transport.run(TerminalSize::DEFAULT, command_receiver, events));

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut folders = Vec::new();
        while Instant::now() < deadline {
            match event_receiver.try_recv() {
                Ok(TerminalTransportEvent::Output(bytes)) => {
                    if bytes.windows(4).any(|window| window == b"\x1b[6n") {
                        let _ =
                            commands.send(TerminalTransportCommand::Write(b"\x1b[1;1R".to_vec()));
                    }
                }
                Ok(TerminalTransportEvent::WorkingDirectory(folder)) => {
                    folders.push(comparable(&folder))
                }
                Ok(TerminalTransportEvent::Exited { .. })
                | Err(async_channel::TryRecvError::Closed) => break,
                Ok(_) => {}
                Err(async_channel::TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
            }
        }
        let _ = commands.send(TerminalTransportCommand::Shutdown);
        worker.join().unwrap().unwrap();
        assert_eq!(folders, [comparable(start.path()), comparable(next.path())]);
    }

    /// shell 跑起程序、程序结束：传输先说前台是那个程序，再说回到了 shell（工单 06）。
    #[test]
    fn the_foreground_program_is_told_as_it_comes_and_goes() {
        let _forks = crate::testing::no_forks();
        let script = "echo a& ping -n 3 127.0.0.1 >nul& echo b& ping -n 3 127.0.0.1 >nul";
        let transport = cmd_transport(script);
        let (commands, command_receiver) = mpsc::channel();
        let (events, event_receiver) = async_channel::unbounded();
        let worker =
            thread::spawn(move || transport.run(TerminalSize::DEFAULT, command_receiver, events));

        let deadline = Instant::now() + Duration::from_secs(15);
        let mut programs = Vec::new();
        while Instant::now() < deadline {
            match event_receiver.try_recv() {
                Ok(TerminalTransportEvent::Output(bytes)) => {
                    if bytes.windows(4).any(|window| window == b"\x1b[6n") {
                        let _ =
                            commands.send(TerminalTransportCommand::Write(b"\x1b[1;1R".to_vec()));
                    }
                }
                Ok(TerminalTransportEvent::ForegroundProgram(program)) => {
                    programs.push(program.map(|name| name.to_lowercase()))
                }
                Ok(TerminalTransportEvent::Exited { .. })
                | Err(async_channel::TryRecvError::Closed) => break,
                Ok(_) => {}
                Err(async_channel::TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
            }
        }
        let _ = commands.send(TerminalTransportCommand::Shutdown);
        worker.join().unwrap().unwrap();
        // 两次 ping 之间可能来不及看到 shell 自己在前台；只要求看到过 ping，且不会连着说两遍同一个。
        assert!(programs.contains(&Some("ping".into())), "{programs:?}");
        assert!(
            programs.windows(2).all(|pair| pair[0] != pair[1]),
            "{programs:?}"
        );
    }

    #[test]
    fn a_command_that_cannot_start_says_which() {
        let (_commands, command_receiver) = mpsc::channel();
        let (events, _event_receiver) = async_channel::unbounded();
        let error = LocalPtyTransportProvider
            .factory_for(&LocalLaunch::new(LocalShell::new(
                r"C:\no\such\shell.exe -l",
            )))
            .create()
            .run(TerminalSize::DEFAULT, command_receiver, events)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(r"无法启动 C:\no\such\shell.exe -l"),
            "{error}"
        );
    }
}

fn to_pty_size(size: TerminalSize) -> PtySize {
    let rows = size.rows().min(u16::MAX as usize) as u16;
    let cols = size.columns().min(u16::MAX as usize) as u16;
    PtySize {
        rows,
        cols,
        pixel_width: size.cell_width().saturating_mul(cols),
        pixel_height: size.cell_height().saturating_mul(rows),
    }
}
