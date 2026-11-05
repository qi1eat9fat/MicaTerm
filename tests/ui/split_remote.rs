//! 分屏复用连接（工单 16）的 UI 测试：远程卡片上分屏、主机右键「在右侧 / 下方分屏打开」、主机栏的绿色数字；
//! 再对着进程内的 SSH 测试服务器真的登录，看分屏不再登录、关第一张卡片不断别的通道、
//! 只给一个通道的主机分屏时回退成新建连接。

use std::time::Duration;

use qishell::app::{ConnectHost, ConnectHostDown, ConnectHostRight};
use qishell::backend::{
    ConnectionId, ConnectionPromptReply, PromptId, RemoteEvent, RemoteTarget, RemoteTerminalId,
    RemoteTerminals, SshRemoteTerminals,
};
use qishell_ssh::testing::{TEST_PASSWORD, TEST_USER, TestServer};
use qishell_ssh::{AgentLocation, SshConnector};
use qishell_terminal::{SharedTerminalTransportFactory, TerminalLifecycle};

use crate::support::*;

fn open_remote(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeRemoteTerminals>,
) {
    let remote = Arc::new(FakeRemoteTerminals::default());
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        remote_terminals: remote.clone(),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    (handle, workspace, remote)
}

/// 按真实时间等 `done` 成立（终端和 SSH 都跑在自己的线程上），同时拨测试时钟（终端每 16ms 收一次事件）。
fn wait_until(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        pump_relay();
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

fn running(cx: &mut TestAppContext, workspace: &Entity<Workspace>, card: u64) -> bool {
    lifecycle(cx, workspace, card) == Some(TerminalLifecycle::Running)
}

fn wait_running(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    workspace: &Entity<Workspace>,
    card: u64,
) {
    assert!(
        wait_until(cx, handle, |cx| running(cx, workspace, card)),
        "卡片 {card} 没有连上：{:?}",
        lifecycle(cx, workspace, card)
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

fn prompt_label(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> Option<String> {
    label(cx, handle, "connection-prompt")
}

/// 每个标签里的卡片 id，按树序。
fn tab_cards(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Vec<Vec<u64>> {
    cx.update(|cx| {
        let workspace = workspace.read(cx);
        workspace
            .tab_ids()
            .into_iter()
            .map(|tab| {
                workspace
                    .tab_cards(tab)
                    .iter()
                    .map(|card| card.read(cx).id().0)
                    .collect()
            })
            .collect()
    })
}

fn card_name(cx: &mut TestAppContext, workspace: &Entity<Workspace>, card: u64) -> String {
    cx.update(|cx| {
        workspace
            .read(cx)
            .card(CardId(card), cx)
            .map(|card| card.read(cx).name().to_string())
            .unwrap_or_default()
    })
}

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

fn open_cards(cx: &mut TestAppContext, workspace: &Entity<Workspace>, host: u64) -> usize {
    cx.update(|cx| workspace.read(cx).open_cards_of(HostId(host), cx))
}

/// 主机栏那一行右侧的绿色数字；没有卡片时不显示。
fn count_badge(cx: &mut TestAppContext, handle: WindowHandle<Root>, host: u64) -> Option<String> {
    label(cx, handle, ("host-open-cards", host as usize))
}

fn settle(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    for _ in 0..3 {
        in_frame(cx, handle, |_, _| {});
    }
}

#[gpui_kit::test]
fn splitting_a_remote_card_opens_the_same_host_on_the_same_login_without_asking(
    cx: &mut TestAppContext,
) {
    let (handle, workspace, remote) = open_remote(cx);
    connect(cx, handle, &workspace, 1, 1);

    dispatch(cx, handle, SplitRight);
    wait_running(cx, handle, &workspace, 2);
    settle(cx, handle);

    assert_eq!(tab_cards(cx, &workspace), [[1, 2]]);
    assert_eq!(card_name(cx, &workspace, 2), "web-01");
    in_frame(cx, handle, |window, _| {
        let first = window.find(("card", 1usize)).bounds();
        let second = window.find(("card", 2usize)).bounds();
        assert_eq!(first.top(), second.top());
        assert!(second.left() > first.right());
        assert_eq!(window.find(("terminal", 2usize)).focused(), Some(true));
    });
    // 登录一样：连接表复用连接、新开 shell 通道（真的不再登录见下面对着测试服务器的测试）；
    // 没有弹任何问题。
    let targets = remote.targets();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[1].0, RemoteTerminalId(2));
    assert_eq!(targets[1].1, targets[0].1);
    assert_eq!(prompt_label(cx, handle), None);
    assert!(remote.replies().is_empty());
    // 本地卡片的分屏照旧（工单 05 的测试），远程卡片分屏不起本地 shell。
    assert_eq!(remote.factory.starts(), 2);
}

#[gpui_kit::test]
fn splitting_a_card_of_an_edited_host_keeps_the_cards_login(cx: &mut TestAppContext) {
    let (handle, workspace, remote) = open_remote(cx);
    connect(cx, handle, &workspace, 1, 1);
    edit_hosts(cx, &workspace, |store, cx| {
        let draft = HostDraft::new("web-01b", "10.0.1.99", 22, "root", AuthKind::Password);
        store.update(HostId(1), draft, cx);
    });

    // 头部的向下分屏按钮。
    dispatch(cx, handle, SplitDown);
    wait_running(cx, handle, &workspace, 2);

    // 分屏复用源卡片的连接：用它打开时的主机和登录，不是编辑后的。
    let targets = remote.targets();
    assert_eq!(targets[1].1, targets[0].1);
    assert_eq!(targets[1].1.login.host, "10.0.1.12");
    assert_eq!(card_name(cx, &workspace, 2), "web-01");

    // 主机删了也能分屏。
    edit_hosts(cx, &workspace, |store, cx| {
        store.remove(HostId(1), cx);
    });
    dispatch(cx, handle, SplitRight);
    wait_running(cx, handle, &workspace, 3);
    assert_eq!(remote.targets()[2].1, targets[0].1);
    assert_eq!(tab_cards(cx, &workspace), [[1, 2, 3]]);
}

/// 主机右键菜单里那一项的标签，按顺序（含分隔线占的位置）。
fn host_menu_labels(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    host: u64,
) -> Vec<Option<String>> {
    in_frame(cx, handle, |window, cx| {
        window.right_click(host_row(host), cx)
    });
    in_frame(cx, handle, |window, _| {
        let popup = window.within("popup-menu");
        (0..4_usize)
            .map(|item| {
                popup
                    .try_find(item)
                    .and_then(|item| item.label().map(str::to_owned))
            })
            .collect()
    })
}

fn click_menu_item(cx: &mut TestAppContext, handle: WindowHandle<Root>, item: usize) {
    in_frame(cx, handle, |window, cx| {
        window.within("popup-menu").click(item, cx)
    });
}

#[gpui_kit::test]
fn the_host_menu_opens_a_host_beside_the_focused_card_local_or_remote(cx: &mut TestAppContext) {
    let (handle, workspace, remote) = open_remote(cx);
    dispatch(cx, handle, NewLocalTerminalTab);

    let labels = host_menu_labels(cx, handle, 2);
    assert_eq!(
        labels[..3],
        [
            Some("连接".to_string()),
            Some("在右侧分屏打开".to_string()),
            Some("在下方分屏打开".to_string())
        ],
        "{labels:?}"
    );
    // 本地卡片旁边，右侧。
    click_menu_item(cx, handle, 1);
    wait_running(cx, handle, &workspace, 2);
    settle(cx, handle);
    assert_eq!(tab_cards(cx, &workspace), [[1, 2]]);
    assert_eq!(card_name(cx, &workspace, 2), "web-02");
    in_frame(cx, handle, |window, _| {
        let local = window.find(("card", 1usize)).bounds();
        let remote = window.find(("card", 2usize)).bounds();
        assert_eq!(local.top(), remote.top());
        assert!(remote.left() > local.right());
    });

    // 远程卡片（焦点卡片 web-02）旁边，下方。
    host_menu_labels(cx, handle, 3);
    click_menu_item(cx, handle, 2);
    wait_running(cx, handle, &workspace, 3);
    settle(cx, handle);
    assert_eq!(tab_cards(cx, &workspace), [[1, 2, 3]]);
    assert_eq!(card_name(cx, &workspace, 3), "db-01");
    in_frame(cx, handle, |window, _| {
        let above = window.find(("card", 2usize)).bounds();
        let below = window.find(("card", 3usize)).bounds();
        assert_eq!(above.left(), below.left());
        assert!(below.top() > above.bottom());
        assert_eq!(window.find(("terminal", 3usize)).focused(), Some(true));
    });
    let hosts: Vec<u64> = remote
        .targets()
        .iter()
        .map(|(_, target)| target.host)
        .collect();
    assert_eq!(hosts, [2, 3]);
}

#[gpui_kit::test]
fn the_split_menu_items_open_a_new_tab_when_no_terminal_tab_is_open(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_remote(cx);

    dispatch(cx, handle, ConnectHostRight(HostId(1)));
    wait_running(cx, handle, &workspace, 1);
    // 已经有标签了：接着开在它旁边。
    dispatch(cx, handle, ConnectHostDown(HostId(1)));
    wait_running(cx, handle, &workspace, 2);

    assert_eq!(tab_cards(cx, &workspace), [[1, 2]]);
}

#[gpui_kit::test]
fn the_host_sidebar_counts_a_hosts_open_cards_in_green(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_remote(cx);
    assert_eq!(count_badge(cx, handle, 1), None);

    connect(cx, handle, &workspace, 1, 1);
    dispatch(cx, handle, SplitRight);
    wait_running(cx, handle, &workspace, 2);
    connect(cx, handle, &workspace, 1, 3);
    connect(cx, handle, &workspace, 2, 4);
    // 本地卡片不算。
    dispatch(cx, handle, NewLocalTerminalTab);

    assert_eq!(count_badge(cx, handle, 1).as_deref(), Some("3"));
    assert_eq!(count_badge(cx, handle, 2).as_deref(), Some("1"));
    assert_eq!(count_badge(cx, handle, 3), None);
    assert_eq!(open_cards(cx, &workspace, 1), 3);

    // 断开了的卡片还开着，照样算。
    dispatch(cx, handle, qishell::app::DisconnectCard(CardId(1)));
    assert_eq!(count_badge(cx, handle, 1).as_deref(), Some("3"));

    dispatch(cx, handle, CloseCard(CardId(2)));
    assert_eq!(count_badge(cx, handle, 1).as_deref(), Some("2"));
    dispatch(cx, handle, CloseCard(CardId(1)));
    dispatch(cx, handle, CloseCard(CardId(3)));
    assert_eq!(count_badge(cx, handle, 1), None);
    assert_eq!(count_badge(cx, handle, 2).as_deref(), Some("1"));
}

/// 卡片 `card` 所在的连接（shell 通道开好、连接表发来 `Attached` 之后才有）。
fn connection(
    cx: &mut TestAppContext,
    workspace: &Entity<Workspace>,
    card: u64,
) -> Option<ConnectionId> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .card(CardId(card), cx)
            .and_then(|card| card.read(cx).connection())
    })
}

/// 真的连接表，事件经测试线程转给窗口壳：连接表在自己的线程上发事件，直接发会在别的线程上唤醒
/// 窗口壳的任务，GPUI 的测试调度器不允许。测试每等一轮就 `pump` 一次。
struct Relayed {
    inner: Arc<SshRemoteTerminals>,
    from: async_channel::Receiver<RemoteEvent>,
    to: async_channel::Sender<RemoteEvent>,
    events: async_channel::Receiver<RemoteEvent>,
}

impl Relayed {
    fn new(inner: Arc<SshRemoteTerminals>) -> Self {
        let (to, events) = async_channel::unbounded();
        Self {
            from: inner.events(),
            inner,
            to,
            events,
        }
    }

    fn pump(&self) {
        while let Ok(event) = self.from.try_recv() {
            let _ = self.to.try_send(event);
        }
    }
}

impl RemoteTerminals for Relayed {
    fn factory_for(
        &self,
        terminal: RemoteTerminalId,
        target: &RemoteTarget,
    ) -> SharedTerminalTransportFactory {
        self.inner.factory_for(terminal, target)
    }

    fn events(&self) -> async_channel::Receiver<RemoteEvent> {
        self.events.clone()
    }

    fn respond(&self, prompt: PromptId, reply: ConnectionPromptReply) {
        self.inner.respond(prompt, reply);
    }
}

thread_local! {
    /// 这个测试用的转发（只有对着测试服务器的测试有）。
    static RELAY: std::cell::RefCell<Option<Arc<Relayed>>> = const { std::cell::RefCell::new(None) };
}

fn pump_relay() {
    RELAY.with(|relay| {
        if let Some(relay) = relay.borrow().as_ref() {
            relay.pump();
        }
    });
}

/// 进程内的 SSH 测试服务器，和连它的真的连接表（主机密钥记在临时目录里）。
struct RealSsh {
    server: TestServer,
    remote: Arc<SshRemoteTerminals>,
    _directory: tempfile::TempDir,
    _runtime: tokio::runtime::Runtime,
}

impl RealSsh {
    fn start() -> Self {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime
            .block_on(TestServer::start(std::env::temp_dir()))
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let secrets =
            qishell_hosts::secrets::StoreSource::shared(Arc::new(InMemorySecretStore::default()));
        let connector = SshConnector::new(directory.path().join("known_hosts"), secrets)
            .with_agent(AgentLocation::None)
            .with_default_key_dir(None);
        Self {
            server,
            remote: Arc::new(SshRemoteTerminals::with_connector(connector)),
            _directory: directory,
            _runtime: runtime,
        }
    }

    /// 打开窗口壳，主机栏里只有一台主机（id 1）：这台测试服务器，密码登录、没存密码（现场问）。
    fn open(&self, cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<Workspace>) {
        let mut hosts = HostData::empty();
        hosts.insert(HostDraft::new(
            "test-server",
            "127.0.0.1",
            self.server.port,
            TEST_USER,
            AuthKind::Password,
        ));
        let relay = Arc::new(Relayed::new(self.remote.clone()));
        RELAY.with(|slot| *slot.borrow_mut() = Some(relay.clone()));
        let services = WorkspaceServices {
            hosts,
            remote_terminals: relay,
            ..fake_services()
        };
        let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
        in_frame(cx, handle, |window, _| window.activate_window());
        (handle, workspace)
    }
}

/// 回答连接问的问题（信任主机密钥、输入密码），直到卡片 `card` 连上。返回问了几次密码。
fn log_in(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    workspace: &Entity<Workspace>,
    card: u64,
) -> usize {
    let mut passwords = 0;
    let started = std::time::Instant::now();
    while !running(cx, workspace, card) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "卡片 {card} 没有连上：{:?}",
            lifecycle(cx, workspace, card)
        );
        match prompt_label(cx, handle) {
            Some(text) if text.starts_with("确认主机密钥") => {
                in_frame(cx, handle, |window, cx| window.click("prompt-confirm", cx));
            }
            Some(text) if text.starts_with("输入密码") => {
                passwords += 1;
                in_frame(cx, handle, |window, cx| window.input(TEST_PASSWORD, cx));
                in_frame(cx, handle, |window, cx| window.press("enter", cx));
            }
            Some(text) => panic!("没想到的问题：{text}"),
            None => {}
        }
        pump_relay();
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, handle, |_, _| {});
        std::thread::sleep(Duration::from_millis(5));
    }
    passwords
}

/// 在焦点卡片里打一行字，等它在卡片 `card` 的屏幕上回显出来。
fn echo(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    workspace: &Entity<Workspace>,
    card: u64,
    text: &'static str,
) {
    in_frame(cx, handle, |window, cx| window.input(text, cx));
    assert!(
        wait_until(cx, handle, |cx| cx.update(|cx| workspace
            .read(cx)
            .card_screen_text(CardId(card), cx)
            .is_some_and(|screen| screen.contains(text)))),
        "卡片 {card} 没有回显 {text}"
    );
}

#[gpui_kit::test]
fn a_split_shares_the_real_connection_and_survives_closing_the_first_card(cx: &mut TestAppContext) {
    let ssh = RealSsh::start();
    let (handle, workspace) = ssh.open(cx);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    assert_eq!(log_in(cx, handle, &workspace, 1), 1);

    dispatch(cx, handle, SplitRight);
    // 不再问任何问题：没有新登录。
    assert_eq!(log_in(cx, handle, &workspace, 2), 0);
    assert_eq!(ssh.remote.connection_count(), 1);
    assert!(wait_until(cx, handle, |cx| connection(cx, &workspace, 2).is_some()));
    assert_eq!(connection(cx, &workspace, 1), connection(cx, &workspace, 2));

    // 双击已经连着的主机开新标签，也复用（2026-11-04 用户决定）。
    dispatch(cx, handle, ConnectHost(HostId(1)));
    assert_eq!(log_in(cx, handle, &workspace, 3), 0);
    assert_eq!(ssh.remote.connection_count(), 1);

    // 关掉第一张卡片：连接跑在共享运行时上，不属于它，别的卡片的通道还在。
    dispatch(cx, handle, CloseCard(CardId(1)));
    dispatch(cx, handle, CloseCard(CardId(3)));
    assert!(running(cx, &workspace, 2));
    echo(cx, handle, &workspace, 2, "still-here");
    assert_eq!(ssh.remote.connection_count(), 1);

    dispatch(cx, handle, CloseCard(CardId(2)));
    let remote = ssh.remote.clone();
    assert!(
        wait_until(cx, handle, |_| remote.connection_count() == 0),
        "最后一张卡片关了，连接断开"
    );
}

#[gpui_kit::test]
fn a_split_of_a_host_that_gives_one_channel_logs_in_on_a_new_connection(cx: &mut TestAppContext) {
    let ssh = RealSsh::start();
    ssh.server.single_channel();
    let (handle, workspace) = ssh.open(cx);

    dispatch(cx, handle, ConnectHost(HostId(1)));
    assert_eq!(log_in(cx, handle, &workspace, 1), 1);

    dispatch(cx, handle, SplitDown);
    // 复用的连接开不出 shell 通道：自动改成新建一条连接，于是要再登录一次，而不是报错。
    assert_eq!(log_in(cx, handle, &workspace, 2), 1);
    assert_eq!(ssh.remote.connection_count(), 2);
    assert!(wait_until(cx, handle, |cx| connection(cx, &workspace, 2).is_some()));
    assert_ne!(connection(cx, &workspace, 1), connection(cx, &workspace, 2));
    assert_eq!(tab_cards(cx, &workspace), [[1, 2]]);
    echo(cx, handle, &workspace, 2, "own-channel");
}
