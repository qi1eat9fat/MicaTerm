//! 右侧的 Agent 面板（Agent CLI 工单 07）：图标栏「Agent」展开、收起（收起时 Agent 还在）、「重新开始」；
//! 启动前缺什么先弹确认框，补好再启动；找不到 Agent 命令时不启动，「打开设置」跳到 Agent 那一页。
//! 本地终端是假的，Agent 不会真的启动；装命令、装 Skill 用临时目录。

use std::time::Duration;

use micaterm::app::{RestartAgent, ShowAgent};
use micaterm::workspace::RightPanel;
use micaterm_terminal::LocalLaunch;

use crate::settings_cli::install_paths;
use crate::support::*;

fn env_value<'a>(launch: &'a LocalLaunch, name: &str) -> Option<&'a str> {
    launch
        .env
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// 打开窗口：本地终端看 `provider`；外部 CLI 启没启用、Agent 命令是什么、装命令和 Skill 的位置由调用方定。
fn open(
    cx: &mut TestAppContext,
    provider: Arc<FixedLocalTerminalTransportProvider>,
    enabled: bool,
    command: Option<&str>,
    install: Option<micaterm::cli::IntegrationPaths>,
) -> (WindowHandle<Root>, Entity<Workspace>) {
    let services = WorkspaceServices {
        cli_install: install,
        ..services_with_terminals(provider)
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    let command = command.map(str::to_string);
    cx.update(|cx| {
        let settings = workspace.read(cx).settings().clone();
        settings.update(cx, |store, cx| {
            store.update(
                |settings| {
                    settings.cli.enabled = enabled;
                    settings.cli.agent_command = command;
                },
                cx,
            )
        })
    });
    cx.run_until_parked();
    (handle, workspace)
}

fn right_panel(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<RightPanel> {
    cx.update(|cx| workspace.read(cx).right_panel())
}

fn agent_id(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<String> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .agent_panel()
            .map(|panel| panel.self_id.to_string())
    })
}

fn has(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &'static str) -> bool {
    in_frame(cx, handle, move |window, _| window.try_find(id).is_some())
}

/// 推着时钟等后台的安装做完。
fn wait(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) {
    let started = std::time::Instant::now();
    while !done(cx) {
        assert!(started.elapsed() < Duration::from_secs(5), "等太久了");
        cx.executor().advance_clock(Duration::from_millis(20));
        cx.run_until_parked();
        in_frame(cx, handle, |_, _| {});
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui_kit::test]
fn the_rail_icon_starts_the_agent_and_collapsing_keeps_it_running(cx: &mut TestAppContext) {
    let provider = fake_local_terminals();
    let (handle, workspace) = open(
        cx,
        provider.clone(),
        true,
        Some("claude --model opus"),
        None,
    );
    let before = provider.launches().len();

    dispatch(cx, handle, ShowAgent);
    assert_eq!(right_panel(cx, &workspace), Some(RightPanel::Agent));
    assert!(has(cx, handle, "agent-panel"));
    let launches = provider.launches()[before..].to_vec();
    assert_eq!(launches.len(), 1);
    let agent = &launches[0];
    assert_eq!(agent.shell.command(), Some("claude --model opus"));
    assert!(agent.args.is_empty(), "没交给它卡片：不带提示");
    assert_eq!(env_value(agent, "TERM_PROGRAM"), Some("micaterm"));
    assert_eq!(env_value(agent, "MICATERM_CARD"), None);
    let first = agent_id(cx, &workspace).unwrap();
    assert_eq!(env_value(agent, "MICATERM_SELF_CARD"), Some(first.as_str()));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("agent-panel-name").label(), Some("Claude Code"));
    });

    // 再点收起：Agent 还在；再展开不另开。
    dispatch(cx, handle, ShowAgent);
    assert_eq!(right_panel(cx, &workspace), None);
    assert!(!has(cx, handle, "agent-panel"));
    assert_eq!(agent_id(cx, &workspace), Some(first.clone()));
    dispatch(cx, handle, ShowAgent);
    assert_eq!(right_panel(cx, &workspace), Some(RightPanel::Agent));
    assert_eq!(provider.launches().len(), before + 1);

    // 重新开始：开一个新的。
    dispatch(cx, handle, RestartAgent);
    assert_eq!(provider.launches().len(), before + 2);
    assert_ne!(agent_id(cx, &workspace), Some(first));
}

/// 外部 CLI 没启用：先确认，确认后打开总开关再启动；取消就什么也不做。
#[gpui_kit::test]
fn a_disabled_cli_is_turned_on_after_asking(cx: &mut TestAppContext) {
    let provider = fake_local_terminals();
    let (handle, workspace) = open(cx, provider.clone(), false, Some("claude"), None);
    let before = provider.launches().len();

    dispatch(cx, handle, ShowAgent);
    assert!(has(cx, handle, "agent-prep"));
    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find("modal-title").label(), Some("Claude Code"));
        assert_eq!(
            window.find(("agent-prep-step", 0usize)).label(),
            Some("启用外部 Agent")
        );
        assert!(
            window.try_find(("agent-prep-step", 1usize)).is_none(),
            "只缺这一件"
        );
        window.click("agent-prep-cancel", cx);
    });
    cx.run_until_parked();
    assert!(!has(cx, handle, "agent-prep"));
    assert_eq!(provider.launches().len(), before, "取消了不启动");
    assert_eq!(agent_id(cx, &workspace), None);

    dispatch(cx, handle, ShowAgent);
    in_frame(cx, handle, |window, cx| {
        window.click("agent-prep-confirm", cx)
    });
    wait(cx, handle, |cx| agent_id(cx, &workspace).is_some());
    let enabled = cx.update(|cx| {
        workspace
            .read(cx)
            .settings()
            .read(cx)
            .settings()
            .cli
            .enabled
    });
    assert!(enabled);
    assert!(!has(cx, handle, "agent-prep"));
    assert_eq!(right_panel(cx, &workspace), Some(RightPanel::Agent));
}

/// 命令没装（Agent 敲不了 micaterm）、Claude Code 的 Skill 没装：列出这两件，确认后装好再启动。
#[gpui_kit::test]
fn the_command_and_the_skill_are_installed_before_starting(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let paths = install_paths(root.path());
    let skill = paths.skill_file(micaterm::cli::AgentKind::ClaudeCode);
    let provider = fake_local_terminals();
    let (handle, workspace) = open(cx, provider, true, Some("claude"), Some(paths.clone()));

    dispatch(cx, handle, ShowAgent);
    in_frame(cx, handle, |window, cx| {
        assert_eq!(
            window.find(("agent-prep-step", 0usize)).label(),
            Some("安装命令行工具")
        );
        assert_eq!(
            window.find(("agent-prep-step", 1usize)).label(),
            Some("安装 Claude Code 的 Skill")
        );
        window.click("agent-prep-confirm", cx);
    });
    wait(cx, handle, |cx| agent_id(cx, &workspace).is_some());
    assert!(paths.bin_link.is_file(), "命令装上了");
    assert_eq!(
        std::fs::read_to_string(&skill).unwrap(),
        micaterm::cli::SKILL,
        "Skill 装上了"
    );
    assert!(!has(cx, handle, "agent-prep"));
}

/// 找不到 Agent 命令（设置里没填，PATH 里也没有）：不启动；「打开设置」跳到 Agent 那一页。
#[gpui_kit::test]
fn without_an_agent_command_it_points_to_the_settings(cx: &mut TestAppContext) {
    let provider = fake_local_terminals();
    let (handle, workspace) = open(cx, provider.clone(), true, None, None);
    let before = provider.launches().len();

    dispatch(cx, handle, ShowAgent);
    assert!(has(cx, handle, "agent-no-command"));
    assert_eq!(provider.launches().len(), before);
    in_frame(cx, handle, |window, cx| {
        window.click("agent-no-command-settings", cx)
    });
    cx.run_until_parked();
    let category = cx.update(|cx| {
        workspace
            .read(cx)
            .settings_tab()
            .map(|tab| tab.read(cx).category())
    });
    assert_eq!(category, Some(4), "Agent 那一页");
    assert!(
        has(cx, handle, "agent-command"),
        "Agent 命令的输入框在这一页"
    );
    assert_eq!(agent_id(cx, &workspace), None);
}
