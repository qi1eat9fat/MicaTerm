//! 远程卡片的被动解析（工单 17）：只读远程自己发出的东西，猜它在跑什么程序、在哪个目录。
//! QiShell 不往远程写任何东西，也不另开通道（设计工单 12 的决定）。
//!
//! - 程序，按优先级：shell 集成报告的命令行（OSC 133;C 的 `cmdline=` / `cmdline_url=`、
//!   OSC 633;E、OSC 3008 的 `comm=` / `cmdline=`），取第一个真正的命令，跳过 `sudo`、`env`、
//!   `VAR=` 这类前缀；其次是程序设的窗口标题（` - VIM`、` - Nvim` 结尾，fish / oh-my-zsh 设成命令行
//!   的标题取第一个词）；只知道进了全屏（alt screen）时记作「在运行」。
//! - 目录，按优先级：OSC 7 / 1337 / 633;P / 3008 / 9;9 报告的（`directory`，优先级在
//!   `NoticeScanner` 里记着）；都没有时从 `user@host: 目录` 格式的标题解析，只用来显示，
//!   以 `/` 或 `~/` 开头时才能用作 SFTP 和分屏的初始目录。
//! - 报告带主机名时和 `uname -n` 核对，对不上（在卡片里又 ssh 到了别处）就不更新目录。
//!
//! 全是纯函数和纯状态，不依赖 gpui：`NoticeScanner` 在解析线程上找出序列，`ShellActivity`
//! 由卡片拿着，把序列、标题、全屏状态合起来。
// 前缀跳过的规则参考 oh-my-zsh 的 lib/termsupport.zsh；「像不像 shell 设的标题」参考 gorex 的
// state.go `titleOfShell`，按 QiShell 的需要重写。

use percent_encoding::percent_decode_str;

use crate::directory::{context_fields, joined, unescape};
use crate::ReportedDirectory;

/// shell 集成说的「命令在跑 / 回到提示符」。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellCommand {
    /// 在提示符下（OSC 133;A/B/D、633;A/B/D、3008 的 shell 上下文、上下文结束）。
    Prompt,
    /// 有命令在跑：第一个真正的命令的名字；集成没说是什么命令时是 `None`
    /// （VTE 的 `133;C`、systemd 自带脚本的 `3008;type=command`）。
    Running(Option<String>),
}

impl ShellCommand {
    /// 接在 `before` 后面收到的 `self`：同一条命令先报了名字（633;E）、又只报「开始执行」（633;C）
    /// 时，名字留着。
    pub fn after(self, before: Option<&ShellCommand>) -> ShellCommand {
        match (self, before) {
            (ShellCommand::Running(None), Some(ShellCommand::Running(Some(name)))) => {
                ShellCommand::Running(Some(name.clone()))
            }
            (next, _) => next,
        }
    }
}

/// OSC 序列（按 `;` 切开的参数）报告的命令状态，不是这类序列时是 `None`。
pub(crate) fn shell_command(params: &[&[u8]]) -> Option<ShellCommand> {
    let second = params.get(1).copied();
    match *params.first()? {
        b"133" => match second? {
            b"A" | b"B" | b"D" => Some(ShellCommand::Prompt),
            b"C" => {
                // `133;C;cmdline=<shell 转义>` 或 `133;C;cmdline_url=<URL 编码>`；命令里的 `;`
                // 被解析器切开了，拼回去（超过 16 段的尾巴已经丢了，只取第一个命令，不碍事）。
                let rest = joined(params, 2).unwrap_or_default();
                let name = if let Some(line) = rest.strip_prefix("cmdline_url=") {
                    percent_decode_str(line)
                        .decode_utf8()
                        .ok()
                        .and_then(|line| command_name(&line))
                } else if let Some(line) = rest.strip_prefix("cmdline=") {
                    command_name(&shell_words(line).join(" "))
                } else {
                    None
                };
                Some(ShellCommand::Running(name))
            }
            _ => None,
        },
        b"633" => match second? {
            b"A" | b"B" | b"D" => Some(ShellCommand::Prompt),
            b"C" => Some(ShellCommand::Running(None)),
            // `633;E;<命令行>;<nonce>`：命令行里的 `;` 转义成 `\x3b`，不会被切开。
            b"E" => {
                let line = std::str::from_utf8(params.get(2)?).ok()?;
                Some(ShellCommand::Running(
                    unescape(line).and_then(|line| command_name(&line)),
                ))
            }
            _ => None,
        },
        b"3008" => {
            let fields = context_fields(params);
            if fields.contains_key("end") {
                return Some(ShellCommand::Prompt);
            }
            fields.get("start")?;
            match fields.get("type").copied() {
                Some("shell") => Some(ShellCommand::Prompt),
                Some("command") => {
                    let field = |key| fields.get(key).and_then(|value| unescape(value));
                    let name = field("cmdline")
                        .and_then(|line| command_name(&line))
                        .or_else(|| field("comm").and_then(|comm| command_name(&comm)));
                    Some(ShellCommand::Running(name))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// 命令行里第一个真正的命令的名字（去掉路径）：跳过 `VAR=值`，以及 `sudo`、`doas`、`env`、`nohup`、
/// `time`、`exec`、`command`、`nice` 和它们的选项。只有前缀、没有命令时是 `None`。
pub fn command_name(line: &str) -> Option<String> {
    let words = shell_words(line);
    let mut at = 0;
    while let Some(word) = words.get(at).map(String::as_str) {
        at += 1;
        if is_assignment(word) {
            continue;
        }
        let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
        // 这些前缀的选项里，后面还跟着一个参数的。
        let takes_value: &[&str] = match name {
            "sudo" => &["-u", "-g", "-h", "-p", "-C", "-D", "-r", "-t", "-U", "-T", "-R"],
            "doas" => &["-u", "-C"],
            "env" => &["-u", "-C", "-S", "--unset", "--chdir", "--split-string"],
            "nice" => &["-n", "--adjustment"],
            "nohup" | "time" | "exec" | "command" | "builtin" => &[],
            _ => return (!name.is_empty()).then(|| name.to_string()),
        };
        // 跳过前缀的选项（`env` 后面还有 `VAR=值`，外层循环会跳）。
        while let Some(option) = words.get(at).map(String::as_str) {
            if option == "--" {
                at += 1;
                break;
            }
            if !option.starts_with('-') || option == "-" {
                break;
            }
            at += if takes_value.contains(&option) { 2 } else { 1 };
        }
    }
    None
}

/// `NAME=值`：名字是字母、数字、下划线，不以数字开头。
fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !name.starts_with(|character: char| character.is_ascii_digit())
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// 按 shell 的规则把一行切成词：空白分词，认反斜杠、单引号、双引号和 `$'…'`（bash `%q` 的写法）。
/// 不展开变量，也不认 `;`、`|` 这些（第一个命令总在最前面）。
pub fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                in_word = true;
                match chars.next() {
                    // 反斜杠加换行是续行。
                    Some('\n') | None => {}
                    Some(next) => word.push(next),
                }
            }
            '\'' => {
                in_word = true;
                for next in chars.by_ref() {
                    if next == '\'' {
                        break;
                    }
                    word.push(next);
                }
            }
            '"' => {
                in_word = true;
                while let Some(next) = chars.next() {
                    match next {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) => {
                            word.extend(chars.next());
                        }
                        _ => word.push(next),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_word = true;
                ansi_c_quoted(&mut chars, &mut word);
            }
            character if character.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            character => {
                in_word = true;
                word.push(character);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

/// `$'…'` 里面的，到结尾的 `'` 为止：`\n`、`\t`、`\e`、`\xNN`、`\\`、`\'` 这些转义。
fn ansi_c_quoted(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, word: &mut String) {
    while let Some(next) = chars.next() {
        match next {
            '\'' => return,
            '\\' => match chars.next() {
                Some('n') => word.push('\n'),
                Some('t') => word.push('\t'),
                Some('r') => word.push('\r'),
                Some('e' | 'E') => word.push('\x1b'),
                Some('a') => word.push('\x07'),
                Some('x') => {
                    let hex: String = std::iter::from_fn(|| {
                        chars.next_if(|character| character.is_ascii_hexdigit())
                    })
                    .take(2)
                    .collect();
                    if let Some(character) = u32::from_str_radix(&hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                    {
                        word.push(character);
                    }
                }
                Some(other) => word.push(other),
                None => return,
            },
            _ => word.push(next),
        }
    }
}

/// 窗口标题说了什么。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TitleSays {
    /// shell 在提示符下设的标题：`user@host: 目录`（Debian/Ubuntu）、`user@host:目录`（RHEL、
    /// oh-my-zsh），或 fish 的 `[主机] 目录`。带主机名、目录的就是它们。
    Shell {
        host: Option<String>,
        directory: Option<String>,
    },
    /// 程序设的标题：vim / nvim 的 `… - VIM`，fish / oh-my-zsh 在命令运行时设成的命令行。
    /// `fish`：fish 的 `[主机] 命令` 格式（远程的 fish 总带主机）。
    Program { name: String, fish: bool },
    /// 认不出（或没有标题）。
    #[default]
    Unknown,
}

/// 读一个窗口标题。
pub fn read_title(title: &str) -> TitleSays {
    let title = title.trim();
    if title.is_empty() {
        return TitleSays::Unknown;
    }
    // vim 退出时恢复不了原标题就设成这句。
    if title == "Thanks for flying Vim" {
        return TitleSays::Shell {
            host: None,
            directory: None,
        };
    }
    for (suffix, name) in [(" - VIM", "vim"), (" - NVIM", "nvim")] {
        if let Some(at) = title.len().checked_sub(suffix.len())
            && title.is_char_boundary(at)
            && title[at..].eq_ignore_ascii_case(suffix)
        {
            return TitleSays::Program {
                name: name.into(),
                fish: false,
            };
        }
    }
    if let Some((host, directory)) = user_at_host(title) {
        return TitleSays::Shell {
            host: Some(host.to_string()),
            directory: (!directory.is_empty()).then(|| directory.to_string()),
        };
    }
    // fish：`[主机] 命令 目录`，在提示符下没有命令。
    let (rest, fish) = match title
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("] "))
    {
        Some((host, rest)) if !host.is_empty() && !host.contains(char::is_whitespace) => {
            (rest.trim_start(), true)
        }
        _ => (title, false),
    };
    let first = rest.split_whitespace().next().unwrap_or_default();
    if first.is_empty() || first.starts_with(['~', '/', '.', '…']) {
        return if fish {
            TitleSays::Shell {
                host: None,
                directory: None,
            }
        } else {
            TitleSays::Unknown
        };
    }
    // 标题不是 shell 转义过的：反斜杠是 Windows 路径（`C:\Windows\System32\cmd.exe`）。
    match command_name(&rest.replace('\\', "/")) {
        Some(name) if looks_like_a_command(&name) => TitleSays::Program { name, fish },
        _ => TitleSays::Unknown,
    }
}

/// `user@host: 目录` / `user@host:目录`：用户名和主机名里没有空白，用户名里没有 `@`。
fn user_at_host(title: &str) -> Option<(&str, &str)> {
    let (user, rest) = title.split_once('@')?;
    let (host, directory) = rest.split_once(':')?;
    let plain = |part: &str| !part.is_empty() && !part.contains(char::is_whitespace);
    (plain(user) && plain(host) && !host.contains('@'))
        .then(|| (host, directory.strip_prefix(' ').unwrap_or(directory)))
}

/// 像个命令名：字母或数字开头，只有字母、数字和 `.`、`_`、`+`、`-`。
fn looks_like_a_command(name: &str) -> bool {
    name.starts_with(|character: char| character.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._+-".contains(character))
}

/// 一张远程卡片被动解析出的东西：把序列报告的目录和命令、窗口标题、全屏状态合起来。
/// 一条连接一份，重连时换新的。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellActivity {
    /// 序列报告的目录（核对过主机名）。
    reported: Option<String>,
    /// 现在的窗口标题。
    title: Option<String>,
    /// 标题说了什么（`read_title(title)`）。
    title_says: TitleSays,
    /// 从标题解析出的目录（核对过主机名），只在没有序列报告时用。
    title_directory: Option<String>,
    /// 这条连接上见过 shell 设的标题：shell 会在提示符下改标题，别的标题就是程序设的。
    shell_title_seen: bool,
    /// shell 集成最近说的；`None` 是这条连接上还没见过集成。
    command: Option<ShellCommand>,
    /// 终端在备用屏幕上（全屏程序）。
    full_screen: bool,
}

impl ShellActivity {
    /// 序列报告了目录：带主机名的要和 `hostname`（`uname -n`）对得上。返回收没收下。
    pub fn report_directory(
        &mut self,
        directory: &ReportedDirectory,
        hostname: Option<&str>,
    ) -> bool {
        if !directory.is_from(hostname) {
            return false;
        }
        self.reported = Some(directory.path.clone());
        true
    }

    /// 窗口标题是 `title`（`hostname` 是 `uname -n`，核对标题里的主机名）。标题没变也可以再调：
    /// 探测出主机名以后，之前对不上的标题目录要重新核对。
    pub fn set_title(&mut self, title: Option<&str>, hostname: Option<&str>) {
        self.title = title.map(str::to_string);
        self.title_says = title.map_or(TitleSays::Unknown, read_title);
        if let TitleSays::Shell { host, directory } = &self.title_says {
            self.shell_title_seen = true;
            if let Some(directory) = directory
                && host.as_deref().is_none_or(|host| same_host(host, hostname))
            {
                self.title_directory = Some(directory.clone());
            }
        }
    }

    /// shell 集成说的命令状态。
    pub fn shell_command(&mut self, command: ShellCommand) {
        self.command = Some(command.after(self.command.as_ref()));
    }

    /// 终端在不在备用屏幕上。
    pub fn set_full_screen(&mut self, full_screen: bool) {
        self.full_screen = full_screen;
    }

    /// 正在跑的程序的名字（可能是个 shell，比如在卡片里又开了 `bash`）；拿不到名字时是 `None`。
    pub fn program(&self) -> Option<&str> {
        match &self.command {
            // 集成说在提示符下：标题可能还是上一条命令设的，不信。
            Some(ShellCommand::Prompt) => None,
            Some(ShellCommand::Running(Some(name))) => Some(name),
            // 集成只说了在跑（或没有集成）：看标题。
            Some(ShellCommand::Running(None)) | None => self.title_program(),
        }
    }

    /// 程序设的标题里的程序：vim 的标题、fish 的格式总算数；其余的只在这条连接上见过 shell
    /// 设的标题时才算（不然可能只是用户设的固定标题）。
    fn title_program(&self) -> Option<&str> {
        match &self.title_says {
            TitleSays::Program { name, fish } if *fish || self.shell_title_seen || is_vim(name) => {
                Some(name)
            }
            _ => None,
        }
    }

    /// 有程序在跑：知道名字的，集成说在跑的，或者在全屏里的。
    pub fn running(&self) -> bool {
        self.program().is_some()
            || matches!(self.command, Some(ShellCommand::Running(_)))
            || self.full_screen
    }

    /// 显示用的目录：序列报告的优先，其次是标题里的。
    pub fn directory(&self) -> Option<&str> {
        self.reported
            .as_deref()
            .or(self.title_directory.as_deref())
    }

    /// 能用作 SFTP 和分屏初始目录的：序列报告的；标题里的只有 `/` 或 `~/` 开头才算。
    pub fn start_directory(&self) -> Option<&str> {
        self.reported.as_deref().or(self
            .title_directory
            .as_deref()
            .filter(|directory| directory.starts_with('/') || directory.starts_with("~/")))
    }

    /// 现在的窗口标题（悬停卡片头部时显示完整的）。
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
}

/// vim / nvim 自己设的标题（`… - VIM`）总能认出来，不用先见过 shell 设的标题。
fn is_vim(name: &str) -> bool {
    name == "vim" || name == "nvim"
}

/// 标题里的主机名和 `uname -n` 是不是同一台：标题常用短主机名（RHEL 的 `${HOSTNAME%%.*}`、
/// bash 的 `\h`），对得上全名或第一段都算。不知道 `uname -n` 时不信。
fn same_host(host: &str, hostname: Option<&str>) -> bool {
    hostname.is_some_and(|name| {
        name.eq_ignore_ascii_case(host)
            || name
                .split('.')
                .next()
                .is_some_and(|short| short.eq_ignore_ascii_case(host))
    })
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::vte::{Params, Parser, Perform};

    use super::*;
    use crate::notices::NoticeScanner;

    /// 一张远程卡片看到的输出：序列经 `NoticeScanner`（和引擎的解析线程一样），标题和备用屏幕
    /// 在这里自己认（引擎里由 alacritty 认）。
    struct Remote {
        scanner: NoticeScanner,
        parser: Parser,
        screen: Screen,
        hostname: Option<&'static str>,
        activity: ShellActivity,
    }

    #[derive(Default)]
    struct Screen {
        title: Option<String>,
        full: bool,
    }

    impl Perform for Screen {
        fn osc_dispatch(&mut self, params: &[&[u8]], _: bool) {
            if matches!(params.first(), Some(&b"0" | &b"2")) {
                self.title = joined(params, 1);
            }
        }

        fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _: bool, action: char) {
            let alternate = params
                .iter()
                .any(|param| matches!(param, [47] | [1047] | [1049]));
            if intermediates == b"?" && alternate && matches!(action, 'h' | 'l') {
                self.full = action == 'h';
            }
        }
    }

    impl Remote {
        fn on(hostname: Option<&'static str>) -> Self {
            Self {
                scanner: NoticeScanner::default(),
                parser: Parser::new(),
                screen: Screen::default(),
                hostname,
                activity: ShellActivity::default(),
            }
        }

        fn feed(&mut self, bytes: &[u8]) -> &mut Self {
            self.scanner.scan(bytes);
            self.parser.advance(&mut self.screen, bytes);
            if let Some(directory) = self.scanner.take_directory() {
                self.activity.report_directory(&directory, self.hostname);
            }
            if let Some(command) = self.scanner.take_command() {
                self.activity.shell_command(command);
            }
            self.activity
                .set_title(self.screen.title.as_deref(), self.hostname);
            self.activity.set_full_screen(self.screen.full);
            self
        }

        fn program(&self) -> Option<&str> {
            self.activity.program()
        }

        fn directory(&self) -> Option<&str> {
            self.activity.directory()
        }
    }

    #[test]
    fn a_report_outranks_the_ones_listed_after_it() {
        let mut remote = Remote::on(Some("web-01"));
        remote.feed(b"\x1b]3008;start=1;type=shell;cwd=/from/3008\x1b\\");
        assert_eq!(remote.directory(), Some("/from/3008"));
        // OSC 7 排在 3008 前面：收到后就只听它。
        remote.feed(
            b"\x1b]7;file://web-01/from/osc7\x07\x1b]3008;start=1;type=shell;cwd=/later\x1b\\",
        );
        assert_eq!(remote.directory(), Some("/from/osc7"));
        remote.feed(b"\x1b]9;9;/elsewhere\x07\x1b]1337;CurrentDir=/iterm\x07");
        assert_eq!(remote.directory(), Some("/from/osc7"));
        remote.feed(b"\x1b]7;kitty-shell-cwd://web-01/srv/next\x07");
        assert_eq!(remote.directory(), Some("/srv/next"));
    }

    #[test]
    fn each_source_reports_alone() {
        for (bytes, path) in [
            (&b"\x1b]7;file:///srv/a%20b\x07"[..], "/srv/a b"),
            (b"\x1b]7;kitty-shell-cwd://web-01/srv\x07", "/srv"),
            (b"\x1b]1337;CurrentDir=/srv/i\x07", "/srv/i"),
            (b"\x1b]633;P;Cwd=/srv/a\\x3bb\x07", "/srv/a;b"),
            (
                b"\x1b]3008;start=9;type=shell;hostname=web-01;cwd=/srv/c\x1b\\",
                "/srv/c",
            ),
            (b"\x1b]9;9;\"/srv/d\"\x07", "/srv/d"),
        ] {
            let mut remote = Remote::on(Some("web-01"));
            // 切成两半喂：序列跨了两次输出也认得出。
            let (head, tail) = bytes.split_at(bytes.len() / 2);
            remote.feed(head).feed(tail);
            assert_eq!(remote.directory(), Some(path), "{bytes:?}");
            assert_eq!(remote.activity.start_directory(), Some(path));
        }
    }

    #[test]
    fn a_report_naming_another_host_does_not_move_the_card() {
        let mut remote = Remote::on(Some("web-01"));
        remote.feed(b"\x1b]7;file://web-01/srv\x07");
        // 在卡片里又 ssh 到了 db-01。
        remote.feed(b"\x1b]7;file://db-01/var/lib\x07");
        remote.feed(b"\x1b]3008;start=2;type=shell;hostname=db-01;cwd=/tmp\x1b\\");
        assert_eq!(remote.directory(), Some("/srv"));
        // 还不知道 `uname -n` 时，带主机名的报告一律不信。
        let mut unknown = Remote::on(None);
        unknown.feed(b"\x1b]7;file://web-01/srv\x07");
        assert_eq!(unknown.directory(), None);
        // 不带主机名的照信。
        let mut unnamed = Remote::on(None);
        unnamed.feed(b"\x1b]1337;CurrentDir=/no/host\x07");
        assert_eq!(unnamed.directory(), Some("/no/host"));
    }

    #[test]
    fn without_reports_the_folder_comes_from_a_shell_title_for_show_only() {
        let mut remote = Remote::on(Some("web-01.example.com"));
        // Debian/Ubuntu 的 bash：`user@短主机名: 目录`。
        remote.feed(b"\x1b]0;qi@web-01: ~/src/app\x07");
        assert_eq!(remote.directory(), Some("~/src/app"));
        assert_eq!(remote.activity.start_directory(), Some("~/src/app"));
        // RHEL：冒号后没有空格。
        remote.feed(b"\x1b]0;qi@web-01:/etc/nginx\x07");
        assert_eq!(remote.directory(), Some("/etc/nginx"));
        assert_eq!(remote.activity.start_directory(), Some("/etc/nginx"));
        // `~` 和截短的、相对的目录只用来显示。
        for title in ["~", "…/deep/dir", "src"] {
            remote.feed(format!("\x1b]2;qi@web-01: {title}\x07").as_bytes());
            assert_eq!(remote.directory(), Some(title));
            assert_eq!(remote.activity.start_directory(), None, "{title}");
        }
        // 别的主机的标题不动目录。
        remote.feed(b"\x1b]0;root@db-01: /var/lib\x07");
        assert_eq!(remote.directory(), Some("src"));
        // 有了序列报告，标题里的目录就不用了。
        remote.feed(b"\x1b]7;file:///srv/real\x07\x1b]0;qi@web-01: ~/other\x07");
        assert_eq!(remote.directory(), Some("/srv/real"));
        assert_eq!(remote.activity.start_directory(), Some("/srv/real"));
    }

    #[test]
    fn a_title_naming_a_host_waits_for_its_name() {
        let mut remote = Remote::on(None);
        remote.feed(b"\x1b]0;qi@web-01: ~/src\x07");
        assert_eq!(remote.directory(), None);
        // 探测出主机名后，同一个标题重新核对。
        remote.hostname = Some("WEB-01");
        remote.feed(b"");
        assert_eq!(remote.directory(), Some("~/src"));
    }

    #[test]
    fn the_command_line_of_a_shell_integration_names_the_program() {
        let mut remote = Remote::on(Some("web-01"));
        // kitty 的 bash 集成：`%q` 转义，`;` 被切开又拼回来。
        remote.feed(b"\x1b]133;C;cmdline=sudo\\ -u\\ root\\ vim\\ /etc/a\\;b\x07");
        assert_eq!(remote.program(), Some("vim"));
        assert!(remote.activity.running());
        remote.feed(b"\x1b]133;D;0\x07\x1b]133;A\x07");
        assert_eq!(remote.program(), None);
        assert!(!remote.activity.running());
        // fish 4：URL 编码。
        remote.feed(b"\x1b]133;C;cmdline_url=env%20-u%20X%20FOO%3D1%20htop%20-d%2010\x07");
        assert_eq!(remote.program(), Some("htop"));
        // VS Code：先报命令行（`;` 转义成 `\x3b`），再报开始执行，名字留着。
        remote.feed(b"\x1b]633;E;LANG=C /usr/bin/git status\\x3b ls;nonce-1\x07");
        remote.feed(b"\x1b]633;C\x07");
        assert_eq!(remote.program(), Some("git"));
        remote.feed(b"\x1b]633;D;0\x07");
        assert_eq!(remote.program(), None);
        // OSC 3008：`cmdline=` 优先，其次 `comm=`；结束了回到提示符。
        remote.feed(b"\x1b]3008;start=7;type=command;comm=sudo;cmdline=sudo\\x20top\x1b\\");
        assert_eq!(remote.program(), Some("top"));
        remote.feed(b"\x1b]3008;end=7;exit=success\x1b\\");
        assert_eq!(remote.program(), None);
        remote.feed(b"\x1b]3008;start=8;type=command;comm=less\x1b\\");
        assert_eq!(remote.program(), Some("less"));
        remote.feed(b"\x1b]3008;start=1;type=shell;cwd=/srv\x1b\\");
        assert_eq!(remote.program(), None);
        assert_eq!(remote.directory(), Some("/srv"));
    }

    #[test]
    fn a_command_line_cut_at_sixteen_parameters_still_names_its_command() {
        let mut remote = Remote::on(None);
        let line = (0..30)
            .map(|n| format!("a{n}"))
            .collect::<Vec<_>>()
            .join("\\;");
        remote.feed(format!("\x1b]133;C;cmdline=python3\\ x.py\\;{line}\x07").as_bytes());
        assert_eq!(remote.program(), Some("python3"));
    }

    #[test]
    fn an_integration_that_names_no_command_still_says_one_runs() {
        let mut remote = Remote::on(None);
        // systemd 自带的脚本只说有命令在跑。
        remote.feed(b"\x1b]3008;start=3;type=command;cwd=/srv\x1b\\");
        assert_eq!(remote.program(), None);
        assert!(remote.activity.running());
        // 这时程序自己设的标题能补上名字。
        remote.feed(b"\x1b]2;main.rs (~/src) - VIM\x07");
        assert_eq!(remote.program(), Some("vim"));
        // 回到提示符：标题还是 vim 的也不信。
        remote.feed(b"\x1b]3008;end=3\x1b\\");
        assert_eq!(remote.program(), None);
        assert!(!remote.activity.running());
    }

    #[test]
    fn without_an_integration_the_title_a_program_set_names_it() {
        let mut remote = Remote::on(Some("web-01"));
        // vim / nvim 的标题总认。
        remote.feed(b"\x1b]2;a.txt + (~/x) - VIM\x07");
        assert_eq!(remote.program(), Some("vim"));
        remote.feed(b"\x1b]2;init.lua (~/.config/nvim) - Nvim\x07");
        assert_eq!(remote.program(), Some("nvim"));
        remote.feed(b"\x1b]2;Thanks for flying Vim\x07");
        assert_eq!(remote.program(), None);
        // fish：`[主机] 命令 目录`；在提示符下只有目录。
        remote.feed(b"\x1b]0;[web-01] htop ~/s/app\x07");
        assert_eq!(remote.program(), Some("htop"));
        remote.feed(b"\x1b]0;[web-01] ~/s/app\x07");
        assert_eq!(remote.program(), None);
    }

    #[test]
    fn oh_my_zsh_titles_name_the_command_after_a_shell_title() {
        // 没见过 shell 设的标题：一个固定的标题不当程序。
        let mut fixed = Remote::on(Some("web-01"));
        fixed.feed(b"\x1b]0;prod-server\x07");
        assert_eq!(fixed.program(), None);

        let mut remote = Remote::on(Some("web-01"));
        remote.feed(b"\x1b]2;qi@web-01:~\x07");
        assert_eq!(remote.program(), None);
        remote.feed(b"\x1b]2;sudo -E make test\x07");
        assert_eq!(remote.program(), Some("make"));
        remote.feed(b"\x1b]2;FOO=1 ./run.sh --fast\x07");
        assert_eq!(remote.program(), Some("run.sh"));
        remote.feed(b"\x1b]2;qi@web-01:~/src\x07");
        assert_eq!(remote.program(), None);
        assert_eq!(remote.directory(), Some("~/src"));
    }

    #[test]
    fn a_full_screen_program_with_no_name_is_only_running() {
        let mut remote = Remote::on(None);
        remote.feed(b"\x1b]0;qi@web-01: ~\x07\x1b[?1049h");
        assert_eq!(remote.program(), None);
        assert!(remote.activity.running());
        remote.feed(b"\x1b[?1049l");
        assert!(!remote.activity.running());
    }

    #[test]
    fn the_first_real_command_skips_prefixes() {
        for (line, name) in [
            ("vim a.txt", Some("vim")),
            ("/usr/bin/htop", Some("htop")),
            ("sudo -u postgres -i psql", Some("psql")),
            ("sudo -- nano /etc/hosts", Some("nano")),
            (
                "env -i PATH=/bin LANG=C.UTF-8 python3 -m http.server",
                Some("python3"),
            ),
            ("A=1 B=2 node app.js", Some("node")),
            ("nohup nice -n 5 time make", Some("make")),
            ("doas -u root vi x", Some("vi")),
            ("'my tool' --flag", Some("my tool")),
            ("sudo", None),
            ("FOO=1", None),
            ("", None),
        ] {
            assert_eq!(command_name(line).as_deref(), name, "{line}");
        }
    }

    #[test]
    fn shell_words_unquote_as_bash_quotes() {
        assert_eq!(shell_words(r"vim\ a\ b"), ["vim a b"]);
        assert_eq!(
            shell_words(r#"git commit -m "a \"b\"""#),
            ["git", "commit", "-m", r#"a "b""#]
        );
        assert_eq!(shell_words("echo 'it''s'"), ["echo", "its"]);
        assert_eq!(shell_words(r"$'ls\n-l\x41'"), ["ls\n-lA"]);
        assert_eq!(shell_words("  a \t b  "), ["a", "b"]);
    }

    #[test]
    fn a_name_reported_first_survives_a_bare_start() {
        let named = ShellCommand::Running(Some("git".into()));
        assert_eq!(ShellCommand::Running(None).after(Some(&named)), named);
        assert_eq!(ShellCommand::Prompt.after(Some(&named)), ShellCommand::Prompt);
        assert_eq!(
            ShellCommand::Running(None).after(Some(&ShellCommand::Prompt)),
            ShellCommand::Running(None)
        );
    }

    #[test]
    fn titles_read_as_shell_program_or_nothing() {
        assert_eq!(
            read_title("ubuntu@web-01: /var/www"),
            TitleSays::Shell {
                host: Some("web-01".into()),
                directory: Some("/var/www".into())
            }
        );
        assert_eq!(read_title(""), TitleSays::Unknown);
        assert_eq!(
            read_title("Administrator: Windows PowerShell"),
            TitleSays::Unknown
        );
        assert_eq!(read_title("emacs@web-01"), TitleSays::Unknown);
        assert_eq!(
            read_title(r"C:\Windows\System32\cmd.exe"),
            TitleSays::Program {
                name: "cmd.exe".into(),
                fish: false
            }
        );
    }
}
