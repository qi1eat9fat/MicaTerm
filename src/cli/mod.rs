// 来自 since2006/shell-rs @ b2a307a: src/cli/mod.rs
// 裁剪（Agent CLI 工单 01）：命令只有 `hosts list` / `hosts show`（工单 02 加回 `exec`，不带 `--terminal`；
// 工单 03 加回 `upload` / `download` / `sync`，工单 04 加回主机和凭据的增删改查，工单 05 加回安装命令和 Skill）；链接留给后面的工单。`--help` 照上游写英文（给 Agent 读），程序名是 `micaterm`。

//! 外部 CLI：给 AI Agent 用的 `micaterm` 命令。命令本身不读数据库、不碰保险库：它把每个请求
//! 经本机的套接字（Windows 上是命名管道）交给正在运行的 MicaTerm，由 MicaTerm 用保存的配置去做。
//!
//! - `protocol`：线上的帧、请求和回复；
//! - `server`：MicaTerm 这一端，一个请求一条线程，只读窗口壳交进来的主机快照（`hosts`），
//!   要连主机的活交给后端（`backend`：`exec`、传输）；
//! - `client`：命令这一端，发请求、打印回复；
//! - `pipe_windows`：只许当前用户连的命名管道；
//! - `install`、`install_windows`、`integration`：把命令装进 PATH、把 Skill 装到各个 Agent 找的地方
//!   （`SKILL.md` 是 Skill 的正文）。

mod agent_env;
mod backend;
mod client;
mod env;
mod hosts;
mod install;
#[cfg(any(windows, test))]
mod install_windows;
mod integration;
mod manage;
#[cfg(windows)]
mod pipe_windows;
mod protocol;
mod server;
#[cfg(test)]
mod tests;

use std::{
    ffi::OsString,
    io::{self, IsTerminal as _, Read as _, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::{Parser, Subcommand};
use gpui_kit::SharedString;

pub use agent_env::{
    CARD_ENV, CLI_ENV, HOST_ENV, SELF_CARD_ENV, TERM_PROGRAM, TERM_PROGRAM_ENV,
    TERM_PROGRAM_VERSION_ENV, cli_callable, identity as identity_env,
};
pub use backend::SshCliBackend;
pub use client::{Console, FAILURE_EXIT, PARTIAL_EXIT, run as send_request};
pub use hosts::{CliTarget, credentials, targets};
pub(crate) use hosts::{host_details, host_info};
pub use install::{AgentKind, BinaryStatus, IntegrationPaths, SKILL, SkillStatus, UserPath};
pub use integration::{CliIntegration, IntegrationChange, IntegrationStatus};
pub use manage::CliChange;
pub(crate) use manage::{
    SecretChange, credential_details, find_credential, find_host, plan_credential, plan_host,
};
pub use protocol::{
    CardInfo, CardScreen, CliError, CredentialDeleted, CredentialDetails, CredentialFields,
    ErrorCode, HostDeleted, HostDetails, HostFields, HostInfo, Reply, Request, TerminalContext,
    TransferCounters, TransferSummary,
};
pub use server::{ChangeReply, CliBackend, CliServer};

use crate::i18n::t;

const AFTER_HELP: &str = concat!(
    "Hosts are named by the 16-character ID that `micaterm hosts list` prints, ",
    "the one MicaTerm copies with Copy ID.\n",
    "MicaTerm must be running, with Settings → Agent → Enable external Agent turned on.\n",
    "\n",
    "Exit codes: exec exits with the remote command's code; 1 means a transfer finished with failures; ",
    "255 means micaterm could not do what was asked ",
    "(the reason is printed as `micaterm: [code] message`).",
);

const HOST_FIELDS: &str = "\
Reads one JSON object from stdin. Every field may be left out; `update` changes only the fields given.
  name        Default the address; an empty name is the address too.
  host        The address (IP or host name). Required to create. Also read as `address`.
  port        Default 22.
  user        Default root. A host using a credential logs in as the credential's user.
  auth        \"password\", \"credential\" or \"no_password\".
  credential  A credential ID from `micaterm credentials list`, for auth \"credential\" (implied).
  password    The host's own password, saved to MicaTerm's vault and never printed; null deletes it.
  route       {\"type\": \"direct\"}
              {\"type\": \"jump\", \"hosts\": [ID, ...]}   (saved hosts, in order)
              {\"type\": \"proxy\", \"kind\": \"http\" | \"socks5\", \"host\": ..., \"port\": ...,
               \"user\": ..., \"password\": ...}
  notes       Free text.
What `micaterm hosts show <ID> --json` prints can be edited and passed back as it is.";

const CREDENTIAL_FIELDS: &str = "\
Reads one JSON object from stdin. Every field may be left out; `update` changes only the fields given.
  name         Required to create.
  kind         \"password\" (the default), \"key\" (implied by a key) or \"agent\".
  user         Default root.
  password     A password credential's, saved to MicaTerm's vault and never printed; null deletes it.
  key_path     A private key file on this machine (relative to the current directory, or ~/...),
               read once into the vault.
  private_key  The private key's text instead.
  passphrase   The key's passphrase, saved to the vault; null deletes it.
What `micaterm credentials show <ID> --json` prints can be edited and passed back as it is.";

/// Run commands on and manage the SSH hosts saved in MicaTerm.
///
/// MicaTerm holds the passwords and keys and makes the connections; this
/// command never reads them. One given to it to save goes to MicaTerm's
/// vault and is never printed back.
#[derive(Debug, Parser)]
#[command(name = "micaterm", version, after_help = AFTER_HELP)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Say where you are and what you can do: this card's ID, the card the
    /// user handed you, whether MicaTerm and its external CLI are on, and
    /// every command with an example. Run it first when unsure.
    Env {
        #[arg(long)]
        json: bool,
    },
    /// Show, create, change or delete saved hosts.
    Hosts {
        #[command(subcommand)]
        command: HostsCommand,
    },
    /// Show, create, change or delete saved credentials.
    Credentials {
        #[command(subcommand)]
        command: CredentialsCommand,
    },
    /// See and type into the terminal cards the user handed to the agent.
    ///
    /// A card is handed over from its right-click menu in MicaTerm (交给 Agent / Hand to
    /// agent) and taken back the same way; only handed cards are listed,
    /// read or typed into, and a card's ID ends when it is taken back.
    Cards {
        #[command(subcommand)]
        command: CardsCommand,
    },
    /// Run one command on a saved host and print its output.
    ///
    /// Opens a connection of its own, runs the command in the login shell,
    /// and closes it. The command gets no stdin. The exit code is the
    /// remote command's.
    Exec {
        /// Host ID, from `micaterm hosts list`. Not with --card.
        #[arg(required_unless_present_any = ["json", "card"])]
        id: Option<String>,
        /// One complete remote shell command, quoted as one argument.
        #[arg(required_unless_present_any = ["stdin", "json", "card"])]
        command: Option<String>,
        /// Type the command into a card the user handed to the agent
        /// (`micaterm cards list`) instead of logging in again, for a bastion
        /// host that allows one login. It sends Ctrl-C first, which
        /// interrupts whatever runs there. Its stderr comes as stdout; with
        /// --json, the result also has "context": the user, host, folder and
        /// shell it ran in. Bash, dash and busybox ash only.
        #[arg(long, value_name = "CARD", conflicts_with = "json")]
        card: Option<String>,
        /// Read the command from stdin instead, for commands with quotes,
        /// pipes, `$` or several lines.
        #[arg(long, conflicts_with = "command")]
        stdin: bool,
        /// Read {"host": ID, "command": COMMAND} from stdin and print
        /// {"exit_code", "stdout", "stderr"} once the command ends, errors as
        /// {"error": {"code", "message"}}. The output is ASCII, everything
        /// else escaped: no shell quoting, and no console code page can
        /// garble it. {"card": CARD, "command": COMMAND} is the same as
        /// --card.
        #[arg(long, conflicts_with_all = ["id", "command", "stdin"])]
        json: bool,
    },
    /// Copy a local file or folder to a saved host.
    ///
    /// Like scp: when the destination is an existing directory the source
    /// goes inside it under its own name; otherwise the destination is the
    /// copy's path. Folders are copied recursively; existing files are
    /// overwritten.
    Upload {
        /// Host ID, from `micaterm hosts list`.
        id: String,
        /// Local file or folder.
        local: PathBuf,
        /// Remote destination. `~` is the login directory.
        remote: String,
        /// Print the summary as JSON. Implied when stdout is not a terminal.
        #[arg(long)]
        json: bool,
    },
    /// Copy a file or folder from a saved host to this machine.
    ///
    /// Destination rules are scp's, as for `upload`.
    Download {
        /// Host ID, from `micaterm hosts list`.
        id: String,
        /// Remote file or folder. `~` is the login directory.
        remote: String,
        /// Local destination.
        local: PathBuf,
        /// Print the summary as JSON. Implied when stdout is not a terminal.
        #[arg(long)]
        json: bool,
    },
    /// Copy what a local folder holds into a folder on a saved host,
    /// skipping files that have not changed.
    ///
    /// The remote folder is made when it is not there (its parent must
    /// be). A file of the same size and modification time is left alone;
    /// any other is copied over. Only this way: nothing comes back.
    Sync {
        /// Host ID, from `micaterm hosts list`.
        id: String,
        /// Local folder, whose contents are copied.
        local: PathBuf,
        /// Remote folder. `~` is the login directory.
        remote: String,
        /// First delete what is in the remote folder and not in the local
        /// one.
        #[arg(long)]
        delete: bool,
        /// Print the summary as JSON. Implied when stdout is not a terminal.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum HostsCommand {
    /// List the saved hosts.
    List {
        /// Only hosts whose name, address or user matches every word, or whose ID is this.
        #[arg(short, long)]
        query: Option<String>,
        /// Print JSON. Implied when stdout is not a terminal.
        #[arg(long)]
        json: bool,
    },
    /// Show one host in full. Passwords are never shown, only whether one
    /// is saved.
    Show {
        /// Host ID, from `micaterm hosts list`.
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Save a new host, described by JSON on stdin.
    #[command(after_help = HOST_FIELDS)]
    Create {
        #[arg(long)]
        json: bool,
    },
    /// Change a saved host: the fields of the JSON on stdin.
    #[command(after_help = HOST_FIELDS)]
    Update {
        /// Host ID, from `micaterm hosts list`.
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Delete a saved host. Its open cards in MicaTerm stay.
    Delete {
        /// Host ID, from `micaterm hosts list`.
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CardsCommand {
    /// List the cards handed to the agent.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Print the last lines of a card's screen and scrollback, wrapped
    /// lines joined, as the user sees them now.
    Read {
        /// Card ID, from `micaterm cards list`.
        id: String,
        /// How many lines, from the bottom up: 120 when left out, at most
        /// 2000.
        #[arg(long)]
        lines: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Press a key in a card: to quit a pager (`q`), answer a prompt (`y`,
    /// `enter`) or interrupt (`ctrl-c`). While `exec --card` runs there,
    /// only `ctrl-c`.
    SendKey {
        /// Card ID, from `micaterm cards list`.
        id: String,
        /// One printable character, or ctrl-a…ctrl-z, esc, enter, tab,
        /// backspace, space, up, down, left, right, home, end, page-up,
        /// page-down, f1…f12.
        key: String,
        /// Press it this many times, at most 100.
        #[arg(long)]
        repeat: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CredentialsCommand {
    /// List the credentials.
    List {
        /// Only credentials whose name, user, kind or ID matches.
        #[arg(short, long)]
        query: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show one credential. Passwords and keys are never shown, only
    /// whether they are saved.
    Show {
        /// Credential ID, from `micaterm credentials list`.
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Save a new credential, described by JSON on stdin.
    #[command(after_help = CREDENTIAL_FIELDS)]
    Create {
        #[arg(long)]
        json: bool,
    },
    /// Change a credential: the fields of the JSON on stdin.
    #[command(after_help = CREDENTIAL_FIELDS)]
    Update {
        /// Credential ID, from `micaterm credentials list`.
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Delete a credential. The hosts using it log in on their own from
    /// then on.
    Delete {
        /// Credential ID, from `micaterm credentials list`.
        id: String,
        #[arg(long)]
        json: bool,
    },
}

/// 命令行入口：`args` 不含程序名。返回进程的退出码。
pub fn main(args: Vec<OsString>) -> i32 {
    let cli = match Cli::try_parse_from(std::iter::once(OsString::from("micaterm")).chain(args)) {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return error.exit_code();
        }
    };
    let (stdout, stderr) = (io::stdout(), io::stderr());
    let json = !stdout.is_terminal();
    let stderr_is_terminal = stderr.is_terminal();
    let mut stdout = ConsoleText::new(stdout, !json);
    let mut stderr = ConsoleText::new(stderr, stderr_is_terminal);
    let mut console = Console {
        stdout: &mut stdout,
        stderr: &mut stderr,
        json,
        exec_json: false,
        stderr_is_terminal,
    };
    // `env` 不发请求、不看退出码：MicaTerm 没开也照样说清楚。
    if let Command::Env { json } = cli.command {
        console.json |= json;
        let endpoint = crate::app::data_dir().map(|dir| crate::app::cli_endpoint(&dir));
        let code = env::run(endpoint.as_deref(), &mut console);
        let _ = stdout.finish();
        let _ = stderr.finish();
        return code;
    }
    let code = match (request(cli.command, &mut console), crate::app::data_dir()) {
        (Err(code), _) => code,
        (Ok(request), Ok(dir)) => {
            client::run(&crate::app::cli_endpoint(&dir), request, &mut console)
        }
        (Ok(_), Err(error)) => console.error(
            ErrorCode::ConnectFailed,
            &t!("cli.client.no_data_dir", error = error),
        ),
    };
    let _ = stdout.finish();
    let _ = stderr.finish();
    code
}

/// 命令行要发的请求，或者发不出去时的退出码（原因已经打印了）。
fn request(command: Command, console: &mut Console) -> Result<Request, i32> {
    Ok(match command {
        Command::Env { .. } => unreachable!("`env` is answered in `main`"),
        Command::Hosts { command } => match command {
            HostsCommand::List { query, json } => {
                console.json |= json;
                Request::List { query }
            }
            HostsCommand::Show { id, json } => {
                console.json |= json;
                Request::ShowHost { host: id }
            }
            HostsCommand::Create { json } => {
                console.json |= json;
                Request::CreateHost {
                    fields: host_fields(console, "micaterm hosts create < host.json")?,
                }
            }
            HostsCommand::Update { id, json } => {
                console.json |= json;
                let example = format!("micaterm hosts update {id} < changes.json");
                Request::UpdateHost {
                    fields: host_fields(console, &example)?,
                    host: id,
                }
            }
            HostsCommand::Delete { id, json } => {
                console.json |= json;
                Request::DeleteHost { host: id }
            }
        },
        Command::Credentials { command } => match command {
            CredentialsCommand::List { query, json } => {
                console.json |= json;
                Request::ListCredentials { query }
            }
            CredentialsCommand::Show { id, json } => {
                console.json |= json;
                Request::ShowCredential { credential: id }
            }
            CredentialsCommand::Create { json } => {
                console.json |= json;
                Request::CreateCredential {
                    fields: credential_fields(
                        console,
                        "micaterm credentials create < credential.json",
                    )?,
                }
            }
            CredentialsCommand::Update { id, json } => {
                console.json |= json;
                let example = format!("micaterm credentials update {id} < changes.json");
                Request::UpdateCredential {
                    fields: credential_fields(console, &example)?,
                    credential: id,
                }
            }
            CredentialsCommand::Delete { id, json } => {
                console.json |= json;
                Request::DeleteCredential { credential: id }
            }
        },
        Command::Cards { command } => match command {
            CardsCommand::List { json } => {
                console.json |= json;
                Request::ListCards
            }
            CardsCommand::Read { id, lines, json } => {
                console.json |= json;
                Request::ReadCard { card: id, lines }
            }
            CardsCommand::SendKey {
                id,
                key,
                repeat,
                json,
            } => {
                console.json |= json;
                Request::SendKey {
                    card: id,
                    key,
                    repeat,
                }
            }
        },
        Command::Exec {
            id,
            command,
            card,
            stdin,
            json,
        } => {
            // 从这里起，错误也是 JSON。
            console.exec_json = json;
            let request = if json {
                exec_request(&read_stdin(console)?)
                    .map_err(|message| console.error(ErrorCode::BadRequest, &message))?
            } else if let Some(card) = card {
                // `exec --card CARD COMMAND`：没有主机 ID，clap 把命令排在第一个位置上。
                if id.is_some() && (command.is_some() || stdin) {
                    return Err(console.error(ErrorCode::BadRequest, &t!("cli.exec.card_and_host")));
                }
                let command = if stdin {
                    read_stdin(console)?
                } else {
                    command.or(id).unwrap_or_default()
                };
                ExecJson {
                    host: None,
                    card: Some(card),
                    command,
                }
            } else {
                ExecJson {
                    host: id,
                    card: None,
                    command: if stdin {
                        read_stdin(console)?
                    } else {
                        command.unwrap_or_default()
                    },
                }
            };
            let command = normalize_command(&request.command);
            if command.trim().is_empty() {
                return Err(console.error(ErrorCode::BadRequest, &t!("cli.exec.empty")));
            }
            match request {
                ExecJson {
                    card: Some(card),
                    host: None,
                    ..
                } => Request::ExecInCard { card, command },
                ExecJson {
                    host: Some(host),
                    card: None,
                    ..
                } => Request::Exec { host, command },
                _ => {
                    return Err(console.error(ErrorCode::BadRequest, &t!("cli.exec.card_and_host")));
                }
            }
        }
        Command::Upload {
            id,
            local,
            remote,
            json,
        } => {
            console.json |= json;
            Request::Upload {
                host: id,
                source: absolute(local, console)?,
                destination: remote,
            }
        }
        Command::Download {
            id,
            remote,
            local,
            json,
        } => {
            console.json |= json;
            Request::Download {
                host: id,
                source: remote,
                destination: absolute(local, console)?,
            }
        }
        Command::Sync {
            id,
            local,
            remote,
            delete,
            json,
        } => {
            console.json |= json;
            Request::Sync {
                host: id,
                source: absolute(local, console)?,
                destination: remote,
                delete,
            }
        }
    })
}

/// MicaTerm 在另一个目录里跑，本地路径换成绝对路径再送过去。
fn absolute(path: PathBuf, console: &mut Console) -> Result<PathBuf, i32> {
    std::path::absolute(&path).map_err(|error| {
        console.error(
            ErrorCode::BadRequest,
            &t!(
                "cli.path.unresolvable",
                path = path.display(),
                error = error
            ),
        )
    })
}

/// stdin 上的那个 JSON 对象。不从终端读：没人说该敲什么、也看不出它在等。
fn read_json(console: &mut Console, example: &str) -> Result<serde_json::Value, i32> {
    if io::stdin().is_terminal() {
        return Err(console.error(
            ErrorCode::BadRequest,
            &t!("cli.json.from_terminal", example = example),
        ));
    }
    let text = read_stdin(console)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_json::from_str(text).map_err(|error| {
        console.error(
            ErrorCode::BadRequest,
            &t!("cli.json.not_json", error = error),
        )
    })
}

fn host_fields(console: &mut Console, example: &str) -> Result<HostFields, i32> {
    let value = read_json(console, example)?;
    manage::host_fields(value).map_err(|error| {
        console.error(
            ErrorCode::BadRequest,
            &t!("cli.json.invalid", error = error),
        )
    })
}

/// 相对当前目录或 `~` 写的私钥文件，换成 MicaTerm 找得到的绝对路径再送过去。
fn credential_fields(console: &mut Console, example: &str) -> Result<CredentialFields, i32> {
    let value = read_json(console, example)?;
    let mut fields = manage::credential_fields(value).map_err(|error| {
        console.error(
            ErrorCode::BadRequest,
            &t!("cli.json.invalid", error = error),
        )
    })?;
    if let Some(path) = fields.key_path.take() {
        let path = match path.strip_prefix("~") {
            Ok(rest) => dirs::home_dir().map(|home| home.join(rest)).unwrap_or(path),
            Err(_) => path,
        };
        fields.key_path = Some(absolute(path, console)?);
    }
    Ok(fields)
}

fn read_stdin(console: &mut Console) -> Result<String, i32> {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text).map_err(|error| {
        console.error(
            ErrorCode::BadRequest,
            &t!("cli.stdin.unreadable", error = error),
        )
    })?;
    Ok(text)
}

/// `exec --json` 读的东西。
#[derive(Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecJson {
    /// 主机 ID；和 `card` 二选一。
    #[serde(default)]
    host: Option<String>,
    /// `--card`：交给 Agent 的卡片 ID。
    #[serde(default)]
    card: Option<String>,
    command: String,
}

/// `exec --json` 的请求。PowerShell 管道送来的文本可能带 BOM。
fn exec_request(text: &str) -> Result<ExecJson, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    serde_json::from_str(text).map_err(|error| {
        t!(
            "cli.exec.bad_json",
            form = r#"{"host": ..., "command": ...}"#,
            error = error
        )
        .to_string()
    })
}

/// 远程 shell 该看到的命令。PowerShell 往程序里送的每一行都以 CRLF 结尾，POSIX shell 会把 CR
/// 当成命令的一部分；BOM 会粘在第一个词上。
fn normalize_command(command: &str) -> String {
    command
        .strip_prefix('\u{feff}')
        .unwrap_or(command)
        .replace("\r\n", "\n")
}

/// 进程启动时带的参数（不含程序名）；没有参数是 `None`，这时照常开窗口。
/// macOS 从访达打开时会加一个 `-psn_…`，不算。
pub fn command_line_arguments() -> Option<Vec<OsString>> {
    let args: Vec<OsString> = std::env::args_os()
        .skip(1)
        .filter(|arg| !arg.to_string_lossy().starts_with("-psn_"))
        .collect();
    (!args.is_empty()).then_some(args)
}

/// 这次运行的外部 CLI 端点怎么样了，设置 › Agent 据此说明。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CliStatus {
    /// 在听：开关打开就能用。
    Listening,
    /// 同一个数据目录的另一个 MicaTerm 已经在听，命令会交给它。
    Elsewhere,
    /// 没听起来，原因给用户看。
    Failed(SharedString),
    /// 没有端点：没有数据目录，或者测试里没给。
    #[default]
    Off,
}

/// 窗口壳持有的外部 CLI：在听的服务，或者为什么没在听。
pub enum CliService {
    Listening(Arc<CliServer>),
    Elsewhere,
    Failed(SharedString),
    Off,
}

impl CliService {
    /// 在数据目录 `dir` 对应的端点上开始听，要连主机的活交给 `backend`。
    pub fn start(dir: Option<&Path>, backend: Arc<dyn CliBackend>) -> Self {
        let Some(dir) = dir else {
            return CliService::Off;
        };
        Self::start_at(crate::app::cli_endpoint(dir), backend)
    }

    /// 在 `endpoint` 上开始听（测试给一个临时的端点和假的后端）。
    pub fn start_at(endpoint: PathBuf, backend: Arc<dyn CliBackend>) -> Self {
        match CliServer::start(endpoint) {
            Ok(server) => {
                server.set_backend(backend);
                CliService::Listening(Arc::new(server))
            }
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => CliService::Elsewhere,
            Err(error) => CliService::Failed(t!("cli.server.listen_failed", error = error)),
        }
    }

    pub fn server(&self) -> Option<&Arc<CliServer>> {
        match self {
            CliService::Listening(server) => Some(server),
            _ => None,
        }
    }

    pub fn status(&self) -> CliStatus {
        match self {
            CliService::Listening(_) => CliStatus::Listening,
            CliService::Elsewhere => CliStatus::Elsewhere,
            CliService::Failed(message) => CliStatus::Failed(message.clone()),
            CliService::Off => CliStatus::Off,
        }
    }
}

/// Standard output or error, made safe for a Windows console: text that is
/// not UTF-8 would be refused there. Invalid bytes become U+FFFD there; a
/// character split across two writes is held until its second half comes.
/// Anywhere else, bytes pass through untouched.
// 来自 shell-rs 的 `ConsoleText`，原样保留：exec 转发远程输出。
struct ConsoleText<W: Write> {
    inner: W,
    lossy: bool,
    /// The start of a character whose rest has not been written yet.
    pending: Vec<u8>,
}

impl<W: Write> ConsoleText<W> {
    fn new(inner: W, is_terminal: bool) -> Self {
        Self {
            inner,
            lossy: cfg!(windows) && is_terminal,
            pending: Vec::new(),
        }
    }

    /// Write out a character cut off at the very end, and flush.
    fn finish(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            self.pending.clear();
            self.inner.write_all("\u{fffd}".as_bytes())?;
        }
        self.inner.flush()
    }
}

impl<W: Write> Write for ConsoleText<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.lossy {
            return self.inner.write(bytes);
        }
        self.pending.extend_from_slice(bytes);
        let mut text = String::new();
        let mut rest: &[u8] = &self.pending;
        while !rest.is_empty() {
            match std::str::from_utf8(rest) {
                Ok(valid) => {
                    text.push_str(valid);
                    rest = &[];
                }
                Err(error) => {
                    let (valid, after) = rest.split_at(error.valid_up_to());
                    text.push_str(&String::from_utf8_lossy(valid));
                    match error.error_len() {
                        Some(invalid) => {
                            text.push('\u{fffd}');
                            rest = &after[invalid..];
                        }
                        // The rest of the character is still to come.
                        None => break,
                    }
                }
            }
        }
        self.pending = rest.to_vec();
        self.inner.write_all(text.as_bytes())?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
