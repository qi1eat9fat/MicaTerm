//! Agent 的状态（Agent CLI 工单 09）：状态 hook 经外部 Agent 的端点报来，面板收起时图标栏亮点、展开时
//! 标题行写状态；前台是 claude 的本地卡片靠屏幕规则认出「等你确认」，看不见时亮橙点；
//! 设置 › Agent 的「Agent 状态」开关往临时用户目录里的 Claude Code 配置装 hook。
//! 外部 Agent 在临时端点上真的听，终端都是假的。

use std::path::{Path, PathBuf};
use std::time::Duration;

use micaterm::agent::{AgentState, HookEvent};
use micaterm::app::{NewLocalTerminalTab, OpenSettings, ShowAgent};
use micaterm::cli::{CliService, Console, Request, send_request};

use crate::settings_cli::{EchoBackend, install_paths};
use crate::support::*;

fn endpoint(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(
            r"\\.\pipe\micaterm-cli-state-test-{}-{name}",
            std::process::id()
        ))
    } else {
        dir.path().join("cli.sock")
    }
}

/// 打开窗口：外部 Agent 在 `endpoint` 上听、已启用，Agent 命令是 claude（终端是假的）。
fn open(
    cx: &mut TestAppContext,
    endpoint: Option<&Path>,
    provider: Arc<FixedLocalTerminalTransportProvider>,
    install: Option<micaterm::cli::IntegrationPaths>,
) -> (WindowHandle<Root>, Entity<Workspace>) {
    let services = WorkspaceServices {
        cli: endpoint
            .map(|endpoint| CliService::start_at(endpoint.to_path_buf(), Arc::new(EchoBackend)))
            .unwrap_or(CliService::Off),
        cli_install: install,
        ..services_with_terminals(provider)
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
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
    (handle, workspace)
}

/// 推着时钟等 `done`。
fn wait(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) {
    let started = std::time::Instant::now();
    while !done(cx) {
        assert!(started.elapsed() < Duration::from_secs(5), "等太久了");
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        in_frame(cx, handle, |_, _| {});
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 像 `micaterm hook` 那样报一件事，推着时钟等应用取走。
fn hook(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    endpoint: &Path,
    card: &str,
    event: HookEvent,
    message: Option<&str>,
) {
    let endpoint = endpoint.to_path_buf();
    let request = Request::Hook {
        card: card.to_string(),
        event,
        message: message.map(str::to_string),
    };
    let sent = std::thread::spawn(move || {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        send_request(
            &endpoint,
            request,
            &mut Console {
                stdout: &mut stdout,
                stderr: &mut stderr,
                json: true,
                exec_json: false,
                stderr_is_terminal: false,
            },
        )
    });
    while !sent.is_finished() {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(sent.join().unwrap(), 0);
    // 窗口壳每 300 毫秒取一次。
    for _ in 0..5 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
    in_frame(cx, handle, |_, _| {});
}

fn label(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &'static str) -> Option<String> {
    in_frame(cx, handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.label().map(str::to_owned))
    })
}

#[gpui_kit::test]
fn hooks_light_the_rail_while_the_panel_is_closed_and_name_the_state_when_open(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir, "panel");
    let (handle, workspace) = open(cx, Some(&endpoint), fake_local_terminals(), None);
    dispatch(cx, handle, ShowAgent);
    start_new_agent(cx, handle);
    let me = cx.update(|cx| {
        workspace
            .read(cx)
            .agent_panel()
            .unwrap()
            .self_id
            .to_string()
    });
    assert_eq!(label(cx, handle, "agent-panel-state"), None, "空闲不写");
    dispatch(cx, handle, ShowAgent);

    // 收起着：等你确认是橙点（悬停提示说状态）。
    hook(
        cx,
        handle,
        &endpoint,
        &me,
        HookEvent::NeedsAttention,
        Some("Claude needs your permission to use Bash"),
    );
    assert_eq!(
        label(cx, handle, "right-sidebar-agent-dot").as_deref(),
        Some("Agent · 等你确认")
    );

    // 展开：没有点，标题行写状态。
    dispatch(cx, handle, ShowAgent);
    assert_eq!(label(cx, handle, "right-sidebar-agent-dot"), None);
    assert_eq!(
        label(cx, handle, "agent-panel-state").as_deref(),
        Some("等你确认")
    );

    // 展开着回合结束：看见了，收起后不亮点。
    hook(cx, handle, &endpoint, &me, HookEvent::TurnDone, None);
    assert_eq!(
        label(cx, handle, "agent-panel-state").as_deref(),
        Some("回合结束")
    );
    dispatch(cx, handle, ShowAgent);
    assert_eq!(label(cx, handle, "right-sidebar-agent-dot"), None);

    // 收起着运行：绿点。
    hook(cx, handle, &endpoint, &me, HookEvent::PromptSubmit, None);
    assert_eq!(
        label(cx, handle, "right-sidebar-agent-dot").as_deref(),
        Some("Agent · 运行中")
    );
    // 别的卡片 ID 报来的不算。
    hook(
        cx,
        handle,
        &endpoint,
        "NotMineNotMine00",
        HookEvent::NeedsAttention,
        None,
    );
    assert_eq!(
        label(cx, handle, "right-sidebar-agent-dot").as_deref(),
        Some("Agent · 运行中")
    );
}

const CLAUDE_ASKING: &str = "\
● Bash(rm -rf build)\r\n\
\r\n\
────────────────────────────────────────────\r\n\
 Bash command\r\n\
\r\n\
   rm -rf build\r\n\
\r\n\
 Do you want to proceed?\r\n\
 ❯ 1. Yes\r\n\
   2. No, and tell Claude what to do differently\r\n\
\r\n\
 Esc to cancel · Tab to amend";

/// 本地卡片里自己敲 claude：没装 hook 也能靠屏幕认出等你确认；卡片在后台标签里就亮橙点。
#[gpui_kit::test]
fn a_local_card_running_claude_is_read_from_its_screen(cx: &mut TestAppContext) {
    let factory = Arc::new(FakeTerminalFactory::default());
    let (handle, workspace) = open(cx, None, fake_local_terminals_with(factory.clone()), None);
    dispatch(cx, handle, NewLocalTerminalTab);
    factory.foreground(1, Some("claude"));
    factory.print(1, CLAUDE_ASKING);
    // 再开一个标签：第一张卡片看不见了。
    dispatch(cx, handle, NewLocalTerminalTab);
    let state = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let card = workspace.read(cx).card(CardId(1), cx).unwrap();
            let card = card.read(cx);
            (card.agent_tracker().state(), card.wants_attention())
        })
    };
    wait(cx, handle, |cx| state(cx).0 == AgentState::Waiting);
    assert!(state(cx).1, "看不见的卡片在等你：亮橙点");

    // 回到 shell：回到空闲。
    factory.foreground(1, None);
    wait(cx, handle, |cx| state(cx).0 == AgentState::Idle);
}

/// 设置 › Agent 的「Agent 状态」：开 Claude Code 的开关，往它的 settings.json 里装 hook；再关就删掉。
#[gpui_kit::test]
fn the_settings_switch_installs_the_claude_hooks(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let paths = install_paths(root.path());
    let file = paths.home.join(".claude").join("settings.json");
    let (handle, _workspace) = open(cx, None, fake_local_terminals(), Some(paths));
    dispatch(cx, handle, OpenSettings);
    in_frame(cx, handle, |window, cx| {
        window.click(("settings-category", 4usize), cx)
    });
    wait(cx, handle, |cx| {
        in_frame(cx, handle, |window, _| {
            window.try_find("cli-hooks-claude").is_some()
                && window.find("cli-hooks-claude").checked() == Some(false)
        })
    });
    // 「Agent 状态」在这一页靠下，测试窗口里要往下滚才看得见。
    in_frame(cx, handle, |window, cx| {
        use gpui_kit::InputEvent as _;
        let position = window.find("agent-command").bounds().center();
        window.dispatch_event(
            gpui_kit::ScrollEvent {
                position,
                delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-2000.))),
                modifiers: gpui_kit::Modifiers::default(),
                touch_phase: gpui_kit::TouchPhase::Moved,
                source: gpui_kit::ScrollSource::Pointer,
            }
            .to_platform_input(),
            cx,
        );
    });
    in_frame(cx, handle, |window, cx| {
        window.click("cli-hooks-claude", cx)
    });
    wait(cx, handle, |cx| {
        in_frame(cx, handle, |window, _| {
            window.find("cli-hooks-claude").checked() == Some(true)
        })
    });
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains(" hook claude"), "{text}");

    in_frame(cx, handle, |window, cx| {
        window.click("cli-hooks-claude", cx)
    });
    wait(cx, handle, |cx| {
        in_frame(cx, handle, |window, _| {
            window.find("cli-hooks-claude").checked() == Some(false)
        })
    });
    assert!(
        !std::fs::read_to_string(&file)
            .unwrap()
            .contains(" hook claude")
    );
}
