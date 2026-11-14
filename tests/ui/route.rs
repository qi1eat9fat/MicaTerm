//! 跳板与代理（工单 15）的 UI 测试：主机对话框「连接方式」的跳板和代理表单、
//! 删除被当作跳板的主机后路线里留下「已删除的主机」、连接时把这条断了的路线交给连接器。

use micaterm::app::{ConnectHost, DeleteHost, EditHost, NewHost};
use micaterm_hosts::{ProxyKind, ProxySettings, Route, SecretRef};
use micaterm_ssh::{JumpLogin, LoginRoute};

use crate::support::*;

fn dialog_open(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> bool {
    in_frame(cx, handle, |window, _| {
        window.try_find("host-dialog").is_some()
    })
}

/// 把字段 `field` 的内容换成 `text`。
fn fill(cx: &mut TestAppContext, handle: WindowHandle<Root>, field: &str, text: &str) {
    let field = gpui_kit::SharedString::from(field.to_owned());
    reveal(cx, handle, &field.clone().into());
    in_frame(cx, handle, move |window, cx| window.click(field, cx));
    in_frame(cx, handle, |window, cx| {
        window.press("ctrl-a", cx);
        window.press("backspace", cx);
        if !text.is_empty() {
            window.input(text, cx);
        }
    });
}

fn click(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: impl Into<ElementId>) {
    let id = id.into();
    reveal(cx, handle, &id);
    in_frame(cx, handle, move |window, cx| window.click(id, cx));
}

/// 「保存」在底部，不随内容滚动。
fn save(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    click_host_save(cx, handle);
}

fn label(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    id: impl Into<ElementId>,
) -> Option<String> {
    let id = id.into();
    in_frame(cx, handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.label().map(str::to_owned))
    })
}

fn error_of(cx: &mut TestAppContext, handle: WindowHandle<Root>, field: &str) -> Option<String> {
    label(
        cx,
        handle,
        gpui_kit::SharedString::from(format!("{field}-error")),
    )
}

fn choose_jump(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    click(cx, handle, ("host-route", 1usize));
}

fn choose_proxy(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    click(cx, handle, ("host-route", 2usize));
}

/// 点「添加跳板主机」，再点主机 `id`。
fn add_jump(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: u64) {
    click(cx, handle, "host-jump-add");
    click(cx, handle, ("host-jump-candidate", id as usize));
}

fn chain(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> Option<String> {
    label(cx, handle, "host-route-chain")
}

fn route_of(cx: &mut TestAppContext, workspace: &Entity<Workspace>, id: u64) -> Route {
    saved_host(cx, workspace, id).expect("saved").route
}

/// 种子数据加一台经 web-01（id 1）跳过去的 db-master（id 7）。
fn seed_with_jump_user() -> HostData {
    let mut data = HostData::seed();
    data.insert_unnotified(
        HostDraft::new("db-master", "10.0.2.8", 22, "root", AuthKind::Password)
            .with_route(Route::Jump(vec![Some(HostId(1))])),
    );
    data
}

#[gpui_kit::test]
fn the_route_offers_direct_jump_hosts_and_a_proxy(cx: &mut TestAppContext) {
    let (handle, _, _) = open_with_seed(cx);
    dispatch(cx, handle, NewHost);

    in_frame(cx, handle, |window, _| {
        let choices: Vec<_> = (0..3usize)
            .map(|ix| window.find(("host-route", ix)).label().map(str::to_owned))
            .collect();
        assert_eq!(
            choices,
            [
                Some("直接".into()),
                Some("SSH 跳板".into()),
                Some("代理".into())
            ]
        );
        // 直接连接没有别的字段。
        assert!(window.try_find("host-route-chain").is_none());
        assert!(window.try_find("host-proxy-host").is_none());
    });

    choose_jump(cx, handle);
    assert_eq!(chain(cx, handle).as_deref(), Some("本机 → 当前主机"));
    assert!(label(cx, handle, "host-proxy-host").is_none());

    choose_proxy(cx, handle);
    assert!(chain(cx, handle).is_none());
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find(("host-proxy-kind", 0usize)).label(),
            Some("HTTP")
        );
        assert_eq!(
            window.find(("host-proxy-kind", 1usize)).label(),
            Some("SOCKS5")
        );
        for id in [
            "host-proxy-host",
            "host-proxy-port",
            "host-proxy-user",
            "host-proxy-password",
        ] {
            assert!(window.find(id).visible(), "{id}");
        }
    });
}

#[gpui_kit::test]
fn jump_hosts_are_added_in_order_shown_as_a_route_and_saved(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_seed(cx);
    dispatch(cx, handle, NewHost);
    fill(cx, handle, "host-name", "db-master");
    fill(cx, handle, "host-address", "10.0.2.8");
    choose_jump(cx, handle);

    add_jump(cx, handle, 1);
    add_jump(cx, handle, 3);
    assert_eq!(
        chain(cx, handle).as_deref(),
        Some("本机 → web-01 → db-01 → db-master")
    );
    assert_eq!(
        label(cx, handle, ("host-jump", 0usize)).as_deref(),
        Some("web-01")
    );
    // 已经加了的不再列出来。
    click(cx, handle, "host-jump-add");
    assert!(label(cx, handle, ("host-jump-candidate", 1usize)).is_none());
    assert_eq!(
        label(cx, handle, ("host-jump-candidate", 2usize)).as_deref(),
        Some("web-02")
    );
    click(cx, handle, "host-jump-add");

    // 第二个上移：db-01 先走。
    click(cx, handle, ("host-jump-up", 1usize));
    assert_eq!(
        chain(cx, handle).as_deref(),
        Some("本机 → db-01 → web-01 → db-master")
    );
    add_jump(cx, handle, 2);
    click(cx, handle, ("host-jump-remove", 1usize));
    assert_eq!(
        chain(cx, handle).as_deref(),
        Some("本机 → db-01 → web-02 → db-master")
    );
    save(cx, handle);

    assert!(!dialog_open(cx, handle));
    assert_eq!(
        route_of(cx, &workspace, 7),
        Route::Jump(vec![Some(HostId(3)), Some(HostId(2))])
    );
}

#[gpui_kit::test]
fn a_jump_route_needs_a_jump_host(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_seed(cx);
    dispatch(cx, handle, NewHost);
    fill(cx, handle, "host-address", "10.0.2.8");
    choose_jump(cx, handle);
    save(cx, handle);

    assert!(dialog_open(cx, handle));
    assert_eq!(
        error_of(cx, handle, "host-jumps").as_deref(),
        Some("请添加跳板主机")
    );
    // 加一个，错误收起，能保存。
    add_jump(cx, handle, 1);
    assert_eq!(error_of(cx, handle, "host-jumps"), None);
    save(cx, handle);
    assert!(!dialog_open(cx, handle));
    assert_eq!(
        route_of(cx, &workspace, 7),
        Route::Jump(vec![Some(HostId(1))])
    );
}

#[gpui_kit::test]
fn a_host_is_not_offered_as_its_own_jump_host(cx: &mut TestAppContext) {
    let (handle, _, _) = open_with_seed(cx);
    dispatch(cx, handle, EditHost(HostId(3)));
    choose_jump(cx, handle);
    click(cx, handle, "host-jump-add");

    assert!(label(cx, handle, ("host-jump-candidate", 3usize)).is_none());
    assert_eq!(
        label(cx, handle, ("host-jump-candidate", 1usize)).as_deref(),
        Some("web-01")
    );
    assert_eq!(
        chain(cx, handle).as_deref(),
        Some("本机 → db-01"),
        "最后一站是这台主机自己"
    );
}

#[gpui_kit::test]
fn a_proxy_is_saved_with_its_password_in_the_vault(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_seed(cx);
    dispatch(cx, handle, NewHost);
    fill(cx, handle, "host-address", "10.0.3.11");
    choose_proxy(cx, handle);
    click(cx, handle, ("host-proxy-kind", 0usize));
    fill(cx, handle, "host-proxy-host", "proxy.test");
    fill(cx, handle, "host-proxy-port", "8080");
    fill(cx, handle, "host-proxy-user", "me");
    fill(cx, handle, "host-proxy-password", "proxy-secret");
    save(cx, handle);

    assert!(!dialog_open(cx, handle));
    assert_eq!(
        route_of(cx, &workspace, 7),
        Route::Proxy(ProxySettings::new(ProxyKind::Http, "proxy.test", 8080).with_user("me"))
    );
    assert_eq!(
        secrets
            .get(&SecretRef::proxy("me", "proxy.test", 8080))
            .unwrap()
            .as_deref()
            .map(String::as_str),
        Some("proxy-secret")
    );
}

#[gpui_kit::test]
fn a_proxy_without_an_address_or_with_a_bad_port_says_why(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_seed(cx);
    dispatch(cx, handle, NewHost);
    fill(cx, handle, "host-address", "10.0.3.11");
    choose_proxy(cx, handle);
    fill(cx, handle, "host-proxy-port", "socks");
    save(cx, handle);

    assert!(dialog_open(cx, handle));
    assert_eq!(
        error_of(cx, handle, "host-proxy-port").as_deref(),
        Some("代理端口必须是 1 到 65535 之间的数字")
    );

    fill(cx, handle, "host-proxy-port", "1080");
    save(cx, handle);
    assert_eq!(
        error_of(cx, handle, "host-proxy-host").as_deref(),
        Some("代理地址不能为空")
    );

    // 填了密码没填用户名：密码没处存。
    fill(cx, handle, "host-proxy-host", "127.0.0.1");
    fill(cx, handle, "host-proxy-password", "lonely");
    save(cx, handle);
    assert_eq!(
        error_of(cx, handle, "host-proxy-user").as_deref(),
        Some("填了代理密码就要填用户名")
    );

    fill(cx, handle, "host-proxy-password", "");
    save(cx, handle);
    assert!(!dialog_open(cx, handle));
    // 默认 SOCKS5，没有用户名就没有密码。
    assert_eq!(
        route_of(cx, &workspace, 7),
        Route::Proxy(ProxySettings::new(ProxyKind::Socks5, "127.0.0.1", 1080))
    );
    assert!(secrets.is_empty());
}

#[gpui_kit::test]
fn editing_a_proxy_host_shows_its_saved_proxy_password_and_clearing_it_deletes_it(
    cx: &mut TestAppContext,
) {
    let mut data = HostData::seed();
    data.insert_unnotified(
        HostDraft::new("mysql-01", "10.0.3.11", 22, "root", AuthKind::Password).with_route(
            Route::Proxy(ProxySettings::new(ProxyKind::Socks5, "127.0.0.1", 1080).with_user("me")),
        ),
    );
    let (handle, workspace, secrets) = open_with_hosts(cx, data);
    let secret = SecretRef::proxy("me", "127.0.0.1", 1080);
    secrets.set(&secret, "proxy-secret").unwrap();

    dispatch(cx, handle, EditHost(HostId(7)));
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find(("host-route", 2usize)).selected(),
            Some(true),
            "代理那一项选着"
        );
        assert_eq!(
            window.find(("host-proxy-kind", 1usize)).selected(),
            Some(true)
        );
    });
    // 只改备注：代理和它的密码原样留着。
    fill(cx, handle, "host-notes", "经代理");
    save(cx, handle);
    assert!(!dialog_open(cx, handle));
    assert_eq!(
        route_of(cx, &workspace, 7),
        Route::Proxy(ProxySettings::new(ProxyKind::Socks5, "127.0.0.1", 1080).with_user("me"))
    );
    assert_eq!(
        secrets.get(&secret).unwrap().as_deref().map(String::as_str),
        Some("proxy-secret")
    );

    dispatch(cx, handle, EditHost(HostId(7)));
    fill(cx, handle, "host-proxy-password", "");
    save(cx, handle);
    assert!(secrets.get(&secret).unwrap().is_none());
}

#[gpui_kit::test]
fn deleting_a_jump_host_leaves_a_red_deleted_host_in_the_route(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_hosts(cx, seed_with_jump_user());

    dispatch(cx, handle, DeleteHost(HostId(1)));
    assert!(
        label(cx, handle, "host-delete-text")
            .unwrap()
            .contains("有 1 台主机把它当作跳板：\ndb-master")
    );
    click(cx, handle, "host-delete-confirm");
    assert_eq!(route_of(cx, &workspace, 7), Route::Jump(vec![None]));

    dispatch(cx, handle, EditHost(HostId(7)));
    assert_eq!(
        chain(cx, handle).as_deref(),
        Some("本机 → 已删除的主机 → db-master")
    );
    assert_eq!(
        label(cx, handle, ("host-jump", 0usize)).as_deref(),
        Some("已删除的主机")
    );
    // 已删除的主机只能移除，不能上下移。
    assert!(label(cx, handle, ("host-jump-up", 0usize)).is_none());

    // 留着它不能保存。
    save(cx, handle);
    assert!(dialog_open(cx, handle));
    assert_eq!(
        error_of(cx, handle, "host-jumps").as_deref(),
        Some("请移除已删除的跳板主机")
    );

    click(cx, handle, ("host-jump-remove", 0usize));
    add_jump(cx, handle, 2);
    save(cx, handle);
    assert!(!dialog_open(cx, handle));
    assert_eq!(
        route_of(cx, &workspace, 7),
        Route::Jump(vec![Some(HostId(2))])
    );
}

#[gpui_kit::test]
fn connecting_a_host_whose_jump_host_was_deleted_hands_the_broken_route_to_the_connector(
    cx: &mut TestAppContext,
) {
    let remote = Arc::new(FakeRemoteTerminals::default());
    let services = WorkspaceServices {
        hosts: seed_with_jump_user(),
        remote_terminals: remote.clone(),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());

    dispatch(cx, handle, ConnectHost(HostId(7)));
    let (_, through_jump) = remote.targets().pop().expect("connected");
    let LoginRoute::Jump(hops) = &through_jump.login.route else {
        panic!("not through a jump host: {:?}", through_jump.login.route);
    };
    assert!(
        matches!(&hops[..], [JumpLogin::Host { name, login }]
            if name == "web-01" && login.host == "10.0.1.12"),
        "{hops:?}"
    );

    edit_hosts(cx, &workspace, |store, cx| {
        store.remove(HostId(1), cx);
    });
    dispatch(cx, handle, ConnectHost(HostId(7)));
    let (_, broken) = remote.targets().pop().expect("connected");
    // 连接器见到已删除的一跳直接报错，什么都不连（见 backend 的路线测试）。
    assert_eq!(
        broken.login.route,
        LoginRoute::Jump(vec![JumpLogin::Deleted])
    );
}
