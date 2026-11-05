//! 凭据（工单 14）的 UI 测试：右侧栏的凭据面板（保险库状态条、列表、编辑、右键菜单）、凭据对话框的
//! 三种凭据、主机对话框的「使用凭据」「无密码」和叠层新建、用凭据登录、删除在用的凭据。

use qishell::app::ShowCredentials;

use crate::support::*;

fn open_with(
    cx: &mut TestAppContext,
    services: WorkspaceServices,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeRemoteTerminals>,
) {
    let remote = Arc::new(FakeRemoteTerminals::default());
    let services = WorkspaceServices {
        hosts: seed_with_credentials(),
        remote_terminals: remote.clone(),
        ..services
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    (handle, workspace, remote)
}

fn open(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<FakeRemoteTerminals>,
) {
    open_with(cx, fake_services())
}

fn present(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    in_frame(cx, handle, move |window, _| window.try_find(id).is_some())
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

fn click(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: impl Into<ElementId>) {
    let id = id.into();
    in_frame(cx, handle, move |window, cx| window.click(id, cx));
}

fn row(id: u64) -> (&'static str, usize) {
    ("credential-row", id as usize)
}

/// 面板里各行的文字，从上到下。
fn rows(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> Vec<String> {
    (1..=8)
        .filter_map(|id| label(cx, handle, row(id)))
        .collect()
}

#[gpui_kit::test]
fn the_credentials_icon_opens_the_panel_and_closes_it_again(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open(cx);

    click(cx, handle, "right-sidebar-credentials");
    assert!(present(cx, handle, "credentials"));
    cx.update(|cx| {
        assert_eq!(
            workspace.read(cx).right_panel(),
            Some(qishell::workspace::RightPanel::Credentials)
        )
    });
    assert_eq!(
        rows(cx, handle),
        [
            "部署密钥 · 密钥 · ED25519 · deploy · 2 台主机",
            "运维密码 · 密码 · root · 1 台主机",
            "本机 Agent · SSH Agent · ops · 1 台主机",
            "旧 RSA 密钥 · 密钥 · admin · 未使用",
        ]
    );

    click(cx, handle, "right-sidebar-credentials");
    assert!(!present(cx, handle, "credentials"));
    cx.update(|cx| assert_eq!(workspace.read(cx).right_panel(), None));

    // 命令片段面板和凭据面板共用一个位置。
    dispatch(cx, handle, ShowCredentials);
    dispatch(cx, handle, qishell::app::ShowSnippets);
    assert!(!present(cx, handle, "credentials"));
    assert!(present(cx, handle, "snippets"));
}

#[gpui_kit::test]
fn the_vault_bar_says_there_is_no_master_password(cx: &mut TestAppContext) {
    for vault in [TestVault::unprotected(), TestVault::absent()] {
        let (handle, _, _) = open_with(cx, vault.services());
        dispatch(cx, handle, ShowCredentials);
        assert_eq!(
            label(cx, handle, "credentials-vault").as_deref(),
            Some("未设主密码：秘密只靠本机钥匙串保护")
        );
        assert_eq!(
            label(cx, handle, "credentials-vault-action").as_deref(),
            Some("设置…")
        );
        click(cx, handle, "credentials-vault-action");
        assert!(present(cx, handle, "vault-password-dialog"));
        in_frame(cx, handle, |window, cx| window.close_dialog(cx));
    }
}

#[gpui_kit::test]
fn a_locked_vault_still_lists_every_credential_and_unlocks_from_the_bar(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked().locked();
    let (handle, _, _) = open_with(cx, vault.services());
    dispatch(cx, handle, ShowCredentials);

    assert_eq!(
        label(cx, handle, "credentials-vault").as_deref(),
        Some("保险库已锁定")
    );
    // 锁着时名称、类型、用户名照常显示。
    assert_eq!(rows(cx, handle).len(), 4);
    assert_eq!(
        label(cx, handle, row(1)).as_deref(),
        Some("部署密钥 · 密钥 · ED25519 · deploy · 2 台主机")
    );

    assert_eq!(
        label(cx, handle, "credentials-vault-action").as_deref(),
        Some("解锁…")
    );
    click(cx, handle, "credentials-vault-action");
    assert!(present(cx, handle, "vault-unlock"));
}

#[gpui_kit::test]
fn an_unlocked_vault_locks_from_the_bar(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked();
    let (handle, _, _) = open_with(cx, vault.services());
    dispatch(cx, handle, ShowCredentials);

    assert_eq!(
        label(cx, handle, "credentials-vault").as_deref(),
        Some("保险库已解锁")
    );
    assert_eq!(
        label(cx, handle, "credentials-vault-action").as_deref(),
        Some("锁定")
    );
    click(cx, handle, "credentials-vault-action");
    assert_eq!(vault.status(), VaultStatus::Locked);
    assert_eq!(
        label(cx, handle, "credentials-vault").as_deref(),
        Some("保险库已锁定")
    );
}
