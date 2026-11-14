//! 密码登录远程卡片（工单 12）的 UI 测试：连接（双击、回车、右键菜单）、主机密钥确认、询问密码和验证码、
//! 状态栏、断开后原地重连、远程终端右键菜单、编辑或删除主机不关卡片、主机对话框的「测试连接」。
//! 远程终端是假的（`FakeRemoteTerminals`），连接表的共享、真的登录在 backend 的测试里。

use std::time::Duration;

use micaterm::app::{ConnectHost, CopyCardAddress, DisconnectCard, EditHost, ReconnectCard};
use micaterm::backend::{AuthenticationRequest, ConnectionPromptField, ConnectionPromptKind};
use micaterm::backend::{ConnectionId, Latency, ProbedHost, RemoteEvent, RemoteTerminalId};
use micaterm_hosts::SecretRef;
use micaterm_terminal::TerminalLifecycle;

use crate::support::*;

fn open_remote(
    cx: &mut TestAppContext,
    factory: FakeTerminalFactory,
    tester: FakeConnectionTester,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeRemoteTerminals>,
    Arc<FakeConnectionTester>,
) {
    let remote = Arc::new(FakeRemoteTerminals::new(factory));
    let tester = Arc::new(tester);
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        remote_terminals: remote.clone(),
        connection_tester: tester.clone(),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    (handle, workspace, remote, tester)
}

/// 假 shell 跑在真线程上：按真实时间等，同时拨测试时钟（终端每 16ms 收一次事件）。
fn wait_until(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, handle, |_, _| {});
        if done(cx) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn lifecycle(
    cx: &mut TestAppContext,
    workspace: &Entity<Workspace>,
    card: u64,
) -> Option<TerminalLifecycle> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .card(CardId(card), cx)
            .map(|card| card.read(cx).terminal().read(cx).lifecycle(cx))
    })
}

fn wait_running(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    workspace: &Entity<Workspace>,
    card: u64,
) {
    assert!(
        wait_until(cx, handle, |cx| lifecycle(cx, workspace, card)
            == Some(TerminalLifecycle::Running)),
        "卡片 {card} 没有连上"
    );
}

fn label(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    id: impl Into<gpui_kit::ElementId>,
) -> Option<String> {
    let id = id.into();
    in_frame(cx, handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.label().map(str::to_owned))
    })
}

fn status(cx: &mut TestAppContext, handle: WindowHandle<Root>, card: u64) -> String {
    // 状态栏从连接状态读起（名称在卡片顶行）。
    label(cx, handle, ("card-status", card as usize)).unwrap_or_default()
}

fn card_names(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Vec<Vec<String>> {
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        workspace
            .tab_ids()
            .into_iter()
            .map(|tab| {
                workspace
                    .tab_cards(tab)
                    .iter()
                    .map(|card| card.read(cx).name().to_string())
                    .collect()
            })
            .collect()
    })
}

/// 连接主机 `host`（像双击那样），等它的卡片 `card` 连上。
fn connect(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    workspace: &Entity<Workspace>,
    host: u64,
    card: u64,
) {
    dispatch(cx, handle, ConnectHost(HostId(host)));
    wait_running(cx, handle, workspace, card);
}

#[gpui_kit::test]
fn double_clicking_a_host_opens_a_new_tab_with_only_its_remote_card(cx: &mut TestAppContext) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );

    in_frame(cx, handle, |window, cx| {
        window.double_click(host_row(1), cx)
    });
    wait_running(cx, handle, &workspace, 1);

    assert_eq!(card_names(cx, &workspace), [["web-01"]]);
    let targets = remote.targets();
    assert_eq!(targets.len(), 1);
    let (terminal, target) = &targets[0];
    assert_eq!(*terminal, RemoteTerminalId(1));
    assert_eq!(target.host, 1);
    assert_eq!(
        (
            target.login.host.as_str(),
            target.login.port,
            target.login.user.as_str()
        ),
        ("10.0.1.12", 22, "root")
    );
    // 主机自己的密码按主机 id 从保险库读；没存过时连接现场询问。
    assert_eq!(target.login.password, Some(SecretRef::password(1)));
    assert!(
        status(cx, handle, 1).starts_with("已连接 · ~ · "),
        "{}",
        status(cx, handle, 1)
    );
}

#[gpui_kit::test]
fn enter_on_the_selected_host_and_the_menus_connect_item_connect_too(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );

    in_frame(cx, handle, |window, cx| window.click(host_row(2), cx));
    in_frame(cx, handle, |window, cx| window.press("enter", cx));
    wait_running(cx, handle, &workspace, 1);

    in_frame(cx, handle, |window, cx| window.right_click(host_row(3), cx));
    in_frame(cx, handle, |window, cx| {
        let mut popup = window.within("popup-menu");
        assert_eq!(popup.find(0usize).label(), Some("连接"));
        popup.click(0usize, cx);
    });
    wait_running(cx, handle, &workspace, 2);

    assert_eq!(card_names(cx, &workspace), [["web-02"], ["db-01"]]);
}

#[gpui_kit::test]
fn connecting_a_connected_host_again_opens_another_card_on_the_same_connection_target(
    cx: &mut TestAppContext,
) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );

    connect(cx, handle, &workspace, 1, 1);
    connect(cx, handle, &workspace, 1, 2);

    assert_eq!(card_names(cx, &workspace), [["web-01"], ["web-01"]]);
    // 登录一样：连接表据此复用连接、新开 shell 通道，不重新登录（backend 的测试验证共享）。
    let targets = remote.targets();
    assert_eq!(targets[0].1, targets[1].1);
}

#[gpui_kit::test]
fn the_status_bar_shows_latency_and_the_probed_system_which_becomes_the_badge(
    cx: &mut TestAppContext,
) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );
    connect(cx, handle, &workspace, 1, 1);
    // 还没探测出系统：图块是名称的首字。
    assert_eq!(
        cx.update(|cx| workspace.read(cx).card_tile(CardId(1), cx)),
        Some("W".into())
    );

    let terminal = RemoteTerminalId(1);
    remote.emit(RemoteEvent::Attached {
        terminal,
        connection: ConnectionId(1),
    });
    remote.emit(RemoteEvent::Latency {
        terminal,
        latency: Latency::Measured(Duration::from_millis(12)),
    });
    remote.emit(RemoteEvent::Probed {
        terminal,
        host: ProbedHost {
            os: "ubuntu",
            hostname: Some("web-01".into()),
        },
    });
    cx.run_until_parked();

    assert!(
        status(cx, handle, 1).starts_with("已连接 12 ms · ~ · Ubuntu · "),
        "{}",
        status(cx, handle, 1)
    );
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        assert_eq!(workspace.card_tile(CardId(1), cx), Some("Ubuntu".into()));
        assert_eq!(workspace.tab_tile(TabId(1), cx).as_deref(), Some("Ubuntu"));
        assert_eq!(
            workspace.card(CardId(1), cx).unwrap().read(cx).connection(),
            Some(ConnectionId(1))
        );
    });
    // 探测出的系统记进主机：主机栏的徽章跟着变。
    assert_eq!(
        saved_host(cx, &workspace, 1).unwrap().os,
        Some(HostOs::Ubuntu)
    );
}

fn prompt_label(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> Option<String> {
    label(cx, handle, "connection-prompt")
}

#[gpui_kit::test]
fn the_first_connection_asks_to_trust_the_host_key(cx: &mut TestAppContext) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );
    connect(cx, handle, &workspace, 1, 1);

    let prompt = remote.ask(
        1,
        ConnectionPromptKind::unknown_host("10.0.1.12", 22, "ssh-ed25519", "SHA256:abc"),
    );
    cx.run_until_parked();

    let text = prompt_label(cx, handle).expect("确认框打开了");
    assert!(
        text.starts_with("确认主机密钥\n第一次连接这台主机"),
        "{text}"
    );
    assert!(text.contains("ssh-ed25519 SHA256:abc"), "{text}");
    in_frame(cx, handle, |window, cx| window.click("prompt-confirm", cx));

    assert_eq!(remote.replies(), [(prompt, Answered::Trust)]);
    assert_eq!(prompt_label(cx, handle), None);
}

#[gpui_kit::test]
fn a_changed_host_key_warns_and_escape_cancels(cx: &mut TestAppContext) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );
    connect(cx, handle, &workspace, 1, 1);

    let prompt = remote.ask(
        1,
        ConnectionPromptKind::host_key_changed(
            "10.0.1.12",
            22,
            "ssh-ed25519",
            vec!["SHA256:old".into()],
            "SHA256:new",
        ),
    );
    cx.run_until_parked();

    let text = prompt_label(cx, handle).expect("告警框打开了");
    assert!(text.starts_with("主机密钥变了"), "{text}");
    assert!(text.contains("记下的：SHA256:old"), "{text}");
    assert!(text.contains("现在的：SHA256:new"), "{text}");
    in_frame(cx, handle, |window, cx| window.press("escape", cx));

    assert_eq!(remote.replies(), [(prompt, Answered::Cancel)]);
    assert_eq!(prompt_label(cx, handle), None);
}

#[gpui_kit::test]
fn an_empty_password_is_asked_for_and_so_are_the_servers_other_questions(cx: &mut TestAppContext) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );
    connect(cx, handle, &workspace, 1, 1);

    let password = remote.ask(
        1,
        ConnectionPromptKind::authentication(AuthenticationRequest::Password {
            saved_rejected: false,
        }),
    );
    // 第二个问题排在后面，等第一个答完再问。
    let code = remote.ask(
        1,
        ConnectionPromptKind::authentication(AuthenticationRequest::KeyboardInteractive {
            name: String::new(),
            instructions: String::new(),
            fields: vec![ConnectionPromptField::new("Verification code: ", true)],
            saved_password_rejected: false,
        }),
    );
    cx.run_until_parked();

    let text = prompt_label(cx, handle).expect("密码框打开了");
    assert!(text.starts_with("输入密码\n请输入登录密码\n密码"), "{text}");
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("prompt-field", 0usize)).focused(), Some(true));
    });
    in_frame(cx, handle, |window, cx| window.input("hunter2", cx));
    in_frame(cx, handle, |window, cx| window.press("enter", cx));

    let text = prompt_label(cx, handle).expect("验证码框接着打开");
    assert!(
        text.starts_with("输入验证码\n服务器要求键盘交互认证（密码已自动回答）："),
        "{text}"
    );
    in_frame(cx, handle, |window, cx| window.input("4821", cx));
    in_frame(cx, handle, |window, cx| window.click("prompt-confirm", cx));

    assert_eq!(
        remote.replies(),
        [
            (password, Answered::Answers(vec!["hunter2".into()])),
            (code, Answered::Answers(vec!["4821".into()])),
        ]
    );
    assert_eq!(prompt_label(cx, handle), None);
}

#[gpui_kit::test]
fn a_question_nobody_waits_for_any_more_is_not_asked(cx: &mut TestAppContext) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );
    connect(cx, handle, &workspace, 1, 1);
    dispatch(cx, handle, CloseCard(CardId(1)));

    let prompt = remote.ask(
        1,
        ConnectionPromptKind::authentication(AuthenticationRequest::Password {
            saved_rejected: false,
        }),
    );
    cx.run_until_parked();

    assert_eq!(prompt_label(cx, handle), None);
    assert_eq!(remote.replies(), [(prompt, Answered::Cancel)]);
}

/// 问着密码时把卡片关了：连接表说问题作废，问答框跟着关，排着的下一个问题照常问
/// （M2 code review S3）。
#[gpui_kit::test]
fn a_withdrawn_question_closes_its_dialog_and_the_next_one_is_asked(cx: &mut TestAppContext) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );
    connect(cx, handle, &workspace, 1, 1);
    connect(cx, handle, &workspace, 2, 2);
    let password = || {
        ConnectionPromptKind::authentication(AuthenticationRequest::Password {
            saved_rejected: false,
        })
    };
    let first = remote.ask(1, password());
    let second = remote.ask(2, password());
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(workspace.read(cx).shown_prompt(), Some(first)));

    // 等第一条连接的卡片都关了（连接表的这一半在 backend 的测试里）。
    remote.emit(RemoteEvent::PromptsWithdrawn {
        connection: first.connection,
    });
    cx.run_until_parked();

    cx.update(|cx| assert_eq!(workspace.read(cx).shown_prompt(), Some(second)));
    let text = prompt_label(cx, handle).expect("下一个问题接着问");
    assert!(text.starts_with("输入密码"), "{text}");
    // 作废的问题不再回答。
    assert_eq!(remote.replies(), []);
    in_frame(cx, handle, |window, cx| window.click("prompt-cancel", cx));
    assert_eq!(remote.replies(), [(second, Answered::Cancel)]);
    assert_eq!(prompt_label(cx, handle), None);
}

#[gpui_kit::test]
fn a_lost_connection_greys_the_card_and_enter_reconnects_in_place(cx: &mut TestAppContext) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::fail_first(),
        FakeConnectionTester::succeeding(),
    );
    dispatch(cx, handle, ConnectHost(HostId(1)));

    assert!(wait_until(cx, handle, |cx| matches!(
        lifecycle(cx, &workspace, 1),
        Some(TerminalLifecycle::Failed(_))
    )));
    assert_eq!(
        ended_notice(cx, &workspace, 1).as_deref(),
        Some("[连接已断开] 测试启动失败")
    );
    assert!(
        status(cx, handle, 1).starts_with("已断开 测试启动失败 · "),
        "{}",
        status(cx, handle, 1)
    );
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        assert_eq!(workspace.card_tile_off(CardId(1), cx), Some(true));
    });

    in_frame(cx, handle, |window, cx| window.press("enter", cx));
    wait_running(cx, handle, &workspace, 1);

    // 原地重连：还是这张卡片、这个标签，重新要了一次传输。
    assert_eq!(card_names(cx, &workspace), [["web-01"]]);
    assert_eq!(remote.targets().len(), 2);
    assert_eq!(ended_notice(cx, &workspace, 1), None);
    assert!(status(cx, handle, 1).starts_with("已连接"));
    cx.update(|cx| assert_eq!(workspace.read(cx).card_tile_off(CardId(1), cx), Some(false)));
}

#[gpui_kit::test]
fn space_on_a_disconnected_card_closes_it(cx: &mut TestAppContext) {
    let (handle, workspace, _, _) = open_remote(
        cx,
        FakeTerminalFactory::exit_first(),
        FakeConnectionTester::succeeding(),
    );
    dispatch(cx, handle, ConnectHost(HostId(1)));
    assert!(wait_until(cx, handle, |cx| matches!(
        lifecycle(cx, &workspace, 1),
        Some(TerminalLifecycle::Exited { .. })
    )));
    assert_eq!(
        ended_notice(cx, &workspace, 1).as_deref(),
        Some("[连接已断开] shell 已退出（退出码 9）")
    );

    in_frame(cx, handle, |window, cx| window.press("space", cx));

    assert!(cx.update(|cx| workspace.read(cx).tab_ids().is_empty()));
}

#[gpui_kit::test]
fn the_remote_terminal_menu_reconnects_disconnects_and_copies_the_ip(cx: &mut TestAppContext) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );
    connect(cx, handle, &workspace, 1, 1);

    in_frame(cx, handle, |window, cx| {
        window.right_click(("terminal", 1usize), cx)
    });
    let labels = in_frame(cx, handle, |window, _| {
        let popup = window.within("popup-menu");
        (0..16_usize)
            .filter_map(|item| popup.try_find(item))
            .filter_map(|item| item.label().map(str::to_owned))
            .collect::<Vec<_>>()
    });
    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    assert_eq!(labels[1], "复制 IP", "{labels:?}");
    // 最后两组：连接的操作，卡片的操作（卡片没有头部，放大、移到新标签、关闭在这里）。
    assert_eq!(
        labels[labels.len() - 5..],
        ["重新连接", "断开连接", "放大卡片", "移到新标签", "关闭卡片"],
        "{labels:?}"
    );

    dispatch(cx, handle, CopyCardAddress(CardId(1)));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("10.0.1.12")
    );

    dispatch(cx, handle, DisconnectCard(CardId(1)));
    assert_eq!(
        remote.factory.wait_for_shutdowns(1),
        1,
        "关掉了这张卡片的 shell 通道"
    );
    assert_eq!(
        ended_notice(cx, &workspace, 1).as_deref(),
        Some("[连接已断开] 已断开连接")
    );

    dispatch(cx, handle, ReconnectCard(CardId(1)));
    wait_running(cx, handle, &workspace, 1);
    assert_eq!(remote.targets().len(), 2);
}

#[gpui_kit::test]
fn editing_or_deleting_a_host_keeps_its_open_card(cx: &mut TestAppContext) {
    let (handle, workspace, remote, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );
    connect(cx, handle, &workspace, 1, 1);

    edit_hosts(cx, &workspace, |store, cx| {
        let draft = HostDraft::new("web-01b", "10.0.1.99", 22, "root", AuthKind::Password);
        store.update(HostId(1), draft, cx);
    });
    assert_eq!(
        card_names(cx, &workspace),
        [["web-01"]],
        "编辑不关、不改已开的卡片"
    );

    // 重连时按主机现在的样子连。
    dispatch(cx, handle, ReconnectCard(CardId(1)));
    wait_running(cx, handle, &workspace, 1);
    assert_eq!(remote.targets()[1].1.login.host, "10.0.1.99");
    assert_eq!(card_names(cx, &workspace), [["web-01b"]]);

    edit_hosts(cx, &workspace, |store, cx| {
        store.remove(HostId(1), cx);
    });
    assert_eq!(
        card_names(cx, &workspace),
        [["web-01b"]],
        "删除主机不关它的卡片"
    );

    // 主机删了：按卡片上次连接时的样子重连。
    dispatch(cx, handle, ReconnectCard(CardId(1)));
    wait_running(cx, handle, &workspace, 1);
    let targets = remote.targets();
    assert_eq!(targets[2].1, targets[1].1);
}

fn open_edit_dialog(cx: &mut TestAppContext, handle: WindowHandle<Root>, host: u64) {
    dispatch(cx, handle, EditHost(HostId(host)));
    in_frame(cx, handle, |window, cx| window.click("host-password", cx));
    in_frame(cx, handle, |window, cx| window.input("pw-typed", cx));
}

fn test_result(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> Option<String> {
    label(cx, handle, "host-test-result")
}

#[gpui_kit::test]
fn test_connection_logs_in_with_what_the_form_holds(cx: &mut TestAppContext) {
    let (handle, _, _, tester) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::succeeding(),
    );
    open_edit_dialog(cx, handle, 4);

    in_frame(cx, handle, |window, cx| {
        window.click("host-test-connection", cx)
    });

    assert_eq!(
        tester.requests(),
        [(
            "10.0.9.20".to_string(),
            2222,
            "deploy".to_string(),
            Some("pw-typed".to_string())
        )]
    );
    assert_eq!(test_result(cx, handle).as_deref(), Some("连接成功"));
}

#[gpui_kit::test]
fn a_failed_connection_test_says_why(cx: &mut TestAppContext) {
    let (handle, _, _, _) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::failing("用户名或密码错误"),
    );
    open_edit_dialog(cx, handle, 1);

    in_frame(cx, handle, |window, cx| {
        window.click("host-test-connection", cx)
    });

    assert_eq!(
        test_result(cx, handle).as_deref(),
        Some("连接失败：用户名或密码错误")
    );
}

#[gpui_kit::test]
fn a_connection_test_asks_to_trust_a_new_host_key_over_the_host_dialog(cx: &mut TestAppContext) {
    let (handle, _, _, tester) = open_remote(
        cx,
        FakeTerminalFactory::default(),
        FakeConnectionTester::asking_trust(),
    );
    open_edit_dialog(cx, handle, 1);

    in_frame(cx, handle, |window, cx| {
        window.click("host-test-connection", cx)
    });
    let text = prompt_label(cx, handle).expect("确认框叠在主机对话框上");
    assert!(text.starts_with("确认主机密钥"), "{text}");
    in_frame(cx, handle, |window, cx| window.click("prompt-confirm", cx));

    assert_eq!(tester.trust_answers(), [true]);
    assert_eq!(test_result(cx, handle).as_deref(), Some("连接成功"));
    // 主机对话框还开着。
    assert!(in_frame(cx, handle, |window, _| window
        .try_find("host-dialog")
        .is_some()));
}
