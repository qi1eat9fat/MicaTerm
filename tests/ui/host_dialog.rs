//! 主机对话框（工单 11）的 UI 测试：新建的三个入口、保存、校验出错、编辑、密码进保险库。

use qishell::app::{EditHost, NewHost};

use crate::support::*;

fn dialog_open(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> bool {
    in_frame(cx, handle, |window, _| window.try_find("host-dialog").is_some())
}

/// 把字段 `field` 的内容换成 `text`。
fn fill(cx: &mut TestAppContext, handle: WindowHandle<Root>, field: &str, text: &str) {
    let field = gpui_kit::SharedString::from(field.to_owned());
    in_frame(cx, handle, move |window, cx| window.click(field, cx));
    in_frame(cx, handle, |window, cx| {
        window.press("ctrl-a", cx);
        window.press("backspace", cx);
        if !text.is_empty() {
            window.input(text, cx);
        }
    });
}

fn save(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    in_frame(cx, handle, |window, cx| window.click("host-dialog-save", cx));
}

fn error_of(cx: &mut TestAppContext, handle: WindowHandle<Root>, field: &str) -> Option<String> {
    let id = format!("{field}-error");
    in_frame(cx, handle, move |window, _| {
        window
            .try_find(gpui_kit::SharedString::from(id))
            .and_then(|error| error.label().map(str::to_owned))
    })
}

#[gpui_kit::test]
fn the_plus_after_the_host_count_opens_a_new_host_dialog(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, cx| window.click("host-sidebar-new-host", cx));

    assert!(dialog_open(cx, handle));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("host-dialog-title").label(), Some("新建主机"));
        // 焦点先给「名称」。
        assert_eq!(window.find("host-name").focused(), Some(true));
        for id in ["host-address", "host-port", "host-user", "host-password", "host-notes"] {
            assert!(window.find(id).visible(), "{id}");
        }
        // 本工单的认证只有「密码」，连接方式只有「直接」。
        assert_eq!(window.find(("host-auth", 0usize)).label(), Some("密码"));
        assert!(window.try_find(("host-auth", 1usize)).is_none());
        assert_eq!(window.find(("host-route", 0usize)).label(), Some("直接"));
        assert!(window.try_find(("host-route", 1usize)).is_none());
    });

    in_frame(cx, handle, |window, cx| window.click("host-dialog-cancel", cx));
    assert!(!dialog_open(cx, handle));
}

#[gpui_kit::test]
fn ctrl_shift_n_and_the_start_card_open_it_too(cx: &mut TestAppContext) {
    let (handle, _) = open_workspace(cx);

    in_frame(cx, handle, |window, cx| window.press("ctrl-shift-n", cx));
    assert!(dialog_open(cx, handle));
    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    assert!(!dialog_open(cx, handle));

    in_frame(cx, handle, |window, cx| window.click("start-new-host", cx));
    assert!(dialog_open(cx, handle));
}

#[gpui_kit::test]
fn a_new_host_is_saved_named_after_its_address_and_selected(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_seed(cx);
    dispatch(cx, handle, NewHost);

    fill(cx, handle, "host-address", " 10.0.1.21 ");
    fill(cx, handle, "host-port", "2200");
    fill(cx, handle, "host-user", "ubuntu");
    fill(cx, handle, "host-password", "hunter2");
    save(cx, handle);

    assert!(!dialog_open(cx, handle));
    let host = saved_host(cx, &workspace, 7).expect("saved");
    // 名称留空用地址。
    assert_eq!(host.name.as_ref(), "10.0.1.21");
    assert_eq!(host.address.as_ref(), "10.0.1.21");
    assert_eq!(host.port, 2200);
    assert_eq!(host.user.as_ref(), "ubuntu");
    assert_eq!(host.auth, AuthKind::Password);
    // 最后一个，自动选上。
    assert_eq!(host_names(cx, &workspace).last().map(String::as_str), Some("10.0.1.21"));
    cx.update(|cx| assert_eq!(workspace.read(cx).selected_host(cx), Some(HostId(7))));
    // 密码按主机 id 进保险库。
    assert_eq!(
        secrets
            .get(&host.password_secret())
            .unwrap()
            .as_deref()
            .map(String::as_str),
        Some("hunter2")
    );
}

#[gpui_kit::test]
fn a_blank_password_saves_nothing_so_each_connection_asks(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_hosts(cx, HostData::empty());
    dispatch(cx, handle, NewHost);

    fill(cx, handle, "host-name", "web-01");
    fill(cx, handle, "host-address", "10.0.1.12");
    save(cx, handle);

    assert_eq!(host_names(cx, &workspace), ["web-01"]);
    assert!(secrets.is_empty());
}

#[gpui_kit::test]
fn a_wrong_field_turns_red_says_why_and_keeps_the_dialog_open(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open_with_hosts(cx, HostData::empty());
    dispatch(cx, handle, NewHost);

    fill(cx, handle, "host-name", "mysql-02");
    fill(cx, handle, "host-port", "ssh");
    save(cx, handle);

    assert!(dialog_open(cx, handle));
    assert!(host_names(cx, &workspace).is_empty());
    assert_eq!(error_of(cx, handle, "host-address").as_deref(), Some("地址不能为空"));
    assert_eq!(
        error_of(cx, handle, "host-port").as_deref(),
        Some("端口必须是 1 到 65535 之间的数字")
    );

    // 改了哪个字段，那个字段的错误先收起来；另一个还在。
    fill(cx, handle, "host-address", "10.0.3.12");
    assert_eq!(error_of(cx, handle, "host-address"), None);
    assert!(error_of(cx, handle, "host-port").is_some());

    // 只剩端口不对，照样不关。
    save(cx, handle);
    assert!(dialog_open(cx, handle));
    assert_eq!(error_of(cx, handle, "host-address"), None);

    fill(cx, handle, "host-port", "22");
    save(cx, handle);
    assert!(!dialog_open(cx, handle));
    assert_eq!(host_names(cx, &workspace), ["mysql-02"]);
}

#[gpui_kit::test]
fn editing_a_host_starts_from_what_it_has_and_saves_the_change(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_seed(cx);
    let staging = saved_host(cx, &workspace, 4).unwrap();
    secrets.set(&staging.password_secret(), "old-secret").unwrap();

    dispatch(cx, handle, EditHost(HostId(4)));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("host-dialog-title").label(), Some("编辑主机"));
    });

    fill(cx, handle, "host-name", "staging");
    fill(cx, handle, "host-port", "2223");
    fill(cx, handle, "host-notes", "预发环境");
    save(cx, handle);

    assert!(!dialog_open(cx, handle));
    let host = saved_host(cx, &workspace, 4).unwrap();
    assert_eq!(host.name.as_ref(), "staging");
    // 没改的字段原样保留。
    assert_eq!(host.address.as_ref(), "10.0.9.20");
    assert_eq!(host.user.as_ref(), "deploy");
    assert_eq!(host.port, 2223);
    assert_eq!(host.notes.as_ref(), "预发环境");
    // 位置不变。
    assert_eq!(host_names(cx, &workspace)[3], "staging");
    // 没碰密码框：原来的密码还在。
    assert_eq!(
        secrets
            .get(&host.password_secret())
            .unwrap()
            .as_deref()
            .map(String::as_str),
        Some("old-secret")
    );
}

#[gpui_kit::test]
fn clearing_the_saved_password_deletes_it(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_seed(cx);
    let web01 = saved_host(cx, &workspace, 1).unwrap();
    secrets.set(&web01.password_secret(), "old-secret").unwrap();

    dispatch(cx, handle, EditHost(HostId(1)));
    fill(cx, handle, "host-password", "");
    save(cx, handle);

    assert!(secrets.get(&web01.password_secret()).unwrap().is_none());
}
