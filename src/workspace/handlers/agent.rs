//! 应用内的 Agent（Agent CLI 工单 07）：右侧图标栏的「Agent」展开、收起 Agent 面板；远程卡片右键
//! 「交给 Agent」把卡片交给面板里的 Agent；面板的「重新开始」。样子和规则在 `crate::agent`。
//!
//! 启动前先看缺什么（外部 CLI、命令、Skill），缺了先弹确认框，补好再启动；找不到 Agent 命令时
//! 不启动，提示去设置里填——但「交给 Agent」照样把卡片交出去（2026-11-16 用户定），Agent 可以跑在
//! MicaTerm 外面的终端里，用 `micaterm cards list` 找到它。整个窗口只有一个 Agent：已经在跑时交给它的卡片说明填进它的输入框，
//! 不按回车（不打断它、不替用户发话）。

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;

use micaterm_terminal::{LocalLaunch, LocalShell, TerminalView};

use crate::agent::{
    AgentPanel, AgentProgram, Handoff, Prep, agent_program_on, handoff_prompt, open_prep_dialog,
    prep_steps,
};
use crate::app::{RestartAgent, ShowAgent};
use crate::card::CardId;
use crate::cli::{CARD_ENV, HOST_ENV, IntegrationStatus};
use crate::i18n::t;
use crate::ui::DialogLayer;
use crate::ui::confirm::{Choice, ChoiceButton, ChoiceTone, ConfirmLine, open_choice_dialog};
use crate::workspace::Workspace;
use crate::workspace::right_sidebar::RightPanel;

pub(in crate::workspace) fn bind_agent_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_show_agent))
        .on_action(cx.listener(Workspace::on_restart_agent))
}

impl Workspace {
    /// 右侧图标栏的「Agent」：展开着就收起（Agent 照常在跑）；Agent 在跑就展开；否则先准备再启动。
    fn on_show_agent(&mut self, _: &ShowAgent, window: &mut Window, cx: &mut Context<Self>) {
        if self.right_panel == Some(RightPanel::Agent) {
            self.right_panel = None;
            if let Some(agent) = &mut self.agent {
                agent.note = None;
            }
            // 焦点在面板的终端里时，面板一收它就没了去处：还给在前的标签（快捷键才有焦点路径可走）。
            self.focus_active_tab(window, cx);
            return;
        }
        if self
            .agent
            .as_ref()
            .is_some_and(|agent| agent.is_running(cx))
        {
            self.show_agent_panel(window, cx);
            return;
        }
        self.prepare_agent(None, false, window, cx);
    }

    /// 面板的「重新开始」：结束面板里的 Agent，开一个新的。
    fn on_restart_agent(&mut self, _: &RestartAgent, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_agent(None, true, window, cx);
    }

    /// 远程卡片右键「交给 Agent」：准备好以后把卡片交出去，交给面板里的 Agent。
    pub(in crate::workspace) fn give_card_to_agent(
        &mut self,
        card: CardId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remote = self
            .card(card, cx)
            .is_some_and(|entity| entity.read(cx).remote_host().is_some());
        if remote {
            self.prepare_agent(Some(card), false, window, cx);
        }
    }

    /// 看缺什么：找不到 Agent 命令就提示去设置；缺外部 CLI、命令、Skill 就先确认、补上；
    /// 都好了启动（`restart` 时先结束面板里的）。
    fn prepare_agent(
        &mut self,
        give: Option<CardId>,
        restart: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.settings.read(cx).settings();
        let program = agent_program_on(
            settings.cli.agent_command.as_deref(),
            self.services.agent_path.as_deref(),
        );
        let Some(program) = program else {
            match give {
                Some(card) => self.hand_over_without_agent(card, window, cx),
                None => self.tell_no_agent_command(window, cx),
            }
            return;
        };
        let paths = self.cli_integration.read(cx).paths().cloned();
        let status = paths.as_ref().map(IntegrationStatus::read);
        let steps = prep_steps(
            settings.cli.enabled,
            paths.as_ref(),
            status.as_ref(),
            program.kind,
        );
        if steps.is_empty() {
            self.start_agent(program, give, restart, window, cx);
            return;
        }
        let card = give
            .and_then(|card| self.card(card, cx))
            .map(|card| card.read(cx).name());
        let prep = Prep {
            name: program.name.clone(),
            card,
            steps,
        };
        let this = cx.entity().downgrade();
        open_prep_dialog(
            prep,
            self.settings.clone(),
            self.cli_integration.clone(),
            Box::new(move |window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.start_agent(program, give, restart, window, cx)
                });
            }),
            window,
            cx,
        );
    }

    /// 启动（或交给已在跑的）Agent，展开面板、焦点给它。
    fn start_agent(
        &mut self,
        program: AgentProgram,
        give: Option<CardId>,
        restart: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let handoff = give.and_then(|card| self.hand_over(card, cx));
        let running = !restart
            && self
                .agent
                .as_ref()
                .is_some_and(|agent| agent.is_running(cx));
        if running {
            if let (Some(handoff), Some(agent)) = (&handoff, &mut self.agent) {
                let text = handoff_prompt(handoff);
                agent.terminal.update(cx, |terminal, cx| {
                    let _ = terminal.enter_command(&text, false, false, cx);
                });
                agent.note = Some(t!("agent.panel.handed", card = handoff.card_name.clone()));
            }
        } else {
            if let Some(old) = self.agent.take() {
                old.terminal
                    .update(cx, |terminal, cx| terminal.shutdown(cx));
            }
            let mut launch = LocalLaunch {
                shell: LocalShell::new(&program.command),
                args: handoff.iter().map(handoff_prompt).collect(),
                ..LocalLaunch::default()
            };
            if let Some(handoff) = &handoff {
                launch
                    .env
                    .push((CARD_ENV.to_string(), handoff.card.to_string()));
                launch
                    .env
                    .push((HOST_ENV.to_string(), handoff.host.to_string()));
            }
            let self_id = self.add_identity(&mut launch, cx);
            let factory = self.services.local_terminals.factory_for(&launch);
            let terminal = cx.new(|cx| {
                TerminalView::new(
                    "agent-terminal",
                    |_| t!("agent.panel.title"),
                    factory,
                    window,
                    cx,
                )
            });
            self.agent_screens.forget(crate::workspace::agent_state::AgentSlot::Panel);
            self.agent = Some(AgentPanel {
                terminal,
                name: program.name,
                self_id,
                note: None,
                kind: program.kind,
                tracker: Default::default(),
                turn_seen: true,
            });
        }
        self.show_agent_panel(window, cx);
        self.sync_cli_cards(cx);
    }

    /// 把远程卡片交出去，说给 Agent 听的那几样。不是远程卡片时 `None`。
    fn hand_over(&mut self, card: CardId, cx: &mut Context<Self>) -> Option<Handoff> {
        let entity = self.card(card, cx)?;
        let id = entity.update(cx, |card, cx| card.hand_to_agent(cx))?;
        let card = entity.read(cx);
        let host = card.remote_host_snapshot()?;
        Some(Handoff {
            card: id,
            card_name: card.name(),
            host: host.public_id.to_string().into(),
        })
    }

    /// 找不到 Agent 命令时的「交给 Agent」：照样交出去，不开面板；弹一条通知说外面的 Agent 怎么找到它，
    /// 「启用外部 Agent」没开时说它还用不了。
    fn hand_over_without_agent(
        &mut self,
        card: CardId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handoff) = self.hand_over(card, cx) else {
            return;
        };
        self.sync_cli_cards(cx);
        let notice = if self.settings.read(cx).settings().cli.enabled {
            Notification::info(t!(
                "agent.handoff.no_agent",
                card = handoff.card_name.clone()
            ))
        } else {
            Notification::warning(t!(
                "agent.handoff.no_agent_cli_off",
                card = handoff.card_name.clone()
            ))
        };
        window.push_notification(notice, cx);
    }

    pub(in crate::workspace) fn show_agent_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.right_panel = Some(RightPanel::Agent);
        // 看见了：回合结束的点熄掉（工单 09）。
        if let Some(agent) = &mut self.agent {
            agent.turn_seen = true;
        }
        if let Some(agent) = &self.agent {
            let focus = agent.terminal.read(cx).focus_handle();
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// 找不到 Agent 命令：不启动，「打开设置」跳到 Agent 那一页。
    fn tell_no_agent_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        open_choice_dialog(
            Choice {
                id: "agent-no-command",
                title: t!("agent.no_command.title"),
                subtitle: None,
                lines: vec![ConfirmLine::Text(t!("agent.no_command.text"))],
                check: None,
                buttons: vec![
                    ChoiceButton::new("cancel", t!("agent.no_command.cancel")),
                    ChoiceButton::new("settings", t!("agent.no_command.settings"))
                        .tone(ChoiceTone::Primary),
                ],
            },
            DialogLayer::new(),
            Box::new(move |outcome, window, cx| {
                if outcome.button == Some("settings") {
                    let _ = this.update(cx, |this, cx| this.open_cli_settings(window, cx));
                }
            }),
            window,
            cx,
        );
    }

    /// 面板里的 Agent，测试用。
    pub fn agent_panel(&self) -> Option<&AgentPanel> {
        self.agent.as_ref()
    }
}
