// 来自 since2006/shell-rs @ b2a307a: src/terminal/directory.rs

//! The folder a shell says it is in: OSC 7 (`file://host/path`, which fish
//! sends by itself, and kitty's `kitty-shell-cwd://host/path`), iTerm2's OSC
//! 1337 `CurrentDir=`, VS Code's OSC 633 `P;Cwd=`, systemd's OSC 3008 `cwd=`
//! and ConEmu's OSC 9;9（工单 17 加了后四种）. `NoticeScanner` finds them in
//! the output on the parser thread.

use std::collections::HashMap;

use percent_encoding::percent_decode_str;

/// A folder the terminal's output reported: where its shell is, if the
/// report came from this host and not from one ssh'd to inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportedDirectory {
    /// The host the report names; none when it names none (OSC 1337, an
    /// empty host, `localhost`).
    pub host: Option<String>,
    pub path: String,
}

impl ReportedDirectory {
    /// Restore this POSIX folder in a shell, with the path kept as one
    /// literal argument even when it contains quotes or shell syntax.
    pub fn cd_command(&self) -> Option<String> {
        (self.path.starts_with('/') && !self.path.chars().any(char::is_control))
            .then(|| format!("cd -- '{}'", self.path.replace('\'', r"'\''")))
    }

    /// 报告是不是来自 `uname -n` 叫 `hostname` 的主机：不带主机名的照它说的算；带主机名的
    /// 要对得上，不知道 `uname -n` 时是 [`HostMatch::Unknown`]。
    pub fn origin(&self, hostname: Option<&str>) -> HostMatch {
        match (&self.host, hostname) {
            (None, _) => HostMatch::Here,
            (Some(host), Some(name)) if host.eq_ignore_ascii_case(name) => HostMatch::Here,
            (Some(_), Some(_)) => HostMatch::Elsewhere,
            (Some(_), None) => HostMatch::Unknown,
        }
    }
}

/// 远程报告（目录序列、窗口标题）里的主机名和卡片所连主机的 `uname -n` 对不对得上。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostMatch {
    /// 对得上，或者报告没说主机名。
    Here,
    /// 是别的主机（比如在卡片里又 ssh 到了别处）。
    Elsewhere,
    /// 报告说了主机名，但不知道 `uname -n`（关了探测、从没探测过）：只信来显示。
    Unknown,
}

/// 哪一种序列报告的目录，越靠前越可信（工单 17 的目录优先级）：同一条连接上收到过靠前的，
/// 就不再听靠后的（`NoticeScanner` 里记着）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum DirectorySource {
    /// OSC 7：`file://`、kitty 的 `kitty-shell-cwd://`。
    Osc7,
    /// iTerm2 的 OSC 1337 `CurrentDir=`。
    Iterm,
    /// VS Code 的 OSC 633 `P;Cwd=`。
    VsCode,
    /// systemd 的 OSC 3008 `cwd=`。
    Context,
    /// ConEmu / Windows Terminal 的 OSC 9;9。
    ConEmu,
}

/// The folder an OSC sequence reports, by its parameters as the parser
/// splits them on `;`, if it is a well-formed report, with the kind of
/// sequence that reported it.
pub(super) fn reported_directory(params: &[&[u8]]) -> Option<(DirectorySource, ReportedDirectory)> {
    // A path may hold `;`, which the parser split on.
    let rest = |from: usize| joined(params, from);
    let (source, directory) = match *params.first()? {
        b"7" => (DirectorySource::Osc7, osc_7(&rest(1)?)?),
        b"1337" => (
            DirectorySource::Iterm,
            ReportedDirectory {
                host: None,
                path: rest(1)?.strip_prefix("CurrentDir=")?.to_string(),
            },
        ),
        // `633;P;Cwd=<值>`：值里的反斜杠和 `;` 转义过（`unescape`）。
        b"633" if params.get(1) == Some(&&b"P"[..]) => (
            DirectorySource::VsCode,
            ReportedDirectory {
                host: None,
                path: unescape(rest(2)?.strip_prefix("Cwd=")?)?,
            },
        ),
        // `3008;start=<id>;type=shell;hostname=<主机>;cwd=<目录>`：结束（`end=`）的不带目录。
        b"3008" => {
            let fields = context_fields(params);
            fields.get("start")?;
            (
                DirectorySource::Context,
                ReportedDirectory {
                    host: fields
                        .get("hostname")
                        .and_then(|host| unescape(host))
                        .filter(|host| !host.is_empty()),
                    path: unescape(fields.get("cwd")?)?,
                },
            )
        }
        // `9;9;"C:\x"`：路径可以带一对双引号。
        b"9" if params.get(1) == Some(&&b"9"[..]) => {
            let path = rest(2)?;
            let path = path
                .strip_prefix('"')
                .and_then(|path| path.strip_suffix('"'))
                .unwrap_or(&path);
            (
                DirectorySource::ConEmu,
                ReportedDirectory {
                    host: None,
                    path: path.to_string(),
                },
            )
        }
        _ => return None,
    };
    let absolute = directory.path.starts_with('/')
        // Windows 上的 shell 报 `C:\x`（OSC 7 写成 `/C:/x`，上面已经是 `/` 开头）。
        || (source != DirectorySource::Osc7 && is_windows_absolute(&directory.path));
    (absolute && !directory.path.chars().any(char::is_control)).then_some((source, directory))
}

/// 从第 `from` 个参数起，用 `;` 拼回去（解析器按 `;` 切开了）。不是 UTF-8 时是 `None`。
pub(super) fn joined(params: &[&[u8]], from: usize) -> Option<String> {
    params
        .get(from..)
        .unwrap_or_default()
        .iter()
        .map(|param| std::str::from_utf8(param).ok())
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join(";"))
}

/// `7;file://主机/编码过的路径`，或 kitty 的 `7;kitty-shell-cwd://主机/路径`（路径不编码）。
fn osc_7(url: &str) -> Option<ReportedDirectory> {
    let (url, encoded) = match strip_prefix_ignore_case(url, "file://") {
        Some(url) => (url, true),
        None => (strip_prefix_ignore_case(url, "kitty-shell-cwd://")?, false),
    };
    let slash = url.find('/')?;
    let (host, path) = url.split_at(slash);
    let path = if encoded {
        percent_decode_str(path).decode_utf8().ok()?.into_owned()
    } else {
        path.to_string()
    };
    let host =
        (!host.is_empty() && !host.eq_ignore_ascii_case("localhost")).then(|| host.to_string());
    Some(ReportedDirectory { host, path })
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// `C:\x`、`C:/x`。
fn is_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

/// OSC 3008 的 `键=值` 字段（第一个参数是 `3008`）。同名的取第一个。
pub(super) fn context_fields<'a>(params: &[&'a [u8]]) -> HashMap<&'a str, &'a str> {
    let mut fields = HashMap::new();
    for param in params.get(1..).unwrap_or_default() {
        let Ok(param) = std::str::from_utf8(param) else {
            continue;
        };
        if let Some((key, value)) = param.split_once('=') {
            fields.entry(key).or_insert(value);
        }
    }
    fields
}

/// 反转义 VS Code（OSC 633）和 OSC 3008 的值：两个反斜杠是一个，`\xNN` 是一个字节；
/// 结果要是 UTF-8。
pub(super) fn unescape(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'\\' {
            if bytes.get(at + 1) == Some(&b'\\') {
                out.push(b'\\');
                at += 2;
                continue;
            }
            if bytes.get(at + 1) == Some(&b'x')
                && let Some(hex) = value.get(at + 2..at + 4)
                && let Ok(byte) = u8::from_str_radix(hex, 16)
            {
                out.push(byte);
                at += 4;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(sequence: &str) -> Option<ReportedDirectory> {
        source_of(sequence).map(|(_, directory)| directory)
    }

    fn source_of(sequence: &str) -> Option<(DirectorySource, ReportedDirectory)> {
        let params: Vec<&[u8]> = sequence.split(';').map(str::as_bytes).collect();
        reported_directory(&params)
    }

    #[test]
    fn kitty_names_the_host_and_a_raw_path() {
        assert_eq!(
            parse("7;kitty-shell-cwd://web-01/srv/a%20b"),
            at(Some("web-01"), "/srv/a%20b")
        );
        assert_eq!(
            source_of("7;kitty-shell-cwd://web-01/srv").unwrap().0,
            DirectorySource::Osc7
        );
    }

    #[test]
    fn vs_code_reports_an_escaped_path() {
        assert_eq!(parse(r"633;P;Cwd=/srv/a\x3bb\\c"), at(None, r"/srv/a;b\c"));
        // Windows 上的 pwsh。
        assert_eq!(parse(r"633;P;Cwd=C:\\Users\\qi"), at(None, r"C:\Users\qi"));
        assert_eq!(parse("633;P;IsWindows=True"), None);
        assert_eq!(parse("633;A"), None);
    }

    #[test]
    fn osc_3008_reports_the_folder_and_the_host_of_a_context() {
        assert_eq!(
            parse(r"3008;start=8f1a;type=shell;user=qi;hostname=web-01;cwd=/srv/a\x3bb"),
            at(Some("web-01"), "/srv/a;b")
        );
        assert_eq!(
            parse("3008;start=1;type=command;cwd=/tmp"),
            at(None, "/tmp")
        );
        // 结束的上下文不带目录；没有 `start=` 的不算。
        assert_eq!(parse("3008;end=8f1a;exit=success"), None);
        assert_eq!(parse("3008;type=shell;cwd=/tmp"), None);
        assert_eq!(parse("3008;start=1;cwd=relative"), None);
    }

    #[test]
    fn conemu_reports_a_windows_or_posix_path_quoted_or_not() {
        assert_eq!(parse(r#"9;9;"C:\Users\qi""#), at(None, r"C:\Users\qi"));
        assert_eq!(parse(r"9;9;D:\work;x"), at(None, r"D:\work;x"));
        assert_eq!(parse("9;9;/home/qi"), at(None, "/home/qi"));
        assert_eq!(parse("9;9;relative"), None);
        assert_eq!(parse("9;4;1;50"), None);
    }

    #[test]
    fn the_sources_rank_as_the_spec_lists_them() {
        let rank = |sequence| source_of(sequence).unwrap().0;
        let ranks = [
            rank("7;file:///a"),
            rank("1337;CurrentDir=/a"),
            rank("633;P;Cwd=/a"),
            rank("3008;start=1;cwd=/a"),
            rank("9;9;/a"),
        ];
        assert!(ranks.windows(2).all(|pair| pair[0] < pair[1]), "{ranks:?}");
    }

    fn at(host: Option<&str>, path: &str) -> Option<ReportedDirectory> {
        Some(ReportedDirectory {
            host: host.map(str::to_string),
            path: path.to_string(),
        })
    }

    #[test]
    fn osc_7_names_the_host_and_an_encoded_path() {
        assert_eq!(
            parse("7;file://web-01/srv/a%20b/%E6%96%87%E4%BB%B6"),
            at(Some("web-01"), "/srv/a b/文件")
        );
        // A `;` in the path, split off by the parser, is put back.
        assert_eq!(
            parse("7;file://web-01/srv/a;b"),
            at(Some("web-01"), "/srv/a;b")
        );
        // Raw UTF-8 as some shells send it.
        assert_eq!(
            parse("7;FILE://web-01/srv/文件"),
            at(Some("web-01"), "/srv/文件")
        );
        // No host, or this one by name, is no host to check.
        assert_eq!(parse("7;file:///tmp"), at(None, "/tmp"));
        assert_eq!(parse("7;file://localhost/tmp"), at(None, "/tmp"));
    }

    #[test]
    fn osc_1337_reports_a_path_alone() {
        assert_eq!(parse("1337;CurrentDir=/srv/app"), at(None, "/srv/app"));
        assert_eq!(parse("1337;SetMark"), None);
    }

    #[test]
    fn what_is_not_a_folder_is_not_reported() {
        for sequence in [
            "7;https://web-01/srv",
            "7;file://web-01",
            "7;file:/srv",
            "7;file://web-01/a%0Ab",
            "7;file://web-01/%FF",
            "1337;CurrentDir=relative",
            "9;file://web-01/srv",
            "0;title",
        ] {
            assert_eq!(parse(sequence), None, "{sequence}");
        }
    }

    #[test]
    fn a_report_naming_a_host_counts_only_from_that_host() {
        let named = at(Some("WEB-01"), "/srv").unwrap();
        assert_eq!(named.origin(Some("web-01")), HostMatch::Here);
        assert_eq!(named.origin(Some("db-01")), HostMatch::Elsewhere);
        assert_eq!(named.origin(None), HostMatch::Unknown);
        assert_eq!(at(None, "/srv").unwrap().origin(None), HostMatch::Here);
    }

    #[test]
    fn restoring_a_folder_quotes_it_as_one_literal_argument() {
        #[cfg(unix)]
        for path in [
            "/srv/itcm",
            "/srv/a b/中文",
            "/srv/it's;$(printf oops)`printf oops`",
            "/-logs",
        ] {
            let directory = at(None, path).unwrap();
            let command = directory.cd_command().unwrap();
            let output = std::process::Command::new("sh")
                .args(["-c", &format!("set -- {command}; printf '%s\\0' \"$@\"")])
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, format!("cd\0--\0{path}\0").as_bytes());
        }
        assert_eq!(
            at(None, "/srv/itcm").unwrap().cd_command().as_deref(),
            Some("cd -- '/srv/itcm'")
        );
        for path in ["relative", "/tmp\ninjected", "/tmp\r", "/tmp\x1b"] {
            assert!(at(None, path).unwrap().cd_command().is_none());
        }
    }
}
