//! 右侧的 Agent 面板（Agent CLI 工单 07）：图标栏「Agent」展开、收起（收起时 Agent 还在）、「重新开始」；
//! 启动前缺什么先弹确认框，补好再启动；找不到 Agent 命令时不启动，「打开设置」跳到 Agent 那一页。
//! 本地终端是假的，Agent 不会真的启动；装命令、装 Skill 用临时目录。

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::ClipboardItem;
use micaterm::app::{RestartAgent, ShowAgent};
use micaterm::workspace::RightPanel;
use micaterm_terminal::LocalLaunch;
use micaterm_terminal::TerminalLifecycle;
use micaterm_terminal::actions::PasteTerminal;

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

fn has(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    id: impl Into<ElementId> + 'static,
) -> bool {
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

    // 先弹启动框，选了才启动。
    dispatch(cx, handle, ShowAgent);
    assert!(has(cx, handle, "agent-prep"));
    assert_eq!(provider.launches().len(), before);
    start_new_agent(cx, handle);
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

    // 重新开始：也先弹启动框；取消什么也不变，选了才开新的。
    dispatch(cx, handle, RestartAgent);
    assert_eq!(provider.launches().len(), before + 1);
    in_frame(cx, handle, |window, cx| {
        window.click("agent-prep-cancel", cx)
    });
    cx.run_until_parked();
    assert_eq!(agent_id(cx, &workspace), Some(first.clone()));
    dispatch(cx, handle, RestartAgent);
    start_new_agent(cx, handle);
    assert_eq!(provider.launches().len(), before + 2);
    assert_ne!(agent_id(cx, &workspace), Some(first));
}

/// 标题行右上角的「关闭」收起面板，Agent 还在跑、不另开；图标栏再点一下展开同一个。
#[gpui_kit::test]
fn the_close_button_collapses_the_panel_keeping_the_agent(cx: &mut TestAppContext) {
    let provider = fake_local_terminals();
    let (handle, workspace) = open(cx, provider.clone(), true, Some("claude"), None);
    dispatch(cx, handle, ShowAgent);
    start_new_agent(cx, handle);
    let agent = agent_id(cx, &workspace).expect("the agent is running");
    let launches = provider.launches().len();

    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.find("agent-panel-close").label(), Some("Close"));
        window.click("agent-panel-close", cx);
    });
    cx.run_until_parked();
    assert_eq!(right_panel(cx, &workspace), None);
    assert!(!has(cx, handle, "agent-panel"));
    assert_eq!(agent_id(cx, &workspace), Some(agent.clone()), "Agent 还在");

    dispatch(cx, handle, ShowAgent);
    assert_eq!(right_panel(cx, &workspace), Some(RightPanel::Agent));
    assert_eq!(agent_id(cx, &workspace), Some(agent));
    assert_eq!(provider.launches().len(), launches, "展开不另开");
}

fn label(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    id: impl Into<ElementId> + 'static,
) -> Option<String> {
    in_frame(cx, handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.label().map(str::to_owned))
    })
}

fn stored_folder(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<PathBuf> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .settings()
            .read(cx)
            .settings()
            .cli
            .agent_directory
    })
}

/// 在 `history` 里给 `folder` 写一个 Claude Code 对话记录：标题 `title`，最后改动在 `minutes` 分钟时。
fn record(history: &Path, folder: &Path, id: &str, title: &str, minutes: u64) {
    let project: String = folder
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = history.join("projects").join(project);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{id}.jsonl"));
    std::fs::write(
        &file,
        format!(r#"{{"type":"custom-title","customTitle":"{title}"}}"#),
    )
    .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(minutes * 60))
        .unwrap();
}

/// 启动框（2026-11-16 用户定）：文件夹默认主目录，「选择…」换；对话列表第一行「新对话」，下面是
/// 这个文件夹里 Claude Code 的历史对话，最近的在前、默认选它，启动带 `--resume <ID>`；选「新对话」
/// 不带。选的文件夹记进设置，下次默认还是它；不在了默认回主目录。
#[gpui_kit::test]
fn the_start_dialog_picks_the_folder_and_the_conversation(cx: &mut TestAppContext) {
    let history = tempfile::tempdir().unwrap();
    let provider = fake_local_terminals();
    let services = WorkspaceServices {
        agent_history: Some(history.path().to_path_buf()),
        ..services_with_terminals(provider.clone())
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    cx.update(|cx| {
        let settings = workspace.read(cx).settings().clone();
        settings.update(cx, |store, cx| {
            store.update(
                |settings| {
                    settings.cli.enabled = true;
                    settings.cli.agent_command = Some("claude".into());
                },
                cx,
            )
        })
    });
    cx.run_until_parked();
    let before = provider.launches().len();
    let home = dirs::home_dir().unwrap().to_string_lossy().into_owned();

    // 主目录没聊过：只有「新对话」，选着它。
    dispatch(cx, handle, ShowAgent);
    assert_eq!(
        label(cx, handle, "agent-prep-folder"),
        Some(home.clone()),
        "没选过：主目录"
    );
    assert_eq!(
        label(cx, handle, "agent-prep-new").as_deref(),
        Some("New Conversation (selected)")
    );
    assert!(!has(cx, handle, ("agent-prep-conversation", 0usize)));

    // 「选择…」：取消不变；选了换上，列出那里的历史对话。
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("工作");
    std::fs::create_dir(&folder).unwrap();
    record(history.path(), &folder, "older", "修登录页", 10);
    record(history.path(), &folder, "newest", "加启动框", 30);
    record(history.path(), &folder, "middle", "SFTP 续传", 20);
    in_frame(cx, handle, |window, cx| {
        window.click("agent-prep-folder-choose", cx)
    });
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    assert_eq!(label(cx, handle, "agent-prep-folder"), Some(home.clone()));
    in_frame(cx, handle, |window, cx| {
        window.click("agent-prep-folder-choose", cx)
    });
    cx.run_until_parked();
    let chosen = folder.clone();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files && !options.multiple);
        Some(vec![chosen])
    });
    cx.run_until_parked();
    let full = folder.to_string_lossy().into_owned();
    assert_eq!(label(cx, handle, "agent-prep-folder"), Some(full.clone()));
    let titles: Vec<Option<String>> = (0..3usize)
        .map(|n| label(cx, handle, ("agent-prep-conversation", n)))
        .collect();
    assert_eq!(
        titles,
        [
            Some("加启动框 (selected)".to_string()),
            Some("SFTP 续传".to_string()),
            Some("修登录页".to_string()),
        ],
        "最近的在前、默认选它"
    );
    assert_eq!(
        label(cx, handle, "agent-prep-new").as_deref(),
        Some("New Conversation")
    );
    assert_eq!(stored_folder(cx, &workspace), None, "启动前不记");

    // 挑第三个，启动：接着它。
    in_frame(cx, handle, |window, cx| {
        window.click(("agent-prep-conversation", 2usize), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        label(cx, handle, ("agent-prep-conversation", 2usize)).as_deref(),
        Some("修登录页 (selected)")
    );
    in_frame(cx, handle, |window, cx| {
        window.click("agent-prep-start", cx)
    });
    cx.run_until_parked();
    let launch = provider.launches()[before].clone();
    assert_eq!(launch.directory.as_deref(), Some(folder.as_path()));
    assert_eq!(launch.args, ["--resume", "older"]);
    assert_eq!(
        stored_folder(cx, &workspace).as_deref(),
        Some(folder.as_path())
    );
    assert_eq!(label(cx, handle, "agent-panel-folder"), Some(full.clone()));

    // 重新开始：默认还是这个文件夹、最近的对话；选「新对话」不带参数。
    dispatch(cx, handle, RestartAgent);
    assert_eq!(label(cx, handle, "agent-prep-folder"), Some(full));
    assert_eq!(
        label(cx, handle, ("agent-prep-conversation", 0usize)).as_deref(),
        Some("加启动框 (selected)")
    );
    start_new_agent(cx, handle);
    let launch = provider.launches()[before + 1].clone();
    assert_eq!(launch.directory.as_deref(), Some(folder.as_path()));
    assert!(launch.args.is_empty());

    // 文件夹没了：默认回主目录，设置里的不动。
    std::fs::remove_dir_all(&folder).unwrap();
    dispatch(cx, handle, RestartAgent);
    assert_eq!(label(cx, handle, "agent-prep-folder"), Some(home));
    assert_eq!(
        stored_folder(cx, &workspace).as_deref(),
        Some(folder.as_path())
    );
}

/// Codex 不能接着以前的对话：启动框里只有「新对话」，下面说它只能开新对话。
#[gpui_kit::test]
fn an_agent_that_cannot_resume_only_starts_new_conversations(cx: &mut TestAppContext) {
    let provider = fake_local_terminals();
    let (handle, _workspace) = open(cx, provider.clone(), true, Some("codex"), None);
    dispatch(cx, handle, ShowAgent);
    assert_eq!(
        label(cx, handle, "agent-prep-conversation-note").as_deref(),
        Some("Codex can only start a new conversation")
    );
    assert!(!has(cx, handle, ("agent-prep-conversation", 0usize)));
    let before = provider.launches().len();
    in_frame(cx, handle, |window, cx| {
        window.click("agent-prep-start", cx)
    });
    cx.run_until_parked();
    assert_eq!(provider.launches().len(), before + 1);
    assert!(provider.launches()[before].args.is_empty());
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
            Some("Enable External Agents")
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
    start_new_agent(cx, handle);
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
            Some("Install the command-line tool")
        );
        assert_eq!(
            window.find(("agent-prep-step", 1usize)).label(),
            Some("Install the Claude Code Skill")
        );
        window.click("agent-prep-start", cx);
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

/// 焦点在 Agent 面板的终端里时，终端动作（右键菜单的粘贴、复制，Ctrl+Shift+V 这些）的对象是它，
/// 不是在前的卡片（2026-11-19：右键菜单的「粘贴」粘到了卡片的终端里）。
#[gpui_kit::test]
fn terminal_actions_in_the_agent_panel_go_to_the_agent(cx: &mut TestAppContext) {
    let provider = fake_local_terminals();
    let (handle, workspace) = open(cx, provider, true, Some("claude"), None);
    dispatch(cx, handle, NewLocalTerminalTab);
    dispatch(cx, handle, ShowAgent);
    start_new_agent(cx, handle);

    // 两个终端都跑起来了、排好版：卡片的假 shell 先打出提示符。
    wait(cx, handle, |cx| {
        cx.update(|cx| {
            let agent = workspace.read(cx).agent_panel().is_some_and(|panel| {
                panel.terminal.read(cx).lifecycle(cx) == TerminalLifecycle::Running
            });
            let card = workspace
                .read(cx)
                .card_screen_text(CardId(1), cx)
                .is_some_and(|screen| screen.contains("会议纪要"));
            agent && card
        })
    });

    let terminal = cx.update(|cx| {
        workspace
            .read(cx)
            .agent_panel()
            .expect("the agent is open")
            .terminal
            .clone()
    });
    in_frame(cx, handle, |window, cx| {
        terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
    });
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("给 Agent 的粘贴".into())));
    dispatch(cx, handle, PasteTerminal);

    let agent_screen = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            workspace
                .read(cx)
                .agent_panel()
                .map(|panel| panel.terminal.read(cx).screen_text(cx))
                .unwrap_or_default()
        })
    };
    wait(cx, handle, |cx| {
        agent_screen(cx).contains("给 Agent 的粘贴")
    });
    let card = cx.update(|cx| workspace.read(cx).card_screen_text(CardId(1), cx));
    let card = card.unwrap_or_default();
    assert!(!card.contains("给 Agent 的粘贴"), "粘到卡片里去了：{card}");
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
