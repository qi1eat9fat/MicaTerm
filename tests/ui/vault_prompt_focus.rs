//! 解锁后不抢问答框的焦点（工单 32）的 UI 测试：保险库锁着 → 连接（卡片先等解锁）→ 解锁 → 开始连接
//! → 连接弹问答框（验证码、主机密钥确认）→ 用户直接打字回答，不用先点问答框。
//! 也覆盖解锁时几张卡片一起等、几个问答框排队，以及问答框开着时新建的卡片不抢焦点、
//! 问答框关了焦点交给它。
//!
//! 保险库是真的（`support/vault.rs`），远程终端是假的：问题由测试替连接表发。

use std::time::Duration;

use qishell::app::ConnectHost;
use qishell::backend::{AuthenticationRequest, ConnectionPromptField, ConnectionPromptKind};
use qishell_hosts::HostId;
use qishell_terminal::TerminalLifecycle;

use crate::support::*;

/// web-01（主机 1）存了密码、保险库锁着；web-02（主机 2）没存秘密，连它不用解锁。
fn locked_vault() -> TestVault {
    TestVault::unlocked()
        .with_secret(&SecretRef::password(1), "hunter2")
        .locked()
}

fn open_with_vault(
    cx: &mut TestAppContext,
    vault: &TestVault,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeRemoteTerminals>,
) {
    let remote = Arc::new(FakeRemoteTerminals::default());
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        remote_terminals: remote.clone(),
        ..vault.services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    (handle, workspace, remote)
}

fn visible(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &'static str) -> bool {
    in_frame(cx, handle, move |window, _| window.try_find(id).is_some())
}

/// 正在问的问答框的文字；没有问答框是 `None`。
fn prompt_label(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> Option<String> {
    in_frame(cx, handle, |window, _| {
        window
            .try_find("connection-prompt")
            .and_then(|element| element.label().map(str::to_owned))
    })
}

/// 在解锁框的主密码框里输入主密码，回车。
fn unlock(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    in_frame(cx, handle, |window, cx| {
        window.click("vault-unlock-password", cx)
    });
    in_frame(cx, handle, |window, cx| window.input(MASTER, cx));
    press(cx, handle, "enter");
}

fn press(cx: &mut TestAppContext, handle: WindowHandle<Root>, key: &'static str) {
    in_frame(cx, handle, move |window, cx| window.press(key, cx));
}

/// 不点任何地方，直接打字。
fn type_text(cx: &mut TestAppContext, handle: WindowHandle<Root>, text: &'static str) {
    in_frame(cx, handle, move |window, cx| window.input(text, cx));
}

fn verification_code() -> ConnectionPromptKind {
    ConnectionPromptKind::authentication(AuthenticationRequest::KeyboardInteractive {
        name: String::new(),
        instructions: String::new(),
        fields: vec![ConnectionPromptField::new("Verification code: ", true)],
        saved_password_rejected: false,
    })
}

fn unknown_host() -> ConnectionPromptKind {
    ConnectionPromptKind::unknown_host("10.0.1.12", 22, "ssh-ed25519", "SHA256:abc")
}

/// 问答框的第一个输入框拿着焦点。
fn prompt_field_focused(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> bool {
    in_frame(cx, handle, |window, _| {
        window.find(("prompt-field", 0usize)).focused() == Some(true)
    })
}

/// 卡片 `card` 的终端拿着焦点。
fn terminal_focused(cx: &mut TestAppContext, handle: WindowHandle<Root>, card: usize) -> bool {
    in_frame(cx, handle, move |window, _| {
        window.find(("terminal", card)).focused() == Some(true)
    })
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

fn wait_running(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    workspace: &Entity<Workspace>,
    card: u64,
) {
    let running = wait_until(cx, handle, |cx| {
        cx.update(|cx| {
            workspace
                .read(cx)
                .card(CardId(card), cx)
                .map(|card| card.read(cx).terminal().read(cx).lifecycle(cx))
        }) == Some(TerminalLifecycle::Running)
    });
    assert!(running, "卡片 {card} 没有连上");
}

#[gpui_kit::test]
fn after_unlocking_the_connections_question_is_answered_by_typing_straight_away(
    cx: &mut TestAppContext,
) {
    let vault = locked_vault();
    let (handle, workspace, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    assert!(visible(cx, handle, "vault-unlock"));
    unlock(cx, handle);
    assert!(!visible(cx, handle, "vault-unlock"));

    // 解开后卡片接着连，连接问验证码：焦点在问答框里，直接打字、回车就答了。
    let code = remote.ask(1, verification_code());
    cx.run_until_parked();
    assert!(prompt_label(cx, handle).is_some_and(|text| text.starts_with("输入验证码")));
    assert!(prompt_field_focused(cx, handle));
    type_text(cx, handle, "4821");
    press(cx, handle, "enter");
    assert_eq!(
        remote.replies(),
        [(code, Answered::Answers(vec!["4821".into()]))]
    );
    assert_eq!(prompt_label(cx, handle), None);

    // 答完焦点回到这张卡片。
    wait_running(cx, handle, &workspace, 1);
    assert!(terminal_focused(cx, handle, 1));
}

#[gpui_kit::test]
fn after_unlocking_the_host_key_question_takes_the_keyboard(cx: &mut TestAppContext) {
    let vault = locked_vault();
    let (handle, workspace, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    unlock(cx, handle);

    let trust = remote.ask(1, unknown_host());
    cx.run_until_parked();
    assert!(prompt_label(cx, handle).is_some());
    // 主机密钥框没有输入框：键盘在框里，Esc 就是取消（不会落到卡片上）。
    press(cx, handle, "escape");
    assert_eq!(remote.replies(), [(trust, Answered::Cancel)]);
    assert_eq!(prompt_label(cx, handle), None);
    wait_running(cx, handle, &workspace, 1);
    assert!(terminal_focused(cx, handle, 1));
}

/// 几张卡片一起等解锁（分屏、另一个标签）：解开后它们的问题排着队，一个答完下一个接着拿焦点，
/// 都能直接打字回答。
#[gpui_kit::test]
fn several_waiting_cards_queue_their_questions_and_each_takes_the_keyboard(
    cx: &mut TestAppContext,
) {
    let vault = locked_vault();
    let (handle, workspace, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    dispatch(cx, handle, SplitRight);
    dispatch(cx, handle, ConnectHost(HostId(1)));
    cx.update(|cx| assert_eq!(workspace.read(cx).tab_ids().len(), 2));
    unlock(cx, handle);
    assert!(!visible(cx, handle, "vault-unlock"));

    let first = remote.ask(1, verification_code());
    let second = remote.ask(2, verification_code());
    let third = remote.ask(3, unknown_host());
    cx.run_until_parked();

    assert!(prompt_field_focused(cx, handle));
    type_text(cx, handle, "1111");
    press(cx, handle, "enter");
    assert!(prompt_field_focused(cx, handle));
    type_text(cx, handle, "2222");
    press(cx, handle, "enter");
    assert!(prompt_label(cx, handle).is_some());
    press(cx, handle, "escape");
    assert_eq!(
        remote.replies(),
        [
            (first, Answered::Answers(vec!["1111".into()])),
            (second, Answered::Answers(vec!["2222".into()])),
            (third, Answered::Cancel),
        ]
    );
    assert_eq!(prompt_label(cx, handle), None);
    wait_running(cx, handle, &workspace, 3);
    // 最后开的卡片在前面的标签里：焦点回到它。
    assert!(terminal_focused(cx, handle, 3));
}

/// 问答框开着时又开了一张卡片（这里是连一台用不到保险库的主机）：它不抢问答框的焦点，
/// 用户照样直接打字回答；问答框关了，焦点交给这张新卡片。
#[gpui_kit::test]
fn a_card_opened_while_a_question_is_open_waits_for_it_before_taking_focus(
    cx: &mut TestAppContext,
) {
    let vault = locked_vault();
    let (handle, workspace, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    unlock(cx, handle);
    let code = remote.ask(1, verification_code());
    cx.run_until_parked();
    assert!(prompt_field_focused(cx, handle));

    dispatch(cx, handle, ConnectHost(HostId(2)));
    wait_running(cx, handle, &workspace, 2);
    assert!(prompt_field_focused(cx, handle));
    assert!(!terminal_focused(cx, handle, 2));
    type_text(cx, handle, "4821");
    press(cx, handle, "enter");
    assert_eq!(
        remote.replies(),
        [(code, Answered::Answers(vec!["4821".into()]))]
    );

    // 问答框关了：焦点给新卡片。
    assert_eq!(prompt_label(cx, handle), None);
    assert!(terminal_focused(cx, handle, 2));
    cx.update(|cx| assert_eq!(workspace.read(cx).focused_card(cx), Some(CardId(2))));
}

/// 解锁框开着时又开了一张卡片：它也不抢主密码框的焦点，直接打主密码就能解锁；
/// 解开后焦点给这张新卡片。
#[gpui_kit::test]
fn a_card_opened_while_unlocking_does_not_take_the_master_password_field(cx: &mut TestAppContext) {
    let vault = locked_vault();
    let (handle, workspace, _remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    dispatch(cx, handle, ConnectHost(HostId(2)));
    wait_running(cx, handle, &workspace, 2);
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("vault-unlock-password").focused(), Some(true));
    });
    type_text(cx, handle, MASTER);
    press(cx, handle, "enter");
    assert!(!visible(cx, handle, "vault-unlock"));
    assert_eq!(vault.status(), VaultStatus::Unlocked { remembered: true });
    assert!(terminal_focused(cx, handle, 2));
}

/// 同上，只是用 Esc 取消解锁（gpui-component 自己关框、关完动画才还焦点）：焦点也给这张新卡片。
#[gpui_kit::test]
fn a_card_opened_while_unlocking_takes_focus_after_the_unlock_is_escaped(cx: &mut TestAppContext) {
    let vault = locked_vault();
    let (handle, workspace, _remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    dispatch(cx, handle, ConnectHost(HostId(2)));
    wait_running(cx, handle, &workspace, 2);
    press(cx, handle, "escape");
    assert!(!visible(cx, handle, "vault-unlock"));
    assert!(wait_until(cx, handle, |cx| terminal_focused(cx, handle, 2)));
    // 动画走完以后焦点也不被还回第一张卡片。
    cx.executor().advance_clock(Duration::from_secs(1));
    assert!(terminal_focused(cx, handle, 2));
    cx.update(|cx| assert_eq!(workspace.read(cx).focused_card(cx), Some(CardId(2))));
}

/// 没有问答框时，新卡片照常拿到焦点。
#[gpui_kit::test]
fn without_a_question_a_new_card_takes_focus_as_usual(cx: &mut TestAppContext) {
    let vault = locked_vault();
    let (handle, workspace, _remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    unlock(cx, handle);
    wait_running(cx, handle, &workspace, 1);
    assert!(terminal_focused(cx, handle, 1));

    dispatch(cx, handle, ConnectHost(HostId(2)));
    assert!(terminal_focused(cx, handle, 2));
    dispatch(cx, handle, SplitRight);
    assert!(terminal_focused(cx, handle, 3));
}
