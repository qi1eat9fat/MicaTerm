//! 本地 shell 报告当前目录（工单 31）：Windows PowerShell 5.1 的 `cd`（`Set-Location`）
//! 不改进程的当前目录，读 PEB 拿不到，所以 MicaTerm 启动本地 shell 时让它在每个提示符前
//! 发一条 OSC 9;9（`ESC ] 9 ; 9 ; "<路径>" ESC \`），卡片按它在同一个目录里分屏。
//!
//! 只改启动方式，不碰用户的任何文件：
//! - PowerShell 5.1 / pwsh 7：照常加载 `$PROFILE`，之后用 `-NoExit -EncodedCommand` 在会话里
//!   给 `prompt` 套一层：先报目录，再原样调用原来的 `prompt`（oh-my-posh、starship 之类照常生效），
//!   `$?` 先放回原样；出错就什么都不报。
//! - cmd：`PROMPT` 环境变量在原来的提示符前加 `$e]9;9;"$P"$e\`。
//!
//! 别的 shell（Git Bash、WSL……）不动。设置里写了自己的参数（`-Command`、`-File` 之类）的
//! PowerShell 也不动：那是用户自己要它做的事。

// 只在 Windows 上接（`local_pty::shell_command`）；别的系统上只有测试用到。
#![cfg_attr(not(windows), allow(dead_code))]

use std::ffi::OsStr;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use portable_pty::CommandBuilder;

/// 认得出、能接上目录报告的 shell。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shell {
    /// Windows PowerShell 5.1 和 PowerShell 7（`pwsh`）。
    PowerShell,
    /// 命令提示符。
    Cmd,
}

/// 设置里跟在 PowerShell 后面、不妨碍接上目录报告的参数（不分大小写，`-` 或 `/` 开头）。
const PLAIN_POWERSHELL_SWITCHES: &[&str] = &[
    "nologo",
    "nol",
    "noprofile",
    "nop",
    "noexit",
    "noe",
    "sta",
    "mta",
    // pwsh 7：当登录 shell 启动。
    "login",
    "l",
];

/// 跟着一个值的参数：`-ExecutionPolicy Bypass`、`-WindowStyle Hidden`、
/// pwsh 7 的 `-WorkingDirectory <目录>`。
const POWERSHELL_OPTIONS_WITH_A_VALUE: &[&str] = &[
    "executionpolicy",
    "ep",
    "ex",
    "windowstyle",
    "w",
    "workingdirectory",
    "wd",
];

/// 给本地 shell 接上目录报告：认得的 shell 才改，改不了就原样。
pub(crate) fn report_directory(command: &mut CommandBuilder) {
    let program = if command.is_default_prog() {
        command.get_shell().into()
    } else {
        match command.get_argv().first() {
            Some(program) => program.clone(),
            None => return,
        }
    };
    match shell_of(&program) {
        Some(Shell::PowerShell) => {
            if plain_powershell_arguments(&command.get_argv()[1..]) {
                command.args([
                    "-NoExit",
                    "-EncodedCommand",
                    &encoded_command(POWERSHELL_PROMPT),
                ]);
            }
        }
        Some(Shell::Cmd) => {
            let prompt = command
                .get_env("PROMPT")
                .map(|prompt| prompt.to_string_lossy().into_owned());
            if let Some(prompt) = cmd_prompt(prompt.as_deref()) {
                command.env("PROMPT", prompt);
            }
        }
        None => {}
    }
}

/// `program` 是哪种 shell：看文件名，不分大小写，`.exe` 可有可无。
fn shell_of(program: &OsStr) -> Option<Shell> {
    let name = Path::new(program)
        .file_name()?
        .to_str()?
        .to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    match name {
        "powershell" | "pwsh" => Some(Shell::PowerShell),
        "cmd" => Some(Shell::Cmd),
        _ => None,
    }
}

/// PowerShell 后面的参数都是不妨碍的那几个：可以在最后加上 `-NoExit -EncodedCommand`。
fn plain_powershell_arguments(arguments: &[std::ffi::OsString]) -> bool {
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let Some(argument) = argument.to_str() else {
            return false;
        };
        let Some(name) = argument
            .strip_prefix('-')
            .or_else(|| argument.strip_prefix('/'))
        else {
            return false;
        };
        let name = name.to_ascii_lowercase();
        if PLAIN_POWERSHELL_SWITCHES.contains(&name.as_str()) {
            continue;
        }
        if POWERSHELL_OPTIONS_WITH_A_VALUE.contains(&name.as_str()) && arguments.next().is_some() {
            continue;
        }
        return false;
    }
    true
}

/// 套在用户的 `prompt` 外面的一层，在用户的 `$PROFILE` 之后跑。
///
/// - 原来的 `prompt` 存在全局变量里，新的先报目录（只报文件系统里的：`HKLM:\` 之类没有目录可言），
///   用 `$Host.UI.Write` 写，不改原来 `prompt` 返回的内容；
/// - 报之前记下 `$?`，调原来的 `prompt` 之前放回去（VS Code 的 shell 集成也这么做），
///   `$LASTEXITCODE` 不碰；
/// - 没有原来的 `prompt` 时用 PowerShell 自带的样子；整段出错就什么都不改。
pub(crate) const POWERSHELL_PROMPT: &str = r#"try {
    if (-not $global:__MicaTermOriginalPrompt) {
        $global:__MicaTermOriginalPrompt = if ($function:prompt) { $function:prompt } else {
            { "PS $($executionContext.SessionState.Path.CurrentLocation)$('>' * ($nestedPromptLevel + 1)) " }
        }
        function global:prompt {
            $__micaterm_succeeded = $global:?
            try {
                $__micaterm_here = $executionContext.SessionState.Path.CurrentLocation
                if ($__micaterm_here.Provider.Name -eq 'FileSystem') {
                    $Host.UI.Write("$([char]27)]9;9;`"$($__micaterm_here.ProviderPath)`"$([char]27)\")
                }
            } catch {}
            $__micaterm_prompt = $global:__MicaTermOriginalPrompt
            if (-not $__micaterm_succeeded) { Write-Error '' -ErrorAction Ignore }
            & $__micaterm_prompt
        }
    }
} catch {}"#;

/// `-EncodedCommand` 要的写法：UTF-16LE 的 Base64。
pub(crate) fn encoded_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    STANDARD.encode(bytes)
}

/// cmd 的 `PROMPT`：在 `prompt`（没有时是 cmd 默认的 `$P$G`）前加一条 OSC 9;9。
/// 已经报目录的不再加，是 `None`。
fn cmd_prompt(prompt: Option<&str>) -> Option<String> {
    const REPORT: &str = r#"$e]9;9;"$P"$e\"#;
    let prompt = prompt.filter(|prompt| !prompt.is_empty()).unwrap_or("$P$G");
    (!prompt.contains("]9;9;")).then(|| format!("{REPORT}{prompt}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(command: &CommandBuilder) -> Vec<String> {
        command
            .get_argv()
            .iter()
            .map(|word| word.to_string_lossy().into_owned())
            .collect()
    }

    fn integrated(words: &[&str]) -> CommandBuilder {
        let mut command = CommandBuilder::from_argv(words.iter().map(Into::into).collect());
        report_directory(&mut command);
        command
    }

    #[test]
    fn powershell_runs_the_prompt_wrapper_after_its_own_arguments() {
        for program in [
            "powershell.exe",
            r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.EXE",
            "pwsh",
            r"C:\Program Files\PowerShell\7\pwsh.exe",
        ] {
            assert_eq!(
                argv(&integrated(&[program, "-NoLogo"])),
                [
                    program,
                    "-NoLogo",
                    "-NoExit",
                    "-EncodedCommand",
                    &encoded_command(POWERSHELL_PROMPT)
                ]
            );
        }
        assert_eq!(
            argv(&integrated(&[
                "pwsh.exe",
                "/nop",
                "-ExecutionPolicy",
                "Bypass"
            ]))
            .len(),
            7
        );
    }

    /// pwsh 的 `-Login`、`-WorkingDirectory <目录>`、`-WindowStyle <样式>`（及缩写）不妨碍接上。
    #[test]
    fn pwsh_login_working_directory_and_window_style_still_report() {
        for words in [
            &["pwsh.exe", "-Login", "-WorkingDirectory", r"D:\work dir"][..],
            &["pwsh.exe", "-l", "-wd", "~"],
            &["powershell.exe", "-WindowStyle", "Hidden", "-NoLogo"],
            &["pwsh.exe", "-w", "Normal"],
        ] {
            let argv = argv(&integrated(words));
            assert_eq!(argv[..words.len()], *words, "{words:?}");
            assert_eq!(
                argv[words.len()..words.len() + 2],
                ["-NoExit", "-EncodedCommand"]
            );
        }
        // 缺了值的不认。
        assert_eq!(
            argv(&integrated(&["pwsh.exe", "-WorkingDirectory"])).len(),
            2
        );
    }

    /// 用户自己让 PowerShell 跑什么（`-Command`、`-File`……），就不动它。
    #[test]
    fn powershell_told_what_to_run_is_left_alone() {
        for words in [
            &["pwsh.exe", "-Command", "Get-Date"][..],
            &["powershell.exe", "-NoExit", "-File", "x.ps1"],
            &["powershell.exe", "-c", "dir"],
            &["pwsh.exe", "x.ps1"],
            &["pwsh.exe", "-ExecutionPolicy"],
        ] {
            assert_eq!(argv(&integrated(words)), words, "{words:?}");
        }
    }

    #[test]
    fn other_shells_are_left_alone() {
        for words in [
            &[r"C:\Program Files\Git\bin\bash.exe", "-l"][..],
            &["wsl.exe"],
            &["nu"],
        ] {
            let command = integrated(words);
            assert_eq!(argv(&command), words);
        }
    }

    #[test]
    fn cmd_reports_before_its_prompt() {
        assert_eq!(cmd_prompt(None).as_deref(), Some(r#"$e]9;9;"$P"$e\$P$G"#));
        assert_eq!(
            cmd_prompt(Some("")).as_deref(),
            Some(r#"$e]9;9;"$P"$e\$P$G"#)
        );
        assert_eq!(
            cmd_prompt(Some("$T $P$_$G")).as_deref(),
            Some(r#"$e]9;9;"$P"$e\$T $P$_$G"#)
        );
        // 已经在报的不加第二遍。
        assert_eq!(cmd_prompt(Some(r"$e]9;9;$P$e\$P$G")), None);

        let mut command = CommandBuilder::from_argv(vec!["CMD.EXE".into(), "/k".into()]);
        command.env("PROMPT", "[$P]");
        report_directory(&mut command);
        assert_eq!(
            command.get_env("PROMPT").and_then(OsStr::to_str),
            Some(r#"$e]9;9;"$P"$e\[$P]"#)
        );
        assert_eq!(argv(&command), ["CMD.EXE", "/k"]);
    }

    #[test]
    fn the_encoded_command_is_base64_of_utf16() {
        // `[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes(...))` 算的。
        assert_eq!(encoded_command("dir"), "ZABpAHIA");
        assert_eq!(encoded_command("cd 中文"), "YwBkACAALU6HZQ==");
        assert_eq!(encoded_command(""), "");
    }
}
