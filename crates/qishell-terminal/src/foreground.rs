//! 本地终端的前台程序：shell 现在在跑什么（工单 06）。
//!
//! - Windows：从 shell 起，取它最新启动的子进程；是 `cmd.exe`（跑批处理的）就再往下找一层；
//! - macOS、Linux：PTY 的前台进程组（`tcgetpgrp`，portable-pty 的 `process_group_leader`）。
//!
//! 拿到进程以后按 `program_name` 起名：去掉 `.exe`，解释器跑的脚本用脚本名（`node …/bin/codex.js` 是 `codex`）。
//! shell 自己在前台（在提示符下）时是 `None`。
// 参考 gorex 的模型重写：internal/rex/proc.go、proc_windows.go、proc_linux.go、
// pty_windows.go 的 `foreground`。

use std::path::Path;

/// 跑脚本的解释器：它们的第一个参数比它们自己更能说明在跑什么。
const INTERPRETERS: &[&str] = &[
    "node", "bun", "deno", "python", "python3", "ruby", "perl", "php", "sh", "bash",
];

/// 进程最为人知的名字。`name` 是进程名（可能带 `.exe`、路径、登录 shell 的前导 `-`），
/// `args` 是它的命令行（`args[0]` 是程序自己）。
pub fn program_name(name: &str, args: &[String]) -> String {
    let mut program = file_stem(name);
    if let Some(first) = args.first() {
        let full = file_stem(first);
        // Linux 的 comm 只留 15 个字节：命令行里的名字更全时用它。
        if full.len() > program.len() && full.starts_with(&program) {
            program = full;
        }
    }
    if INTERPRETERS
        .iter()
        .any(|interpreter| program.eq_ignore_ascii_case(interpreter))
        && let Some(script) = args.iter().skip(1).find(|arg| !arg.starts_with('-'))
    {
        // `node …/bin/codex.js` 是 codex；`node demo/snake.mjs` 还是 node。
        let slashed = script.replace('\\', "/");
        if slashed.contains("/bin/") || slashed.contains("node_modules") {
            let mut name = base_name(script).to_string();
            for extension in [".js", ".mjs", ".cjs", ".ts", ".py", ".rb"] {
                if let Some(stem) = name.strip_suffix(extension) {
                    name = stem.to_string();
                    break;
                }
            }
            // npm 跑的是 …/npm/bin/npm-cli.js，没有叫 npm 的链接。
            if let Some(stem) = name.strip_suffix("-cli") {
                name = stem.to_string();
            }
            return name;
        }
    }
    program
}

/// 路径的最后一段：`/` 和 `\` 都算分隔符（远程和 Windows 的路径都会来）。
fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// 程序文件的名字：最后一段，去掉 `.exe`（不分大小写）和登录 shell 的前导 `-`。
fn file_stem(path: &str) -> String {
    let base = base_name(path).trim_start_matches('-');
    let stem = match Path::new(base).extension() {
        Some(extension) if extension.eq_ignore_ascii_case("exe") => {
            &base[..base.len() - extension.len() - 1]
        }
        _ => base,
    };
    stem.to_string()
}

/// 本地 shell 的前台程序。`shell` 是 shell 的进程号；shell 自己在前台时是 `None`。
#[cfg(windows)]
pub(crate) fn foreground_program(
    shell: u32,
    _master: &dyn portable_pty::MasterPty,
) -> Option<String> {
    windows::foreground(shell)
}

/// 本地 shell 的前台程序：PTY 的前台进程组组长；是 shell 自己就是 `None`。
#[cfg(unix)]
pub(crate) fn foreground_program(
    shell: u32,
    master: &dyn portable_pty::MasterPty,
) -> Option<String> {
    let leader = u32::try_from(master.process_group_leader()?).ok()?;
    if leader == shell {
        return None;
    }
    unix::program_of(leader)
}

#[cfg(not(any(windows, unix)))]
pub(crate) fn foreground_program(_: u32, _: &dyn portable_pty::MasterPty) -> Option<String> {
    None
}

#[cfg(unix)]
mod unix {
    use super::program_name;

    /// 进程 `pid` 的名字和命令行。
    #[cfg(target_os = "linux")]
    pub(super) fn program_of(pid: u32) -> Option<String> {
        let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        let args: Vec<String> = std::fs::read(format!("/proc/{pid}/cmdline"))
            .map(|line| {
                line.split(|byte| *byte == 0)
                    .filter(|arg| !arg.is_empty())
                    .map(|arg| String::from_utf8_lossy(arg).into_owned())
                    .collect()
            })
            .unwrap_or_default();
        Some(program_name(name.trim_end(), &args))
    }

    /// 进程 `pid` 的名字：可执行文件的路径（`proc_pidpath`）。
    #[cfg(target_os = "macos")]
    pub(super) fn program_of(pid: u32) -> Option<String> {
        let pid = libc::c_int::try_from(pid).ok()?;
        let mut buffer = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: 缓冲区是 `buffer`，长度如实传入。
        let written = unsafe {
            libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32)
        };
        if written <= 0 {
            return None;
        }
        buffer.truncate(written as usize);
        let path = String::from_utf8_lossy(&buffer).into_owned();
        Some(program_name(&path, &[]))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub(super) fn program_of(_: u32) -> Option<String> {
        None
    }
}

/// 把 Windows 的命令行拆成参数，规则同 `CommandLineToArgvW`：空白分开参数，双引号括起空白；
/// 引号前的 2n 个反斜杠是 n 个反斜杠，2n+1 个是 n 个反斜杠加一个字面的引号；别处的反斜杠照原样。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn split_command_line(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    // 当前参数里有没有东西：`""` 也是一个（空的）参数。
    let mut started = false;
    let mut quoted = false;
    let mut backslashes = 0_usize;
    for character in line.chars() {
        match character {
            '\\' => {
                backslashes += 1;
                started = true;
                continue;
            }
            '"' => {
                current.extend(std::iter::repeat_n('\\', backslashes / 2));
                if backslashes % 2 == 1 {
                    current.push('"');
                } else {
                    quoted = !quoted;
                }
                started = true;
            }
            ' ' | '\t' if !quoted => {
                current.extend(std::iter::repeat_n('\\', backslashes));
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                current.extend(std::iter::repeat_n('\\', backslashes));
                current.push(character);
                started = true;
            }
        }
        backslashes = 0;
    }
    current.extend(std::iter::repeat_n('\\', backslashes));
    if started {
        args.push(current);
    }
    args
}

#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::ffi::c_void;

    use windows_sys::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessCommandLineInformation,
    };
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use super::{INTERPRETERS, program_name, split_command_line};

    /// 用完关掉的句柄。
    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: 句柄是打开来的，只关这一次。
            unsafe { CloseHandle(self.0) };
        }
    }

    /// 系统里的进程：谁是谁的子进程，叫什么（`szExeFile`，只有文件名）。
    struct Processes {
        children: HashMap<u32, Vec<u32>>,
        names: HashMap<u32, String>,
    }

    fn processes() -> Option<Processes> {
        // SAFETY: 只取进程的快照，失败返回 INVALID_HANDLE_VALUE。
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let snapshot = Handle(snapshot);
        let mut processes = Processes {
            children: HashMap::new(),
            names: HashMap::new(),
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        // SAFETY: `entry` 的 dwSize 已经填好。
        let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        while more {
            let pid = entry.th32ProcessID;
            if pid != entry.th32ParentProcessID {
                processes
                    .children
                    .entry(entry.th32ParentProcessID)
                    .or_default()
                    .push(pid);
            }
            let length = entry
                .szExeFile
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(entry.szExeFile.len());
            processes
                .names
                .insert(pid, String::from_utf16_lossy(&entry.szExeFile[..length]));
            // SAFETY: 同上。
            more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        Some(processes)
    }

    /// 进程什么时候启动的（100 纳秒为单位）；打不开就是 `None`。
    fn started(pid: u32) -> Option<u64> {
        // SAFETY: 只读权限打开，失败返回空句柄。
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let handle = Handle(handle);
        let mut times = [FILETIME::default(); 4];
        let [created, exited, kernel, user] = &mut times;
        // SAFETY: 四个都是可写的 FILETIME。
        if unsafe { GetProcessTimes(handle.0, created, exited, kernel, user) } == 0 {
            return None;
        }
        Some(u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
    }

    impl Processes {
        /// `parent` 最新启动的子进程。比 `parent` 启动得早的不算：它只是占了
        /// `parent` 以前那个同号进程的子进程位置。
        fn newest_child(&self, parent: u32) -> Option<u32> {
            let born = started(parent)?;
            self.children
                .get(&parent)?
                .iter()
                .filter_map(|child| Some((*child, started(*child)?)))
                .filter(|(_, at)| *at >= born)
                .max_by_key(|(_, at)| *at)
                .map(|(child, _)| child)
        }
    }

    /// shell `shell` 在跑的程序：它最新的子进程，跳过跑批处理的 `cmd.exe`；没有子进程是 `None`。
    pub(super) fn foreground(shell: u32) -> Option<String> {
        let processes = processes()?;
        let mut foreground = None;
        let mut parent = shell;
        while let Some(child) = processes.newest_child(parent) {
            foreground = Some(child);
            parent = child;
            let name = processes.names.get(&child).map(String::as_str);
            if !name.is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe")) {
                break;
            }
        }
        let pid = foreground?;
        let name = processes.names.get(&pid)?;
        let plain = program_name(name, &[]);
        // 只有解释器才要看命令行：它跑的脚本才是程序。
        let args = if INTERPRETERS
            .iter()
            .any(|interpreter| plain.eq_ignore_ascii_case(interpreter))
        {
            command_line(pid).map(|line| split_command_line(&line))
        } else {
            None
        };
        Some(program_name(name, &args.unwrap_or_default()))
    }

    /// 进程 `pid` 的命令行（`ProcessCommandLineInformation`）。
    fn command_line(pid: u32) -> Option<String> {
        // SAFETY: 只读权限打开，失败返回空句柄。
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let handle = Handle(handle);
        /// 结果开头是一个 `UNICODE_STRING`，文字紧跟在后面（`buffer` 指向这块内存里）。
        #[repr(C)]
        struct UnicodeString {
            length: u16,
            maximum_length: u16,
            buffer: *const u16,
        }
        let mut buffer = vec![0_u64; 1024];
        loop {
            let size = (buffer.len() * size_of::<u64>()) as u32;
            let mut needed = 0_u32;
            // SAFETY: `buffer` 可写，大小如实传入。
            let status = unsafe {
                NtQueryInformationProcess(
                    handle.0,
                    ProcessCommandLineInformation,
                    buffer.as_mut_ptr().cast::<c_void>(),
                    size,
                    &mut needed,
                )
            };
            if status >= 0 {
                break;
            }
            // STATUS_INFO_LENGTH_MISMATCH：放大了再来一次。
            if needed > size && needed < 1 << 20 {
                buffer = vec![0_u64; (needed as usize).div_ceil(size_of::<u64>())];
                continue;
            }
            return None;
        }
        // SAFETY: 成功时缓冲区开头就是 UNICODE_STRING，按 8 字节对齐。
        let text = unsafe { &*buffer.as_ptr().cast::<UnicodeString>() };
        let start = buffer.as_ptr().cast::<u8>() as usize;
        let end = start + buffer.len() * size_of::<u64>();
        let units = usize::from(text.length) / 2;
        let at = text.buffer as usize;
        if text.length == 0 || at < start || at + units * 2 > end {
            return None;
        }
        // SAFETY: 刚核对过文字整个落在 `buffer` 里。
        let line = unsafe { std::slice::from_raw_parts(text.buffer, units) };
        let _ = text.maximum_length;
        Some(String::from_utf16_lossy(line))
    }

    #[cfg(test)]
    mod tests {
        use super::foreground;

        /// cmd.exe 跑着 ping：前台程序是 ping（跳过 cmd.exe 那一层是 shell 起批处理的情形，
        /// 这里 cmd.exe 本身就是被查的 shell）。
        #[test]
        fn the_newest_child_of_the_shell_is_its_foreground_program() {
            let _forks = crate::testing::no_forks();
            let mut child = std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "ping -n 30 127.0.0.1 >nul"])
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut seen = None;
            while std::time::Instant::now() < deadline {
                seen = foreground(child.id());
                if seen.is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let _ = child.kill();
            let _ = child.wait();
            assert_eq!(seen.map(|name| name.to_lowercase()).as_deref(), Some("ping"));
            // 不存在的进程什么都没有。
            assert_eq!(foreground(u32::MAX - 3), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{program_name, split_command_line};

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn a_program_is_named_by_its_file_without_exe() {
        assert_eq!(program_name("nvim.exe", &[]), "nvim");
        assert_eq!(
            program_name(r"C:\Program Files\Git\bin\git.EXE", &[]),
            "git"
        );
        assert_eq!(program_name("/usr/bin/htop", &[]), "htop");
        assert_eq!(program_name("-zsh", &[]), "zsh");
    }

    #[test]
    fn a_script_an_interpreter_runs_from_a_bin_folder_names_the_program() {
        assert_eq!(
            program_name(
                "node.exe",
                &args(&[
                    "node",
                    r"C:\Users\qi\AppData\Roaming\npm\node_modules\@openai\codex\bin\codex.js",
                ])
            ),
            "codex"
        );
        assert_eq!(
            program_name("node", &args(&["node", "--no-warnings", "/usr/lib/node_modules/npm/bin/npm-cli.js"])),
            "npm"
        );
        // 自己的脚本：还是 node。
        assert_eq!(program_name("node", &args(&["node", "demo/snake.mjs"])), "node");
        assert_eq!(program_name("node", &[]), "node");
    }

    #[test]
    fn a_name_cut_short_is_completed_from_the_command_line() {
        // Linux 的 comm 只留 15 个字节。
        assert_eq!(
            program_name("redis-benchmar", &args(&["/usr/bin/redis-benchmark", "-q"])),
            "redis-benchmark"
        );
    }

    #[test]
    fn a_windows_command_line_splits_as_programs_split_it() {
        assert_eq!(
            split_command_line(r#""C:\Program Files\nodejs\node.exe" "C:\a b\codex.js" --x"#),
            args(&[r"C:\Program Files\nodejs\node.exe", r"C:\a b\codex.js", "--x"])
        );
        assert_eq!(
            split_command_line(r#"a\\\"b c\d "e\\" f"#),
            args(&[r#"a\"b"#, r"c\d", r"e\", "f"])
        );
        assert_eq!(split_command_line("  "), Vec::<String>::new());
    }
}
