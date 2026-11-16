//! 设置 › Agent：启用外部 Agent 的开关和这次运行的端点（Agent CLI 工单 01），命令行工具和
//! Agent Skills 的安装行（工单 05），状态 hook 的开关（工单 09）。装、换、删都在后台跑（`cli::CliIntegration`），失败弹通知。

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Disableable as _, FocusableExt as _, Sizable as _, switch::Switch};
use gpui_kit::*;

use super::AppSettings;
use super::controls::{page_note, reset_if, row, section, setting_switch};
use super::tab::SettingsTab;
use crate::cli::{
    AgentKind, BinaryStatus, CliIntegration, CliStatus, HOOK_AGENTS, HookStatus, IntegrationChange,
    SKILL, SkillStatus, hook_file,
};
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::modal::{ModalButtonTone, OnClick, modal_button};

pub(super) fn cli(
    tab: &SettingsTab,
    tokens: &Tokens,
    window: &mut Window,
    cx: &mut Context<SettingsTab>,
) -> Vec<AnyElement> {
    let store = tab.store.clone();
    let settings = store.read(cx).settings();
    let enabled = setting_switch(
        "cli-enabled",
        t!("settings.cli.enabled"),
        settings.cli.enabled,
        &store,
        |settings, checked| settings.cli.enabled = checked,
    );
    // 端点没听起来时说为什么；在听时说命令在哪。
    let status = match &tab.cli {
        CliStatus::Listening => t!("settings.cli.command"),
        CliStatus::Elsewhere => t!("settings.cli.elsewhere"),
        CliStatus::Failed(message) => message.clone(),
        CliStatus::Off => t!("settings.cli.off"),
    };
    let mut rows = vec![
        section(t!("settings.cli.section"), None, true, tokens),
        row(
            t!("settings.cli.enabled"),
            Some(t!("settings.cli.enabled_description")),
            reset_if(
                settings.cli.enabled != AppSettings::default().cli.enabled,
                "cli-enabled",
                &store,
                |settings| settings.cli.enabled = AppSettings::default().cli.enabled,
                tokens,
            ),
            enabled,
            tokens,
        ),
        div()
            .id("cli-status")
            .test_support()
            .aria_label(status.clone())
            .child(page_note(status, tokens))
            .into_any_element(),
        // 应用内启动的 Agent 跑哪个程序（工单 07）。
        row(
            t!("settings.cli.agent_command"),
            Some(t!("settings.cli.agent_command_description")),
            reset_if(
                settings.cli.agent_command.is_some(),
                "agent-command",
                &store,
                |settings| settings.cli.agent_command = None,
                tokens,
            ),
            tab.agent_command.clone(),
            tokens,
        ),
    ];
    if let Some(integration) = &tab.cli_integration {
        rows.extend(install_rows(integration, tokens, window, cx));
    }
    rows
}

/// 命令行工具一行，每个 Agent 的 Skill 一行，最后「复制 Skill」。
fn install_rows(
    integration: &Entity<CliIntegration>,
    tokens: &Tokens,
    window: &mut Window,
    cx: &mut Context<SettingsTab>,
) -> Vec<AnyElement> {
    let state = integration.read(cx);
    let busy = state.is_busy();
    let installable = state.paths().is_some();
    let status = state.status().cloned();
    let bin_link = state
        .paths()
        .map(|paths| paths.bin_link.display().to_string());

    // 一个改动的按钮：正在装、没有路径时点不了。
    let button = |id: SharedString,
                  label: SharedString,
                  tone: ModalButtonTone,
                  change: IntegrationChange,
                  window: &mut Window,
                  cx: &mut Context<SettingsTab>| {
        let on_click: Option<OnClick> = (installable && !busy).then(|| {
            let integration = integration.clone();
            Box::new(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let task = integration.update(cx, |integration, cx| integration.apply(change, cx));
                window
                    .spawn(cx, async move |cx| {
                        if let Err(error) = task.await {
                            let message = t!("settings.cli.install_failed", error = error);
                            cx.update(|window, cx| {
                                window.push_notification(Notification::error(message), cx)
                            })
                            .ok();
                        }
                    })
                    .detach();
            }) as OnClick
        });
        modal_button(id, label, None, tone, on_click, tokens, window, cx).into_any_element()
    };

    let (binary_note, binary_button) = match (&status, installable) {
        (_, false) => (t!("settings.cli.install_off"), None),
        (None, true) => (t!("settings.cli.checking"), None),
        (Some(status), true) => match &status.binary {
            BinaryStatus::Installed => (
                t!(
                    "settings.cli.binary_installed",
                    path = bin_link.clone().unwrap_or_default()
                ),
                Some((t!("settings.cli.remove"), IntegrationChange::RemoveBinary)),
            ),
            BinaryStatus::Missing => (
                t!("settings.cli.binary_missing"),
                Some((t!("settings.cli.install"), IntegrationChange::InstallBinary)),
            ),
            BinaryStatus::Outdated => (
                t!("settings.cli.binary_outdated"),
                Some((t!("settings.cli.update"), IntegrationChange::InstallBinary)),
            ),
            BinaryStatus::NotOnPath => (
                t!("settings.cli.binary_not_on_path"),
                Some((t!("settings.cli.install"), IntegrationChange::InstallBinary)),
            ),
            BinaryStatus::Stale { target } => (
                t!("settings.cli.binary_stale", target = target.display()),
                Some((t!("settings.cli.install"), IntegrationChange::InstallBinary)),
            ),
            BinaryStatus::Occupied { .. } => (
                t!(
                    "settings.cli.binary_occupied",
                    path = bin_link.clone().unwrap_or_default()
                ),
                None,
            ),
            BinaryStatus::Unavailable => (t!("settings.cli.binary_unavailable"), None),
        },
    };
    let binary_control = div().children(binary_button.map(|(label, change)| {
        button(
            "cli-binary".into(),
            label,
            ModalButtonTone::Line,
            change,
            window,
            cx,
        )
    }));
    let mut rows = vec![
        section(t!("settings.cli.tools"), None, false, tokens),
        div()
            .id("cli-binary-status")
            .test_support()
            .aria_label(binary_note.clone())
            .child(row(
                t!("settings.cli.binary"),
                Some(binary_note),
                None,
                binary_control,
                tokens,
            ))
            .into_any_element(),
        section(t!("settings.cli.skills"), None, false, tokens),
    ];
    for agent in AgentKind::ALL {
        let skill = status.as_ref().map(|status| status.skill(agent));
        let note = match skill {
            _ if !installable => t!("settings.cli.install_off"),
            None => t!("settings.cli.checking"),
            Some(SkillStatus::Installed) => t!("settings.cli.skill_installed"),
            Some(SkillStatus::Missing) => t!("settings.cli.skill_missing"),
            Some(SkillStatus::Outdated) => t!("settings.cli.skill_outdated"),
        };
        // WorkBuddy 没有能在 Agent 面板里跑的命令，只装 Skill：说清楚，再说本机有没有它。
        let description = match (agent, &status) {
            (AgentKind::WorkBuddy, Some(status)) if status.workbuddy_found => {
                Some(t!("settings.cli.workbuddy_found"))
            }
            (AgentKind::WorkBuddy, Some(_)) => Some(t!("settings.cli.workbuddy_missing")),
            _ => agent.description(),
        };
        let note = match description {
            Some(description) => t!("settings.cli.note_with", note = note, more = description),
            None => note,
        };
        let key = agent.key();
        let mut controls = Vec::new();
        match skill {
            Some(SkillStatus::Missing) => controls.push(button(
                format!("cli-skill-{key}-install").into(),
                t!("settings.cli.install"),
                ModalButtonTone::Line,
                IntegrationChange::InstallSkill(agent),
                window,
                cx,
            )),
            Some(SkillStatus::Outdated) => {
                controls.push(button(
                    format!("cli-skill-{key}-install").into(),
                    t!("settings.cli.update"),
                    ModalButtonTone::Line,
                    IntegrationChange::InstallSkill(agent),
                    window,
                    cx,
                ));
                controls.push(button(
                    format!("cli-skill-{key}-remove").into(),
                    t!("settings.cli.remove"),
                    ModalButtonTone::Plain,
                    IntegrationChange::RemoveSkill(agent),
                    window,
                    cx,
                ));
            }
            Some(SkillStatus::Installed) => controls.push(button(
                format!("cli-skill-{key}-remove").into(),
                t!("settings.cli.remove"),
                ModalButtonTone::Plain,
                IntegrationChange::RemoveSkill(agent),
                window,
                cx,
            )),
            None => {}
        }
        rows.push(
            div()
                .id(SharedString::from(format!("cli-skill-{key}-status")))
                .test_support()
                .aria_label(note.clone())
                .child(row(
                    agent.label(),
                    Some(note),
                    None,
                    div().flex().gap_2().children(controls),
                    tokens,
                ))
                .into_any_element(),
        );
    }
    // 别的 Agent：把 Skill 全文复制下来自己放。
    let copy: OnClick = Box::new(|_, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(SKILL.to_string()));
        window.push_notification(Notification::success(t!("settings.cli.skill_copied")), cx);
    });
    rows.push(row(
        t!("settings.cli.copy_skill"),
        Some(t!("settings.cli.copy_skill_description")),
        None,
        modal_button(
            "cli-copy-skill",
            t!("settings.cli.copy"),
            None,
            ModalButtonTone::Line,
            Some(copy),
            tokens,
            window,
            cx,
        ),
        tokens,
    ));
    let paths = integration.read(cx).paths().cloned();
    rows.extend(hook_rows(
        integration,
        paths.as_ref(),
        status.as_ref(),
        installable,
        busy,
        tokens,
    ));
    rows
}

/// 「Agent 状态」一组（工单 09，26 号设置 › Agent 页）：每个 Agent 一行开关，开着就是装了状态 hook。
/// 开关跟着配置文件里实际装没装走；装不上、读不懂时弹通知，那一行写原因。
fn hook_rows(
    integration: &Entity<CliIntegration>,
    paths: Option<&crate::cli::IntegrationPaths>,
    status: Option<&crate::cli::IntegrationStatus>,
    installable: bool,
    busy: bool,
    tokens: &Tokens,
) -> Vec<AnyElement> {
    let mut rows = vec![section(
        t!("settings.cli.hooks"),
        Some(t!("settings.cli.hooks_description")),
        false,
        tokens,
    )];
    for agent in HOOK_AGENTS {
        let hooks = status.map(|status| status.hooks(agent));
        let file = paths
            .and_then(|paths| hook_file(paths, agent))
            .map(|file| file.display().to_string())
            .unwrap_or_default();
        let note = match &hooks {
            Some(HookStatus::Blocked(reason)) => reason.clone().into(),
            _ if agent == AgentKind::ClaudeCode => {
                t!("settings.cli.hooks_claude", file = file.as_str())
            }
            _ => t!("settings.cli.hooks_codex", file = file.as_str()),
        };
        let checked = hooks == Some(HookStatus::Installed);
        let integration = integration.clone();
        let switch = Switch::new(SharedString::from(format!("cli-hooks-{}", agent.key())))
            .small()
            .focus_ring(false)
            .checked(checked)
            .disabled(!installable || busy || hooks.is_none())
            .accessibility_label(agent.label())
            .on_click(move |checked: &bool, window, cx| {
                let change = if *checked {
                    IntegrationChange::InstallHooks(agent)
                } else {
                    IntegrationChange::RemoveHooks(agent)
                };
                let task = integration.update(cx, |integration, cx| integration.apply(change, cx));
                window
                    .spawn(cx, async move |cx| {
                        if let Err(error) = task.await {
                            let message = t!("settings.cli.hooks_failed", error = error);
                            cx.update(|window, cx| {
                                window.push_notification(Notification::error(message), cx)
                            })
                            .ok();
                        }
                    })
                    .detach();
            });
        rows.push(
            div()
                .id(SharedString::from(format!(
                    "cli-hooks-{}-status",
                    agent.key()
                )))
                .test_support()
                .aria_label(note.clone())
                .child(row(agent.label(), Some(note), None, switch, tokens))
                .into_any_element(),
        );
    }
    rows
}
