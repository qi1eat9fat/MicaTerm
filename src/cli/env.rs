//! `micaterm env`（Agent CLI 工单 07）：一条命令讲清 Agent 在哪、能做什么，不用它翻进程、读源码。
//! MicaTerm 没开也能答（读卡片带的环境变量）；开着时再问它 CLI 开没开、交给 Agent 的卡片还在不在。
//! 退出码总是 0：说清楚现状就是这条命令的事。

use std::io;
use std::path::Path;

use serde::Serialize;

use super::agent_env::{
    CARD_ENV, CLI_ENV, HOST_ENV, SELF_CARD_ENV, TERM_PROGRAM, TERM_PROGRAM_ENV,
    TERM_PROGRAM_VERSION_ENV,
};
use super::client::{self, Console, OUTPUT_CLOSED_EXIT};
use super::protocol::{CardInfo, CliError, ErrorCode, Reply, Request};
use crate::i18n::t;

/// `micaterm env` 打印的东西；`--json` 原样输出。
#[derive(Debug, Serialize)]
pub struct EnvReport {
    /// 在 MicaTerm 的本地卡片里（`TERM_PROGRAM=micaterm`）。
    pub in_micaterm: bool,
    /// 开这张卡片的 MicaTerm 的版本。
    pub app_version: Option<String>,
    /// 这个命令自己的版本。
    pub cli_version: &'static str,
    /// 自己这张卡片的 CLI 卡片 ID。
    pub self_card: Option<String>,
    /// `micaterm` 命令的完整路径。
    pub cli: Option<String>,
    pub app: AppState,
    /// 不是 `running` 时为什么。
    pub reason: Option<String>,
    /// 交给这个 Agent 的卡片（Agent 卡片里才有）。
    pub card: Option<HandedCard>,
    /// 用户交给 Agent 的全部卡片（MicaTerm 开着、CLI 开着时）。
    pub cards: Vec<CardInfo>,
    pub commands: Vec<CommandHelp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppState {
    /// MicaTerm 开着，外部 CLI 启用了。
    Running,
    NotRunning,
    /// MicaTerm 开着，外部 CLI 没启用。
    NotEnabled,
    /// 连不上或说不通（版本不对、没权限）。
    Unreachable,
}

/// 交给这个 Agent 的那张卡片。
#[derive(Debug, Serialize)]
pub struct HandedCard {
    pub id: String,
    /// 交出去时那张卡片的主机 ID。
    pub host: Option<String>,
    /// 还交着；问不到 MicaTerm 时不知道。
    pub handed: Option<bool>,
    pub host_name: Option<String>,
    pub connected: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct CommandHelp {
    pub command: &'static str,
    pub what: String,
}

/// 能用的命令，一行一个例子。
const COMMANDS: [(&str, &str); 14] = [
    ("micaterm env", "cli.env.command.env"),
    (
        "micaterm hosts list [-q <keyword>]",
        "cli.env.command.hosts_list",
    ),
    (
        "micaterm hosts show <host-id>",
        "cli.env.command.hosts_show",
    ),
    (
        "micaterm exec <host-id> \"<command>\"",
        "cli.env.command.exec",
    ),
    (
        "micaterm exec --card <card-id> \"<command>\"",
        "cli.env.command.exec_card",
    ),
    ("micaterm cards list", "cli.env.command.cards_list"),
    (
        "micaterm cards read <card-id> [--lines N]",
        "cli.env.command.cards_read",
    ),
    (
        "micaterm cards send-key <card-id> <key>",
        "cli.env.command.cards_send_key",
    ),
    (
        "micaterm upload <host-id> <local> <remote>",
        "cli.env.command.upload",
    ),
    (
        "micaterm download <host-id> <remote> <local>",
        "cli.env.command.download",
    ),
    (
        "micaterm sync <host-id> <local-dir> <remote-dir> [--delete]",
        "cli.env.command.sync",
    ),
    (
        "micaterm hosts create|update|delete",
        "cli.env.command.hosts_manage",
    ),
    (
        "micaterm credentials list|show|create|update|delete",
        "cli.env.command.credentials",
    ),
    ("micaterm <command> --help", "cli.env.command.help"),
];

/// 按环境变量（`var`）和 MicaTerm 的回答（`cards list` 的，问不到时为什么）拼出报告。
pub fn report(var: impl Fn(&str) -> Option<String>, answer: Result<Reply, CliError>) -> EnvReport {
    let present = |name: &str| var(name).filter(|value| !value.is_empty());
    let (app, reason, cards) = match answer {
        Ok(Reply::Cards { cards }) => (AppState::Running, None, cards),
        Ok(_) => (AppState::Unreachable, None, Vec::new()),
        Err(error) => {
            let state = match error.code {
                ErrorCode::NotRunning => AppState::NotRunning,
                ErrorCode::NotEnabled => AppState::NotEnabled,
                _ => AppState::Unreachable,
            };
            (state, Some(error.message), Vec::new())
        }
    };
    let card = present(CARD_ENV).map(|id| {
        let found = cards.iter().find(|card| card.id == id);
        HandedCard {
            host: present(HOST_ENV),
            handed: (app == AppState::Running).then_some(found.is_some()),
            host_name: found.map(|card| card.host_name.clone()),
            connected: found.map(|card| card.connected),
            id,
        }
    });
    EnvReport {
        in_micaterm: present(TERM_PROGRAM_ENV).as_deref() == Some(TERM_PROGRAM),
        app_version: present(TERM_PROGRAM_VERSION_ENV),
        cli_version: env!("CARGO_PKG_VERSION"),
        self_card: present(SELF_CARD_ENV),
        cli: present(CLI_ENV),
        app,
        reason,
        card,
        cards,
        commands: COMMANDS
            .iter()
            .map(|(command, key)| CommandHelp {
                command,
                what: t!(*key).to_string(),
            })
            .collect(),
    }
}

/// `micaterm env`：问 `endpoint` 上的 MicaTerm（找不到数据目录时 `Err`），打印报告。
pub fn run(endpoint: Result<&Path, &io::Error>, console: &mut Console) -> i32 {
    let answer = match endpoint {
        Ok(endpoint) => client::query(endpoint, Request::ListCards),
        Err(error) => Err(CliError::new(
            ErrorCode::ConnectFailed,
            t!("cli.client.no_data_dir", error = error),
        )),
    };
    let report = report(|name| std::env::var(name).ok(), answer);
    match print(&report, console) {
        Ok(()) => 0,
        Err(_) => OUTPUT_CLOSED_EXIT,
    }
}

fn print(report: &EnvReport, console: &mut Console) -> io::Result<()> {
    if console.json {
        serde_json::to_writer_pretty(&mut *console.stdout, report).map_err(io::Error::other)?;
        return writeln!(console.stdout);
    }
    let out = &mut *console.stdout;
    match (&report.self_card, report.in_micaterm) {
        (Some(card), true) => writeln!(
            out,
            "{}",
            t!(
                "cli.env.in_card",
                version = report.app_version.as_deref().unwrap_or("?"),
                card = card
            )
        )?,
        _ => writeln!(out, "{}", t!("cli.env.not_in_card"))?,
    }
    if let Some(cli) = &report.cli {
        writeln!(out, "{}", t!("cli.env.cli", path = cli))?;
    }
    let app = match report.app {
        AppState::Running => t!("cli.env.app.running"),
        AppState::NotRunning => t!("cli.env.app.not_running"),
        AppState::NotEnabled => t!("cli.env.app.not_enabled"),
        AppState::Unreachable => t!(
            "cli.env.app.unreachable",
            reason = report.reason.as_deref().unwrap_or_default()
        ),
    };
    writeln!(out, "{app}")?;
    if let Some(card) = &report.card {
        let line = match (card.handed, &card.host_name, card.connected) {
            (Some(true), Some(name), Some(true)) => {
                t!("cli.env.card.connected", id = card.id, host = name)
            }
            (Some(true), Some(name), _) => {
                t!("cli.env.card.disconnected", id = card.id, host = name)
            }
            (Some(false), _, _) => t!("cli.env.card.taken_back", id = card.id),
            _ => t!("cli.env.card.unknown", id = card.id),
        };
        writeln!(out, "{line}")?;
    }
    if report.app == AppState::Running {
        writeln!(out, "{}", t!("cli.env.cards", count = report.cards.len()))?;
    }
    writeln!(out)?;
    writeln!(out, "{}", t!("cli.env.commands"))?;
    let width = report
        .commands
        .iter()
        .map(|help| help.command.len())
        .max()
        .unwrap_or(0);
    for help in &report.commands {
        writeln!(out, "  {:width$}  {}", help.command, help.what)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    const AGENT_CARD: &[(&str, &str)] = &[
        ("TERM_PROGRAM", "micaterm"),
        ("TERM_PROGRAM_VERSION", "0.1.0"),
        ("MICATERM_SELF_CARD", "SelfSelfSelfSelf"),
        ("MICATERM_CLI", "/opt/micaterm/micaterm"),
        ("MICATERM_CARD", "CardAAAAAAAAAAAA"),
        ("MICATERM_HOST", "WendaoSmoke00001"),
    ];

    fn printed(report: &EnvReport, json: bool) -> String {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        print(
            report,
            &mut Console {
                stdout: &mut stdout,
                stderr: &mut stderr,
                json,
                exec_json: false,
                stderr_is_terminal: false,
            },
        )
        .unwrap();
        String::from_utf8(stdout).unwrap()
    }

    #[test]
    fn without_micaterm_it_still_says_what_the_card_told_it() {
        let report = report(
            vars(AGENT_CARD),
            Err(CliError::new(ErrorCode::NotRunning, "没有运行")),
        );
        assert!(report.in_micaterm);
        assert_eq!(report.self_card.as_deref(), Some("SelfSelfSelfSelf"));
        assert_eq!(report.app, AppState::NotRunning);
        let card = report.card.as_ref().unwrap();
        assert_eq!(card.handed, None, "问不到就不知道");
        assert_eq!(card.host.as_deref(), Some("WendaoSmoke00001"));
        let text = printed(&report, false);
        assert!(text.contains("SelfSelfSelfSelf"), "{text}");
        assert!(text.contains("/opt/micaterm/micaterm"), "{text}");
        assert!(text.contains("micaterm exec --card <card-id>"), "{text}");
    }

    #[test]
    fn outside_a_card_and_with_the_cli_off_it_says_so() {
        let report = report(
            vars(&[]),
            Err(CliError::new(ErrorCode::NotEnabled, "没启用")),
        );
        assert!(!report.in_micaterm);
        assert_eq!(report.app, AppState::NotEnabled);
        assert!(report.card.is_none());
        let json: serde_json::Value = serde_json::from_str(&printed(&report, true)).unwrap();
        assert_eq!(json["app"], "not_enabled");
        assert_eq!(json["self_card"], serde_json::Value::Null);
        assert_eq!(json["commands"][0]["command"], "micaterm env");
    }

    #[test]
    fn with_micaterm_running_it_says_whether_the_card_is_still_handed() {
        let card = CardInfo {
            id: "CardAAAAAAAAAAAA".into(),
            host: "WendaoSmoke00001".into(),
            host_name: "wendao".into(),
            connected: true,
            running: false,
        };
        let report = report(vars(AGENT_CARD), Ok(Reply::Cards { cards: vec![card] }));
        let handed = report.card.as_ref().unwrap();
        assert_eq!(handed.handed, Some(true));
        assert_eq!(handed.host_name.as_deref(), Some("wendao"));
        assert!(printed(&report, false).contains("wendao"));

        let report = report_without_cards();
        assert_eq!(report.card.as_ref().unwrap().handed, Some(false));
    }

    fn report_without_cards() -> EnvReport {
        report(vars(AGENT_CARD), Ok(Reply::Cards { cards: Vec::new() }))
    }
}
