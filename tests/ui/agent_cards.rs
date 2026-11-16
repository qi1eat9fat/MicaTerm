//! 交给 Agent 的卡片（Agent CLI 工单 06）：右键「交给 Agent」后 `cards list` 列出它、状态栏标出来，
//! `cards read` 读得到屏幕、`cards send-key` 送得进键；「收回」后它的 CLI 卡片 ID 作废，
//! `cards read`、`exec --card` 一律 `no_terminal`。外部 CLI 在临时端点上真的听，远程终端是假的。
//! 本地卡片的身份环境变量（工单 07）：看假 provider 记下的启动参数。
//! 交给右侧面板里的 Agent（工单 07）：没在跑时带初始提示启动，已在跑时说明填进它的输入框、不发送。

use std::path::{Path, PathBuf};
use std::time::Duration;

use micaterm::app::{ConnectHost, HandCardToAgent, NewLocalTerminalTab, TakeCardBack};
use micaterm::cli::{CliService, Console, Request, send_request};
use micaterm_terminal::TerminalLifecycle;

use crate::settings_cli::EchoBackend;
use crate::support::*;

/// 只给这个测试用的端点（Unix 上的套接字放在 `dir` 里）。
fn endpoint(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(
            r"\\.\pipe\micaterm-cli-cards-test-{}-{name}",
            std::process::id()
        ))
    } else {
        dir.path().join("cli.sock")
    }
}

/// 打开窗口（种子主机、假远程终端），外部 CLI 在 `endpoint` 上听、已启用；连上主机 1，它的卡片是 1。
fn open(
    cx: &mut TestAppContext,
    endpoint: &Path,
    factory: FakeTerminalFactory,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeRemoteTerminals>,
) {
    let (handle, workspace, remote, _) = open_with_local(
        cx,
        endpoint,
        factory,
        Arc::new(FakeTerminalFactory::default()),
    );
    (handle, workspace, remote)
}

/// 同 [`open`]，本地终端（Agent 面板里的 Agent）用 `local` 的假传输，provider 交回来看启动参数。
fn open_with_local(
    cx: &mut TestAppContext,
    endpoint: &Path,
    factory: FakeTerminalFactory,
    local: Arc<FakeTerminalFactory>,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeRemoteTerminals>,
    Arc<FixedLocalTerminalTransportProvider>,
) {
    let remote = Arc::new(FakeRemoteTerminals::new(factory));
    let provider = fake_local_terminals_with(local);
    let services = WorkspaceServices {
        local_terminals: provider.clone(),
        hosts: HostData::seed(),
        remote_terminals: remote.clone(),
        connection_tester: Arc::new(FakeConnectionTester::succeeding()),
        cli: CliService::start_at(endpoint.to_path_buf(), Arc::new(EchoBackend)),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    cx.update(|cx| {
        let settings = workspace.read(cx).settings().clone();
        // Agent 命令写死，不看这台机器的 PATH 里有没有 claude（工单 07）；本地终端是假的，不会真的启动。
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

    dispatch(cx, handle, ConnectHost(HostId(1)));
    let started = std::time::Instant::now();
    while lifecycle(cx, &workspace) != Some(TerminalLifecycle::Running) {
        assert!(started.elapsed() < Duration::from_secs(3), "卡片没有连上");
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, handle, |_, _| {});
        std::thread::sleep(Duration::from_millis(5));
    }
    (handle, workspace, remote, provider)
}

fn lifecycle(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<TerminalLifecycle> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .card(CardId(1), cx)
            .map(|card| card.read(cx).terminal().read(cx).lifecycle(cx))
    })
}

/// 在另一条线程上发请求，这边推着时钟等它回来：退出码和 stdout（JSON）加 stderr。
fn ask(cx: &mut TestAppContext, endpoint: &Path, request: Request) -> (i32, String) {
    let endpoint = endpoint.to_path_buf();
    let thread = std::thread::spawn(move || {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = send_request(
            &endpoint,
            request,
            &mut Console {
                stdout: &mut stdout,
                stderr: &mut stderr,
                json: true,
                exec_json: false,
                stderr_is_terminal: false,
            },
        );
        let mut out = String::from_utf8(stdout).unwrap();
        out.push_str(&String::from_utf8(stderr).unwrap());
        (code, out)
    });
    for _ in 0..200 {
        if thread.is_finished() {
            break;
        }
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    thread.join().unwrap()
}

/// `cards list` 的结果。
fn cards(cx: &mut TestAppContext, endpoint: &Path) -> Vec<serde_json::Value> {
    let (code, out) = ask(cx, endpoint, Request::ListCards);
    assert_eq!(code, 0, "{out}");
    serde_json::from_str(&out).unwrap()
}

fn status(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> String {
    in_frame(cx, handle, |window, _| {
        window
            .try_find(("card-status", 1usize))
            .and_then(|element| element.label().map(str::to_owned))
    })
    .unwrap_or_default()
}

fn read(card: &str) -> Request {
    Request::ReadCard {
        card: card.into(),
        lines: None,
    }
}

#[gpui_kit::test]
fn a_card_handed_to_the_agent_is_listed_read_and_typed_into_until_taken_back(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir, "handed");
    let (handle, workspace, remote) =
        open(cx, &endpoint, FakeTerminalFactory::printing("hello agent"));

    // 没交出去：不列出，状态栏也没有标记。
    assert!(cards(cx, &endpoint).is_empty());
    assert!(!status(cx, handle).starts_with("Agent"));

    dispatch(cx, handle, HandCardToAgent(CardId(1)));
    assert!(
        status(cx, handle).starts_with("Agent · 已连接"),
        "{}",
        status(cx, handle)
    );
    let listed = cards(cx, &endpoint);
    assert_eq!(listed.len(), 1, "{listed:?}");
    let (host_id, host_name) = cx.update(|cx| {
        let host = workspace
            .read(cx)
            .hosts()
            .read(cx)
            .host(HostId(1))
            .unwrap()
            .clone();
        (host.public_id.to_string(), host.name.to_string())
    });
    assert_eq!(listed[0]["host"], host_id.as_str());
    assert_eq!(listed[0]["host_name"], host_name.as_str());
    assert_eq!(listed[0]["connected"], true);
    assert_eq!(listed[0]["running"], false);
    let card = listed[0]["id"].as_str().unwrap().to_string();
    assert_eq!(card.len(), 16, "{card}");

    // 读屏幕：假 shell 打印的字在里面。
    let (code, out) = ask(cx, &endpoint, read(&card));
    assert_eq!(code, 0, "{out}");
    let screen: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        screen["text"].as_str().unwrap().contains("hello agent"),
        "{screen}"
    );
    assert_eq!(screen["alternate_screen"], false);

    // 按键：q 原样送进 shell；不认识的键不送。
    let before = remote.factory.written_text().len();
    let (code, out) = ask(
        cx,
        &endpoint,
        Request::SendKey {
            card: card.clone(),
            key: "q".into(),
            repeat: Some(3),
        },
    );
    assert_eq!(code, 0, "{out}");
    let written = remote.factory.written_text();
    assert_eq!(&written[before..], "qqq");
    let (code, out) = ask(
        cx,
        &endpoint,
        Request::SendKey {
            card: card.clone(),
            key: "ctrl-alt-delete".into(),
            repeat: None,
        },
    );
    assert_eq!(code, micaterm::cli::FAILURE_EXIT);
    assert!(out.contains("bad_request"), "{out}");
    assert_eq!(remote.factory.written_text(), written);

    // 再交一次照旧用原来的 ID。
    dispatch(cx, handle, HandCardToAgent(CardId(1)));
    assert_eq!(cards(cx, &endpoint)[0]["id"], card.as_str());

    // 收回：不再列出，原来的 ID 读不了、也打不进命令。
    dispatch(cx, handle, TakeCardBack(CardId(1)));
    assert!(!status(cx, handle).starts_with("Agent"));
    assert!(cards(cx, &endpoint).is_empty());
    let (code, out) = ask(cx, &endpoint, read(&card));
    assert_eq!(code, micaterm::cli::FAILURE_EXIT);
    assert!(out.contains("no_terminal"), "{out}");
    let (code, out) = ask(
        cx,
        &endpoint,
        Request::ExecInCard {
            card: card.clone(),
            command: "true".into(),
        },
    );
    assert_eq!(code, micaterm::cli::FAILURE_EXIT);
    assert!(out.contains("no_terminal"), "{out}");
    let (code, out) = ask(
        cx,
        &endpoint,
        Request::SendKey {
            card: card.clone(),
            key: "q".into(),
            repeat: None,
        },
    );
    assert_eq!(code, micaterm::cli::FAILURE_EXIT);
    assert!(out.contains("no_terminal"), "{out}");
    assert_eq!(remote.factory.written_text(), written, "什么也没打进去");

    // 再交出去是新的 ID。
    dispatch(cx, handle, HandCardToAgent(CardId(1)));
    let again = cards(cx, &endpoint);
    assert_ne!(again[0]["id"], card.as_str());
}

/// 右键菜单：没交出去是「交给 Agent」，交出去了换成「收回」，点了就交出、收回。
#[gpui_kit::test]
fn the_card_menu_hands_the_card_over_and_takes_it_back(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir, "menu");
    let (handle, _workspace, _remote) = open(cx, &endpoint, FakeTerminalFactory::default());
    let labels = terminal_menu_labels(cx, handle, 1);
    assert!(labels.contains(&"交给 Agent".to_string()), "{labels:?}");
    assert!(!labels.contains(&"收回".to_string()), "{labels:?}");

    click_terminal_menu(cx, handle, 1, "交给 Agent");
    assert_eq!(cards(cx, &endpoint).len(), 1);
    let labels = terminal_menu_labels(cx, handle, 1);
    assert!(labels.contains(&"收回".to_string()), "{labels:?}");
    assert!(!labels.contains(&"交给 Agent".to_string()), "{labels:?}");

    click_terminal_menu(cx, handle, 1, "收回");
    assert!(cards(cx, &endpoint).is_empty());
}

fn env_value<'a>(launch: &'a micaterm_terminal::LocalLaunch, name: &str) -> Option<&'a str> {
    launch
        .env
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// 每张本地卡片都带身份环境变量（工单 07）：自己的 CLI 卡片 ID 各不相同、和卡片记着的一样；
/// 装好的 `micaterm` 给完整路径，它的目录放到 `PATH` 最前面。
#[gpui_kit::test]
fn every_local_card_carries_its_identity_and_where_the_command_is(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let command = if cfg!(windows) {
        "micaterm.exe"
    } else {
        "micaterm"
    };
    let bin = root.path().join("app").join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join(command), "cli").unwrap();
    std::fs::write(root.path().join("app").join("micaterm-cli.exe"), "cli").unwrap();
    let provider = fake_local_terminals();
    let services = WorkspaceServices {
        cli_install: Some(micaterm::cli::IntegrationPaths {
            home: root.path().join("home"),
            bin_link: bin.join(command),
            exe: root.path().join("app").join("micaterm-cli.exe"),
            user_path: None,
        }),
        ..services_with_terminals(provider.clone())
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    let before = provider.launches().len();
    dispatch(cx, handle, NewLocalTerminalTab);
    dispatch(cx, handle, NewLocalTerminalTab);
    let launches = provider.launches()[before..].to_vec();
    assert_eq!(launches.len(), 2);

    let own_ids: Vec<String> = cx.update(|cx| {
        let workspace = workspace.read(cx);
        workspace
            .tab_ids()
            .into_iter()
            .flat_map(|tab| workspace.tab_cards(tab))
            .filter_map(|card| card.read(cx).self_id().map(|id| id.to_string()))
            .collect()
    });
    for launch in &launches {
        assert_eq!(env_value(launch, "TERM_PROGRAM"), Some("micaterm"));
        assert_eq!(
            env_value(launch, "TERM_PROGRAM_VERSION"),
            Some(env!("CARGO_PKG_VERSION"))
        );
        let own = env_value(launch, "MICATERM_SELF_CARD").unwrap();
        assert_eq!(own.len(), 16, "{own}");
        assert!(own_ids.iter().any(|id| id == own), "{own} {own_ids:?}");
        assert_eq!(
            env_value(launch, "MICATERM_CLI"),
            Some(bin.join(command).display().to_string().as_str())
        );
        assert_eq!(launch.path_front.as_deref(), Some(bin.as_path()));
        // 只在 Agent 卡片里有的两个，普通本地卡片没有。
        assert_eq!(env_value(launch, "MICATERM_CARD"), None);
        assert_eq!(env_value(launch, "MICATERM_HOST"), None);
    }
    assert_ne!(
        env_value(&launches[0], "MICATERM_SELF_CARD"),
        env_value(&launches[1], "MICATERM_SELF_CARD")
    );
}

/// 远程卡片一个身份变量都不给：连主机不经本地终端，卡片也没有自己的 CLI 卡片 ID。
#[gpui_kit::test]
fn a_remote_card_gets_no_identity(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir, "remote-identity");
    let (_handle, workspace, _remote) = open(cx, &endpoint, FakeTerminalFactory::default());
    let card = cx.update(|cx| workspace.read(cx).card(CardId(1), cx).unwrap());
    cx.update(|cx| {
        assert!(card.read(cx).remote_host().is_some());
        assert_eq!(card.read(cx).self_id(), None);
    });
}

/// 交给 Agent（工单 07）：面板里的 Agent 没在跑时带着初始提示启动（卡片 ID、主机 ID、两条用法），
/// 环境里有 `MICATERM_CARD` / `MICATERM_HOST`；已在跑时同样的说明填进它的输入框、不按回车，面板上一行提示。
#[gpui_kit::test]
fn handing_a_card_starts_the_agent_with_a_prompt_or_fills_in_its_input(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir, "give");
    let local = Arc::new(FakeTerminalFactory::default());
    let (handle, workspace, _remote, provider) =
        open_with_local(cx, &endpoint, FakeTerminalFactory::default(), local.clone());
    let before = provider.launches().len();

    dispatch(cx, handle, HandCardToAgent(CardId(1)));
    let card = cards(cx, &endpoint)[0]["id"].as_str().unwrap().to_string();
    let host = cx.update(|cx| {
        workspace
            .read(cx)
            .hosts()
            .read(cx)
            .host(HostId(1))
            .unwrap()
            .public_id
            .to_string()
    });
    let launches = provider.launches()[before..].to_vec();
    assert_eq!(launches.len(), 1, "开了 Agent");
    let agent = &launches[0];
    assert_eq!(agent.shell.command(), Some("claude"));
    assert_eq!(agent.args.len(), 1);
    assert!(
        agent.args[0].contains(&format!("micaterm exec --card {card}")),
        "{}",
        agent.args[0]
    );
    assert!(
        agent.args[0].contains(&format!("micaterm cards read {card}")),
        "{}",
        agent.args[0]
    );
    assert_eq!(env_value(agent, "MICATERM_CARD"), Some(card.as_str()));
    assert_eq!(env_value(agent, "MICATERM_HOST"), Some(host.as_str()));
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        assert_eq!(
            workspace.right_panel(),
            Some(micaterm::workspace::RightPanel::Agent)
        );
        let panel = workspace.agent_panel().unwrap();
        assert_eq!(
            env_value(agent, "MICATERM_SELF_CARD"),
            Some(panel.self_id.as_ref())
        );
        assert_eq!(panel.note, None);
    });

    // 等 Agent 的终端起来。
    let started = std::time::Instant::now();
    while !cx.update(|cx| {
        workspace
            .read(cx)
            .agent_panel()
            .unwrap()
            .terminal
            .read(cx)
            .lifecycle(cx)
            == TerminalLifecycle::Running
    }) {
        assert!(started.elapsed() < Duration::from_secs(3), "Agent 没有起来");
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, handle, |_, _| {});
        std::thread::sleep(Duration::from_millis(5));
    }

    // 收回再交：Agent 已在跑，不另开；说明填进它的输入框，没有回车。
    dispatch(cx, handle, TakeCardBack(CardId(1)));
    let typed_before = local.written_text().len();
    dispatch(cx, handle, HandCardToAgent(CardId(1)));
    let again = cards(cx, &endpoint)[0]["id"].as_str().unwrap().to_string();
    assert_ne!(again, card);
    assert_eq!(provider.launches().len(), before + 1, "没有另开");
    let typed = local.written_text()[typed_before..].to_string();
    assert!(
        typed.contains(&format!("micaterm cards read {again}")),
        "{typed}"
    );
    assert!(!typed.contains('\r') && !typed.contains('\n'), "{typed:?}");
    let note = cx.update(|cx| workspace.read(cx).agent_panel().unwrap().note.clone());
    assert!(note.is_some_and(|note| note.contains("web-01")));
}
