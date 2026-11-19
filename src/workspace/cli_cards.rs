//! 交给 Agent 的卡片（Agent CLI 工单 06）：右键「交给 Agent」「收回 Agent」，定时把交出去的卡片列给
//! 外部 CLI 的服务（`cards list`），把排着的 `exec --card` 交给卡片的终端，`cards read` /
//! `cards send-key` 在 UI 线程上找卡片做掉。CLI 只认交出去的卡片：没交出去、收回了、关了的一律
//! 回 `no_terminal`，跟卡片不存在一样。
// 参考 since2006/shell-rs @ b2a307a: src/workspace/cli.rs 的终端那部分重写：那边按主机找终端，
// 这里只认用户交出去的卡片。

use gpui_kit::*;

use micaterm_terminal::{KeyRefused, RunFailure, RunHandle};

use super::Workspace;
use crate::card::{CardId, RemoteState, TerminalCard};
use crate::cli::{CardInfo, CardScreen, CliError, ErrorCode, Reply};
use crate::i18n::t;

/// `cards read` 不给行数时读多少行。
const DEFAULT_READ_LINES: usize = 120;
/// `cards read --lines` 最多读多少行。
const MAX_READ_LINES: usize = 2000;
/// `cards send-key --repeat` 最多重复多少次。
const MAX_KEY_REPEAT: usize = 100;

impl Workspace {
    // 交出去在 `handlers/agent.rs`（工单 07：交给右侧面板里的 Agent，先看缺什么）。

    /// 收回：CLI 卡片 ID 随即作废。正在跑的 `exec --card` 不打断，它跑完照常回话。
    pub(super) fn take_card_back(&mut self, card: CardId, cx: &mut Context<Self>) {
        if let Some(card) = self.card(card, cx) {
            card.update(cx, |card, cx| card.take_back(cx));
            self.sync_cli_cards(cx);
        }
    }

    /// 交出去的卡片和它们的 CLI 卡片 ID，按标签、卡片的顺序。
    fn agent_cards(&self, cx: &App) -> Vec<(SharedString, Entity<TerminalCard>)> {
        self.terminal_tabs()
            .flat_map(|tab| tab.cards())
            .filter_map(|entity| {
                let id = entity.read(cx).agent_id()?.clone();
                Some((id, entity.clone()))
            })
            .collect()
    }

    /// CLI 卡片 ID 为 `id` 的交出去的卡片。
    fn agent_card(&self, id: &str, cx: &App) -> Result<Entity<TerminalCard>, CliError> {
        self.agent_cards(cx)
            .into_iter()
            .find(|(agent, _)| agent.as_ref() == id)
            .map(|(_, card)| card)
            .ok_or_else(no_terminal)
    }

    /// 把交出去的卡片列给服务；顺带告诉卡片 Agent 在不在它那里执行命令（状态栏的标记）。
    pub(super) fn sync_cli_cards(&self, cx: &mut App) {
        let Some(server) = self.services.cli.server().cloned() else {
            return;
        };
        let mut cards = Vec::new();
        for (id, entity) in self.agent_cards(cx) {
            let running = entity.read(cx).terminal().read(cx).is_running_command(cx);
            entity.update(cx, |card, cx| card.set_agent_running(running, cx));
            let card = entity.read(cx);
            let Some(host) = card.remote_host_snapshot() else {
                continue;
            };
            cards.push(CardInfo {
                id: id.to_string(),
                host: host.public_id.to_string(),
                host_name: host.name.to_string(),
                connected: card.remote_state(cx) == Some(RemoteState::Connected),
                running,
            });
        }
        server.set_cards(cards);
    }

    /// 排着的 `exec --card` 一个个交给卡片的终端；找不到卡片、卡片没连上时直接告诉服务为什么。
    pub(super) fn take_cli_runs(&self, cx: &App) {
        let Some(server) = self.services.cli.server() else {
            return;
        };
        while let Some((card, run)) = server.take_run() {
            if let Err(failure) = self.start_run(&card, run.clone(), cx) {
                run.fail(failure);
            }
        }
    }

    fn start_run(&self, card: &str, run: RunHandle, cx: &App) -> Result<(), RunFailure> {
        let entity = self
            .agent_card(card, cx)
            .map_err(|_| RunFailure::NoTerminal)?;
        let card = entity.read(cx);
        if card.remote_state(cx) != Some(RemoteState::Connected) {
            return Err(RunFailure::NotConnected);
        }
        card.terminal().read(cx).run_command(run, cx)
    }

    /// `cards read`：从卡片缓冲区底部往上读。
    pub(super) fn read_cli_card(
        &self,
        card: &str,
        lines: Option<usize>,
        cx: &App,
    ) -> Result<Reply, CliError> {
        let entity = self.agent_card(card, cx)?;
        if entity.read(cx).remote_state(cx) != Some(RemoteState::Connected) {
            return Err(not_connected());
        }
        let lines = lines.unwrap_or(DEFAULT_READ_LINES).clamp(1, MAX_READ_LINES);
        let read = entity.read(cx).terminal().read(cx).read_tail(lines, cx);
        Ok(Reply::Screen(CardScreen {
            text: read.text,
            returned_lines: read.returned_lines,
            history_available: read.history_available,
            truncated: read.truncated,
            alternate_screen: read.alternate_screen,
        }))
    }

    /// `cards send-key`：在卡片里按一个命名键。有 `exec --card` 在跑时只收 `ctrl-c`：别的键会
    /// 打乱那条命令。
    pub(super) fn send_cli_key(
        &self,
        card: &str,
        key: &str,
        repeat: Option<usize>,
        cx: &App,
    ) -> Result<Reply, CliError> {
        let entity = self.agent_card(card, cx)?;
        let card = entity.read(cx);
        if card.remote_state(cx) != Some(RemoteState::Connected) {
            return Err(not_connected());
        }
        let terminal = card.terminal().read(cx);
        let interrupt = key.trim().eq_ignore_ascii_case("ctrl-c");
        if terminal.is_running_command(cx) && !interrupt {
            return Err(CliError::new(
                ErrorCode::TerminalBusy,
                t!("cli.card.key_while_running"),
            ));
        }
        let repeat = repeat.unwrap_or(1).clamp(1, MAX_KEY_REPEAT);
        match terminal.send_named_key(key, repeat, cx) {
            Ok(()) => Ok(Reply::KeySent {
                key: key.to_string(),
                repeat,
            }),
            Err(KeyRefused::Unknown) => Err(CliError::new(
                ErrorCode::BadRequest,
                t!("cli.card.unknown_key", key = key),
            )),
            Err(KeyRefused::NotConnected) => Err(not_connected()),
        }
    }
}

fn no_terminal() -> CliError {
    CliError::new(ErrorCode::NoTerminal, t!("cli.card.not_found"))
}

fn not_connected() -> CliError {
    CliError::new(ErrorCode::NotConnected, t!("cli.card.not_connected"))
}
