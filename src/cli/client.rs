// 来自 since2006/shell-rs @ b2a307a: src/cli/client.rs
// 裁剪（Agent CLI 工单 01）：只打印主机列表和一台主机（工单 02 加回 exec 和 `exec --json`，工单 03 加回传输进度和摘要，工单 04 加回凭据和增删改）；
// 去掉单实例唤起，
// 后面的工单按需加回。错误前缀是 `micaterm:`。

//! The command's end of the CLI socket: send one request to the running
//! app and print what comes back.

use std::{
    io::{self, BufReader, Write},
    path::Path,
};

use gpui_kit::SharedString;
use unicode_width::UnicodeWidthStr as _;

use micaterm_hosts::HostOs;

use super::protocol::{
    AuthChoice, CardInfo, CardScreen, CliError, CredentialDeleted, CredentialDetails,
    CredentialKindChoice, Envelope, ErrorCode, FrameKind, HostDeleted, HostDetails, HostInfo,
    PROTOCOL_VERSION, ProxyChoice, Reply, Request, RouteDetails, TerminalContext, TransferCounters,
    TransferSummary, parse_json, read_frame, write_json,
};
use crate::i18n::t;

/// Exit code when the command could not do what was asked at all, as ssh
/// uses it.
pub const FAILURE_EXIT: i32 = 255;
/// Exit code when a transfer finished but some items failed.
pub const PARTIAL_EXIT: i32 = 1;
/// 自己的 stdout 或 stderr 被关掉了（`| head` 读够就走）：照 shell 里被 SIGPIPE 结束那样，
/// 128 + 13，不再说什么。
pub const OUTPUT_CLOSED_EXIT: i32 = 141;

/// Where output goes, and what it goes to.
pub struct Console<'a> {
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
    /// JSON instead of text, asked for or implied by a pipe.
    pub json: bool,
    /// `exec --json`: the output gathered into one ASCII JSON object, and
    /// errors in JSON as well.
    pub exec_json: bool,
    /// Progress lines only make sense on a terminal.
    pub stderr_is_terminal: bool,
}

impl Console<'_> {
    pub fn error(&mut self, code: ErrorCode, message: &str) -> i32 {
        if self.exec_json {
            let error = serde_json::json!({
                "error": { "code": code.as_str(), "message": message }
            });
            let _ = writeln!(self.stdout, "{}", ascii_json(&error));
        } else {
            let _ = writeln!(self.stderr, "micaterm: [{}] {message}", code.as_str());
        }
        FAILURE_EXIT
    }
}

/// What `exec --json` prints once the command ends. Output that is not
/// UTF-8 has U+FFFD in place of what is not.
#[derive(serde::Serialize)]
struct ExecResult<'a> {
    exit_code: i32,
    stdout: std::borrow::Cow<'a, str>,
    stderr: std::borrow::Cow<'a, str>,
    /// Where an `exec --card` command ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<TerminalContext>,
}

/// `value` as JSON in ASCII alone: every other character escaped, which
/// only ever happens inside strings. PowerShell and the Windows console
/// read it right whatever code page they assume.
pub(super) fn ascii_json(value: &impl serde::Serialize) -> String {
    let json = serde_json::to_string(value).unwrap_or_default();
    let mut ascii = String::with_capacity(json.len());
    for character in json.chars() {
        if character.is_ascii() {
            ascii.push(character);
        } else {
            for unit in character.encode_utf16(&mut [0; 2]) {
                ascii.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    ascii
}

/// Send `request` to the app listening at `endpoint` and print the answer;
/// the exit code.
pub fn run(endpoint: &Path, request: Request, console: &mut Console) -> i32 {
    let result = connect(endpoint).and_then(|stream| talk(&stream, request, console));
    match result {
        Ok(code) => code,
        Err(Failure::NotRunning) => {
            console.error(ErrorCode::NotRunning, &t!("cli.client.not_running"))
        }
        Err(Failure::Refused(message)) => console.error(ErrorCode::ConnectFailed, &message),
        Err(Failure::Broken(error)) => console.error(
            ErrorCode::ConnectFailed,
            &t!("cli.client.broken", error = error),
        ),
        // 连接没断，是读输出的那一头走了：MicaTerm 那边见连接关了会放弃命令。
        Err(Failure::OutputClosed) => OUTPUT_CLOSED_EXIT,
    }
}

/// 问应用一句，拿回它的回复，什么也不打印（`micaterm env` 用）。应用回的错误和连不上一样
/// 是 `Err`，带着错误码。
pub fn query(endpoint: &Path, request: Request) -> Result<Reply, CliError> {
    let answer = connect(endpoint).and_then(|stream| ask(&stream, request));
    match answer {
        Ok(answer) => answer,
        Err(Failure::NotRunning) => Err(CliError::new(
            ErrorCode::NotRunning,
            t!("cli.client.not_running"),
        )),
        Err(Failure::Refused(message)) => Err(CliError::new(ErrorCode::ConnectFailed, message)),
        Err(Failure::Broken(error)) => Err(CliError::new(
            ErrorCode::ConnectFailed,
            t!("cli.client.broken", error = error),
        )),
        // 这里不往自己的输出写，碰不上。
        Err(Failure::OutputClosed) => Err(CliError::new(ErrorCode::ConnectFailed, "")),
    }
}

/// 只把请求交出去，不等回话（`micaterm hook`：不能卡住 Agent）。
pub fn send(endpoint: &Path, request: Request) -> io::Result<()> {
    let stream = connect(endpoint).map_err(|_| io::Error::from(io::ErrorKind::NotConnected))?;
    let mut writer = &stream;
    write_json(
        &mut writer,
        &Envelope {
            version: PROTOCOL_VERSION,
            request,
        },
    )?;
    writer.flush()
}

/// 发请求，读到第一条 JSON 回复为止。
fn ask<S>(stream: S, request: Request) -> Result<Result<Reply, CliError>, Failure>
where
    S: Copy + io::Read + Write,
{
    let mut writer = stream;
    write_json(
        &mut writer,
        &Envelope {
            version: PROTOCOL_VERSION,
            request,
        },
    )?;
    let mut reader = BufReader::new(stream);
    loop {
        let Some((kind, payload)) = read_frame(&mut reader)? else {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
        };
        if matches!(kind, FrameKind::Json) {
            let reply: Reply = parse_json(&payload)?;
            return Ok(match reply {
                Reply::Error { code, message } => Err(CliError::new(code, message)),
                reply => Ok(reply),
            });
        }
    }
}

enum Failure {
    NotRunning,
    /// Not allowed to talk to the app, or not willing to.
    Refused(SharedString),
    Broken(io::Error),
    /// 往自己的 stdout、stderr 写不进去了。
    OutputClosed,
}

/// 往 stdout、stderr 写：写不进去不是和 MicaTerm 的连接出了问题。
fn output<T>(result: io::Result<T>) -> Result<T, Failure> {
    result.map_err(|_| Failure::OutputClosed)
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Failure::Broken(error)
    }
}

#[cfg(unix)]
fn connect(socket: &Path) -> Result<std::os::unix::net::UnixStream, Failure> {
    std::os::unix::net::UnixStream::connect(socket).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => Failure::NotRunning,
        io::ErrorKind::PermissionDenied => Failure::Refused(no_permission()),
        _ => error.into(),
    })
}

/// What a sandbox that forbids local connections looks like.
fn no_permission() -> SharedString {
    t!("cli.client.no_permission")
}

/// Open the app's pipe, and make sure it is the app's before saying
/// anything: the name is the machine's, so someone else could hold it.
#[cfg(windows)]
fn connect(pipe: &Path) -> Result<std::fs::File, Failure> {
    use super::pipe_windows;

    let pipe = pipe_windows::open(pipe).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Failure::NotRunning,
        io::ErrorKind::PermissionDenied => Failure::Refused(no_permission()),
        _ => error.into(),
    })?;
    if !pipe_windows::owned_by_current_user(&pipe)? {
        return Err(Failure::Refused(t!("cli.client.foreign_pipe")));
    }
    Ok(pipe)
}

#[cfg(not(any(unix, windows)))]
fn connect(_: &Path) -> Result<std::fs::File, Failure> {
    Err(Failure::Refused(t!("cli.unsupported")))
}

/// What the request did, for the words its answer is printed with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Asked {
    Show,
    Create,
    Update,
    Sync,
    Other,
}

impl Asked {
    fn of(request: &Request) -> Self {
        match request {
            Request::ShowHost { .. } | Request::ShowCredential { .. } => Asked::Show,
            Request::CreateHost { .. } | Request::CreateCredential { .. } => Asked::Create,
            Request::UpdateHost { .. } | Request::UpdateCredential { .. } => Asked::Update,
            Request::Sync { .. } => Asked::Sync,
            _ => Asked::Other,
        }
    }
}

/// Send the request and print the replies until the last one.
fn talk<S>(stream: S, request: Request, console: &mut Console) -> Result<i32, Failure>
where
    S: Copy + io::Read + Write,
{
    let asked = Asked::of(&request);
    let sync = asked == Asked::Sync;
    let mut writer = stream;
    write_json(
        &mut writer,
        &Envelope {
            version: PROTOCOL_VERSION,
            request,
        },
    )?;
    let mut reader = BufReader::new(stream);
    // The progress line on screen, to be blanked before anything else.
    let mut progress = ProgressLine::default();
    // What `exec --json` holds until the command ends.
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut context = None;
    loop {
        let Some((kind, payload)) = read_frame(&mut reader)? else {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
        };
        match kind {
            FrameKind::Stdout if console.exec_json => stdout.extend_from_slice(&payload),
            FrameKind::Stderr if console.exec_json => stderr.extend_from_slice(&payload),
            FrameKind::Stdout => {
                output(console.stdout.write_all(&payload))?;
                output(console.stdout.flush())?;
            }
            FrameKind::Stderr => {
                output(console.stderr.write_all(&payload))?;
                output(console.stderr.flush())?;
            }
            FrameKind::Json => {
                let reply: Reply = parse_json(&payload)?;
                if !matches!(reply, Reply::Progress(_)) {
                    progress.clear(console.stderr);
                }
                if let Reply::Progress(counters) = reply {
                    if console.stderr_is_terminal {
                        progress.show(console.stderr, &progress_line(counters));
                    }
                    continue;
                }
                // Only `--json` prints where the command ran.
                if let Reply::Context(found) = reply {
                    context = Some(found);
                    continue;
                }
                return Ok(match reply {
                    Reply::Progress(_) | Reply::Context(_) => unreachable!("handled above"),
                    Reply::Cards { cards } => {
                        output(print_cards(&cards, console))?;
                        0
                    }
                    Reply::Screen(screen) => {
                        output(print_screen(&screen, console))?;
                        0
                    }
                    Reply::KeySent { key, repeat } => {
                        output(print_key_sent(&key, repeat, console))?;
                        0
                    }
                    Reply::Accepted => 0,
                    Reply::TransferDone(summary) => output(print_summary(&summary, sync, console))?,
                    Reply::Hosts { hosts } => {
                        output(print_hosts(&hosts, console))?;
                        0
                    }
                    Reply::Host(host) => {
                        output(print_host(&host, asked, console))?;
                        0
                    }
                    Reply::HostDeleted(deleted) => {
                        output(print_host_deleted(&deleted, console))?;
                        0
                    }
                    Reply::Credentials { credentials } => {
                        output(print_credentials(&credentials, console))?;
                        0
                    }
                    Reply::Credential(credential) => {
                        output(print_credential(&credential, asked, console))?;
                        0
                    }
                    Reply::CredentialDeleted(deleted) => {
                        output(print_credential_deleted(&deleted, console))?;
                        0
                    }
                    Reply::Exit { code } if console.exec_json => {
                        let result = ExecResult {
                            exit_code: code,
                            stdout: String::from_utf8_lossy(&stdout),
                            stderr: String::from_utf8_lossy(&stderr),
                            context: context.take(),
                        };
                        output(writeln!(console.stdout, "{}", ascii_json(&result)))?;
                        code
                    }
                    Reply::Exit { code } => code,
                    // What an app from before the request was added says:
                    // serde names what it did not know.
                    Reply::Error {
                        code: ErrorCode::BadRequest,
                        message,
                    } if message.contains("unknown variant") => {
                        console.error(ErrorCode::VersionMismatch, &t!("cli.client.older_app"))
                    }
                    Reply::Error { code, message } => console.error(code, &message),
                });
            }
        }
    }
}

/// A line rewritten in place with a carriage return. Shorter text is
/// padded with spaces rather than cleared with an escape sequence, which
/// the older Windows console does not understand.
#[derive(Default)]
struct ProgressLine {
    /// Columns the line on screen takes up.
    width: usize,
}

impl ProgressLine {
    fn show(&mut self, out: &mut dyn Write, text: &str) {
        let width = text.width();
        let padding = self.width.saturating_sub(width);
        let _ = write!(out, "\r{text}{}", " ".repeat(padding));
        if padding > 0 {
            // Back to the end of the text.
            let _ = write!(out, "\r{text}");
        }
        let _ = out.flush();
        self.width = width;
    }

    fn clear(&mut self, out: &mut dyn Write) {
        if self.width > 0 {
            let _ = write!(out, "\r{}\r", " ".repeat(self.width));
            let _ = out.flush();
            self.width = 0;
        }
    }
}

/// 一次传输的结果：一行摘要，失败的一个一行写在 stderr；有失败时退出码 1。
/// `sync` 时跳过的是没变的。
fn print_summary(summary: &TransferSummary, sync: bool, console: &mut Console) -> io::Result<i32> {
    if console.json {
        print_json(summary, console)?;
    } else {
        let mut line = t!(
            "cli.summary.transferred",
            count = summary.files,
            size = format_bytes(summary.bytes)
        );
        if summary.skipped > 0 {
            line = if sync {
                t!(
                    "cli.summary.unchanged",
                    summary = line,
                    count = summary.skipped
                )
            } else {
                t!(
                    "cli.summary.skipped",
                    summary = line,
                    count = summary.skipped
                )
            };
        }
        if summary.deleted > 0 {
            line = t!(
                "cli.summary.deleted",
                summary = line,
                count = summary.deleted
            );
        }
        if summary.failed > 0 {
            line = t!("cli.summary.failed", summary = line, count = summary.failed);
        }
        writeln!(console.stdout, "{line}")?;
        for failure in &summary.failures {
            writeln!(console.stderr, "{failure}")?;
        }
    }
    Ok(if summary.failed > 0 { PARTIAL_EXIT } else { 0 })
}

fn progress_line(counters: TransferCounters) -> String {
    t!(
        "cli.progress",
        files = counters.files,
        total_files = counters.total_files,
        bytes = format_bytes(counters.bytes),
        total_bytes = format_bytes(counters.total_bytes)
    )
    .to_string()
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.;
    let mut unit = 0;
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn print_json(value: &impl serde::Serialize, console: &mut Console) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *console.stdout, value).map_err(io::Error::other)?;
    writeln!(console.stdout)
}

/// Rows lined up in columns, wide characters counted as two.
fn print_table<const N: usize>(
    header: [SharedString; N],
    rows: &[[String; N]],
    console: &mut Console,
) -> io::Result<()> {
    let header = header.map(String::from);
    let widths: Vec<usize> = (0..N)
        .map(|column| {
            std::iter::once(&header)
                .chain(rows)
                .map(|row| row[column].width())
                .max()
                .unwrap_or(0)
        })
        .collect();
    for row in std::iter::once(&header).chain(rows) {
        let line: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell}{}", " ".repeat(width - cell.width())))
            .collect();
        writeln!(console.stdout, "{}", line.join("  ").trim_end())?;
    }
    Ok(())
}

/// Labelled lines, the labels lined up.
fn print_fields(fields: &[(SharedString, String)], console: &mut Console) -> io::Result<()> {
    let width = fields
        .iter()
        .map(|(label, _)| label.width())
        .max()
        .unwrap_or(0);
    for (label, value) in fields {
        let padding = " ".repeat(width - label.width());
        writeln!(console.stdout, "{label}{padding}  {value}")?;
    }
    Ok(())
}

/// `label` saying whether its password is saved, when that is known.
fn with_saved(label: SharedString, saved: Option<bool>) -> String {
    let text = match saved {
        Some(true) => t!("cli.show.saved", label = label),
        Some(false) => t!("cli.show.not_saved", label = label),
        None => label,
    };
    text.to_string()
}

/// The system's name as the host list shows it.
fn os_label(os: Option<&str>) -> String {
    os.and_then(HostOs::from_stored)
        .map(|os| os.label().to_string())
        .unwrap_or_default()
}

fn print_hosts(hosts: &[HostInfo], console: &mut Console) -> io::Result<()> {
    if console.json {
        return print_json(&hosts, console);
    }
    if hosts.is_empty() {
        return writeln!(console.stderr, "{}", t!("cli.hosts.none"));
    }
    let rows: Vec<[String; 4]> = hosts
        .iter()
        .map(|host| {
            [
                host.id.clone(),
                host.name.clone(),
                host.address(),
                os_label(host.os.as_deref()),
            ]
        })
        .collect();
    print_table(
        [
            t!("cli.column.id"),
            t!("cli.column.name"),
            t!("cli.column.address"),
            t!("cli.column.os"),
        ],
        &rows,
        console,
    )
}

fn print_cards(cards: &[CardInfo], console: &mut Console) -> io::Result<()> {
    if console.json {
        return print_json(&cards, console);
    }
    if cards.is_empty() {
        return writeln!(console.stderr, "{}", t!("cli.cards.none"));
    }
    let rows: Vec<[String; 4]> = cards
        .iter()
        .map(|card| {
            let state = if card.running {
                t!("cli.cards.running")
            } else if card.connected {
                t!("cli.cards.connected")
            } else {
                t!("cli.cards.disconnected")
            };
            [
                card.id.clone(),
                card.host_name.clone(),
                card.host.clone(),
                state.to_string(),
            ]
        })
        .collect();
    print_table(
        [
            t!("cli.column.id"),
            t!("cli.column.name"),
            t!("cli.column.host"),
            t!("cli.column.state"),
        ],
        &rows,
        console,
    )
}

/// `cards read`：屏幕上的字原样印到 stdout，有没有读全写到 stderr。
fn print_screen(screen: &CardScreen, console: &mut Console) -> io::Result<()> {
    if console.json {
        return print_json(screen, console);
    }
    if !screen.text.is_empty() {
        writeln!(console.stdout, "{}", screen.text)?;
    }
    if screen.alternate_screen {
        writeln!(console.stderr, "{}", t!("cli.cards.alternate_screen"))?;
    }
    Ok(())
}

fn print_key_sent(key: &str, repeat: usize, console: &mut Console) -> io::Result<()> {
    if console.json {
        return print_json(
            &serde_json::json!({ "key": key, "repeat": repeat }),
            console,
        );
    }
    Ok(())
}

fn print_host(host: &HostDetails, asked: Asked, console: &mut Console) -> io::Result<()> {
    if console.json {
        return print_json(host, console);
    }
    match asked {
        Asked::Create => {
            return writeln!(
                console.stdout,
                "{}",
                t!("cli.host.created", name = host.name, id = host.id)
            );
        }
        Asked::Update => {
            return writeln!(
                console.stdout,
                "{}",
                t!("cli.host.saved", name = host.name, id = host.id)
            );
        }
        _ => {}
    }
    let auth = match host.auth {
        AuthChoice::Password => with_saved(t!("cli.auth.password"), host.password_saved),
        AuthChoice::Credential => t!(
            "cli.auth.credential",
            id = host.credential.as_deref().unwrap_or_default()
        )
        .to_string(),
        AuthChoice::NoPassword => t!("cli.auth.no_password").to_string(),
    };
    let mut fields = vec![
        (t!("cli.field.id"), host.id.clone()),
        (t!("cli.field.name"), host.name.clone()),
        (
            t!("cli.field.address"),
            format!("{}@{}:{}", host.user, host.host, host.port),
        ),
        (t!("cli.field.auth"), auth),
        (t!("cli.field.route"), route_text(&host.route)),
    ];
    let os = os_label(host.os.as_deref());
    if !os.is_empty() {
        fields.push((t!("cli.field.os"), os));
    }
    if !host.notes.is_empty() {
        fields.push((t!("cli.field.notes"), host.notes.clone()));
    }
    print_fields(&fields, console)
}

fn print_host_deleted(deleted: &HostDeleted, console: &mut Console) -> io::Result<()> {
    if console.json {
        return print_json(deleted, console);
    }
    let name = &deleted.host.name;
    let line = if deleted.jump_users > 0 {
        t!(
            "cli.host.deleted_jump_users",
            name = name,
            count = deleted.jump_users
        )
    } else {
        t!("cli.host.deleted", name = name)
    };
    writeln!(console.stdout, "{line}")
}

fn kind_label(kind: CredentialKindChoice) -> SharedString {
    match kind {
        CredentialKindChoice::Password => t!("cli.kind.password"),
        CredentialKindChoice::Key => t!("cli.kind.key"),
        CredentialKindChoice::Agent => t!("cli.kind.agent"),
    }
}

fn print_credentials(credentials: &[CredentialDetails], console: &mut Console) -> io::Result<()> {
    if console.json {
        return print_json(&credentials, console);
    }
    if credentials.is_empty() {
        return writeln!(console.stderr, "{}", t!("cli.credentials.none"));
    }
    let rows: Vec<[String; 5]> = credentials
        .iter()
        .map(|credential| {
            [
                credential.id.clone(),
                credential.name.clone(),
                kind_label(credential.kind).to_string(),
                credential.user.clone(),
                credential.hosts.len().to_string(),
            ]
        })
        .collect();
    print_table(
        [
            t!("cli.column.id"),
            t!("cli.column.name"),
            t!("cli.column.kind"),
            t!("cli.column.user"),
            t!("cli.column.hosts"),
        ],
        &rows,
        console,
    )
}

fn print_credential(
    credential: &CredentialDetails,
    asked: Asked,
    console: &mut Console,
) -> io::Result<()> {
    if console.json {
        return print_json(credential, console);
    }
    match asked {
        Asked::Create => {
            return writeln!(
                console.stdout,
                "{}",
                t!(
                    "cli.credential.created",
                    name = credential.name,
                    id = credential.id
                )
            );
        }
        Asked::Update => {
            return writeln!(
                console.stdout,
                "{}",
                t!(
                    "cli.credential.saved",
                    name = credential.name,
                    id = credential.id
                )
            );
        }
        _ => {}
    }
    let mut fields = vec![
        (t!("cli.field.id"), credential.id.clone()),
        (t!("cli.field.name"), credential.name.clone()),
        (
            t!("cli.field.kind"),
            with_saved(kind_label(credential.kind), credential.password_saved),
        ),
        (t!("cli.field.user"), credential.user.clone()),
    ];
    if credential.kind == CredentialKindChoice::Key {
        let key = with_saved(
            t!("cli.credential.private_key"),
            credential.private_key_saved,
        );
        fields.push((t!("cli.field.private_key"), key));
        if let Some(public_key) = &credential.public_key {
            fields.push((t!("cli.field.public_key"), public_key.clone()));
        }
        if let Some(saved) = credential.passphrase_saved {
            let saved = if saved {
                t!("cli.credential.passphrase_saved")
            } else {
                t!("cli.credential.passphrase_not_saved")
            };
            fields.push((t!("cli.field.passphrase"), saved.to_string()));
        }
    }
    fields.push((t!("cli.field.hosts"), credential.hosts.join(", ")));
    print_fields(&fields, console)
}

fn print_credential_deleted(deleted: &CredentialDeleted, console: &mut Console) -> io::Result<()> {
    if console.json {
        return print_json(deleted, console);
    }
    let mut line = t!("cli.credential.deleted", name = deleted.credential.name);
    if !deleted.released.is_empty() {
        let names: Vec<&str> = deleted
            .released
            .iter()
            .map(|host| host.name.as_str())
            .collect();
        line = t!(
            "cli.credential.deleted_released",
            deleted = line,
            hosts = names.join(t!("cli.list.separator").as_ref())
        );
    }
    writeln!(console.stdout, "{line}")
}

/// How a host is reached.
fn route_text(route: &RouteDetails) -> String {
    match route {
        RouteDetails::Direct => t!("cli.route.direct").to_string(),
        RouteDetails::Jump { hosts } => {
            let deleted = t!("cli.route.deleted_host");
            let hosts = hosts
                .iter()
                .map(|hop| hop.as_deref().unwrap_or(deleted.as_str()))
                .collect::<Vec<_>>()
                .join(" → ");
            t!("cli.route.jump", hosts = hosts).to_string()
        }
        RouteDetails::Proxy {
            kind,
            host,
            port,
            user,
            password_saved,
        } => {
            let user = user
                .as_deref()
                .map(|user| format!("{user}@"))
                .unwrap_or_default();
            let address = format!("{user}{host}:{port}");
            let proxy = match kind {
                ProxyChoice::Http => t!("cli.route.http", address = address),
                ProxyChoice::Socks5 => t!("cli.route.socks5", address = address),
            };
            let proxy = match password_saved {
                Some(true) => t!("cli.route.password_saved", proxy = proxy),
                Some(false) => t!("cli.route.password_not_saved", proxy = proxy),
                None => proxy,
            };
            proxy.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `print` writes to stdout, as on a terminal.
    fn printed(print: impl FnOnce(&mut Console) -> io::Result<()>) -> String {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        print(&mut Console {
            stdout: &mut stdout,
            stderr: &mut stderr,
            json: false,
            exec_json: false,
            stderr_is_terminal: false,
        })
        .unwrap();
        String::from_utf8(stdout).unwrap()
    }

    #[test]
    fn ascii_json_escapes_everything_else_including_astral_characters() {
        let value = serde_json::json!({ "stdout": "生产 ✓ 🚀\n" });
        let text = ascii_json(&value);
        assert!(text.is_ascii(), "{text}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            value
        );
    }

    #[test]
    fn a_summary_reads_as_one_line_and_failures_exit_with_one() {
        let summary = TransferSummary {
            files: 3,
            bytes: 2048,
            skipped: 5,
            failed: 1,
            deleted: 2,
            failures: vec!["/srv/app/a: 权限不足".into()],
        };
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = print_summary(
            &summary,
            true,
            &mut Console {
                stdout: &mut stdout,
                stderr: &mut stderr,
                json: false,
                exec_json: false,
                stderr_is_terminal: false,
            },
        )
        .unwrap();
        assert_eq!(code, PARTIAL_EXIT);
        let line = String::from_utf8(stdout).unwrap();
        assert_eq!(line.lines().count(), 1, "{line}");
        assert!(line.contains("2.0 KB"), "{line}");
        assert_eq!(String::from_utf8(stderr).unwrap(), "/srv/app/a: 权限不足\n");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn the_host_list_lines_up_wide_names() {
        let hosts = [
            HostInfo {
                id: "VmLkf1snMOuPKJ07".into(),
                name: "生产 web".into(),
                user: "root".into(),
                host: "10.0.0.2".into(),
                port: 22,
                os: Some("ubuntu".into()),
            },
            HostInfo {
                id: "a1b2c3d4e5f6g7h8".into(),
                name: "db".into(),
                user: "deploy".into(),
                host: "db.example.com".into(),
                port: 2222,
                os: None,
            },
        ];
        let text = printed(|console| print_hosts(&hosts, console));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        // 第二列在每行都从同一个显示宽度开始：中文按两格算。
        let column = |line: &str, cell: &str| line[..line.find(cell).unwrap()].width();
        assert_eq!(column(lines[1], "root@"), column(lines[2], "deploy@"));
        assert!(lines[1].ends_with("Ubuntu"));
    }

    #[test]
    fn a_host_says_whether_its_passwords_are_saved_and_never_what_they_are() {
        let host = HostDetails {
            id: "VmLkf1snMOuPKJ07".into(),
            name: "web".into(),
            host: "10.0.0.2".into(),
            port: 22,
            user: "root".into(),
            auth: AuthChoice::Password,
            credential: None,
            password_saved: Some(true),
            route: RouteDetails::Proxy {
                kind: ProxyChoice::Socks5,
                host: "proxy".into(),
                port: 1080,
                user: Some("me".into()),
                password_saved: Some(false),
            },
            notes: String::new(),
            os: None,
        };
        let text = printed(|console| print_host(&host, Asked::Show, console));
        assert!(text.contains("root@10.0.0.2:22"), "{text}");
        assert!(text.contains(&*t!("cli.show.saved", label = t!("cli.auth.password"))));
        assert!(text.contains("me@proxy:1080"), "{text}");
    }
}
