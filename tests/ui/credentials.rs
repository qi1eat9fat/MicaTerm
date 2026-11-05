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

// ---- 凭据对话框：三种凭据的新建 ----

/// 只在内存里的秘密，测试拿着它看存了什么。
fn open_with_secrets(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<InMemorySecretStore>,
) {
    let secrets = Arc::new(InMemorySecretStore::default());
    let (handle, workspace, _) = open_with(
        cx,
        WorkspaceServices {
            secrets: secrets.clone(),
            ..fake_services()
        },
    );
    (handle, workspace, secrets)
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

fn error_of(cx: &mut TestAppContext, handle: WindowHandle<Root>, field: &str) -> Option<String> {
    label(
        cx,
        handle,
        gpui_kit::SharedString::from(format!("{field}-error")),
    )
}

fn credential_dialog_open(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> bool {
    present(cx, handle, "credential-dialog")
}

/// 从凭据面板的「+」打开新建凭据对话框。
fn new_credential(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    dispatch(cx, handle, ShowCredentials);
    click(cx, handle, "credentials-new");
    assert!(credential_dialog_open(cx, handle));
}

fn save_credential(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    click(cx, handle, "credential-dialog-save");
}

fn credential(
    cx: &mut TestAppContext,
    workspace: &Entity<Workspace>,
    id: u64,
) -> Option<qishell_hosts::Credential> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .hosts()
            .read(cx)
            .credential(qishell_hosts::CredentialId(id))
            .cloned()
    })
}

fn secret(secrets: &InMemorySecretStore, secret: &SecretRef) -> Option<String> {
    secrets
        .get(secret)
        .unwrap()
        .map(|value| value.as_str().to_owned())
}

#[gpui_kit::test]
fn a_new_password_credential_keeps_its_password_in_the_vault(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_secrets(cx);
    new_credential(cx, handle);
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find("credential-dialog-title").label(),
            Some("新建凭据")
        );
        assert_eq!(window.find("credential-name").focused(), Some(true));
    });

    click(cx, handle, ("credential-kind", 0usize));
    save_credential(cx, handle);
    // 名称和密码都是必填的；出错时对话框不关。
    assert!(credential_dialog_open(cx, handle));
    assert_eq!(
        error_of(cx, handle, "credential-name").as_deref(),
        Some("名称不能为空")
    );
    assert_eq!(
        error_of(cx, handle, "credential-password").as_deref(),
        Some("请输入密码")
    );

    fill(cx, handle, "credential-name", "跳板密码");
    fill(cx, handle, "credential-user", "ops");
    fill(cx, handle, "credential-password", "hunter2");
    save_credential(cx, handle);

    assert!(!credential_dialog_open(cx, handle));
    let saved = credential(cx, &workspace, 5).expect("saved");
    assert_eq!(saved.name.as_ref(), "跳板密码");
    assert_eq!(saved.kind, qishell_hosts::CredentialKind::Password);
    assert_eq!(saved.user.as_ref(), "ops");
    assert_eq!(
        secret(&secrets, &saved.password_secret()).as_deref(),
        Some("hunter2")
    );
    assert_eq!(
        label(cx, handle, row(5)).as_deref(),
        Some("跳板密码 · 密码 · ops · 未使用")
    );
}

#[gpui_kit::test]
fn a_generated_key_shows_its_public_key_and_goes_into_the_vault(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_secrets(cx);
    new_credential(cx, handle);
    // 新建时照原型默认「密钥 · 生成新密钥 · Ed25519」。
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find(("credential-kind", 1usize)).selected(),
            Some(true)
        );
        assert_eq!(
            window.find(("credential-key-source", 2usize)).selected(),
            Some(true)
        );
        assert_eq!(
            window.find(("credential-key-algorithm", 0usize)).selected(),
            Some(true)
        );
    });
    fill(cx, handle, "credential-name", "新服务器密钥");
    fill(cx, handle, "credential-user", "deploy");

    let line = label(cx, handle, "credential-public-key").expect("the public key");
    assert!(line.starts_with("ssh-ed25519 "), "{line}");
    assert!(line.ends_with(" 新服务器密钥"), "{line}");
    click(cx, handle, "credential-copy-public-key");
    let copied = cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some(line.as_str()));

    fill(cx, handle, "credential-passphrase", "correct horse");
    save_credential(cx, handle);

    assert!(!credential_dialog_open(cx, handle));
    let saved = credential(cx, &workspace, 5).expect("saved");
    assert_eq!(saved.kind, qishell_hosts::CredentialKind::Key);
    assert_eq!(saved.public_key.as_deref(), Some(line.as_str()));
    // 私钥用口令加密后存进保险库，口令也存。
    let text = secret(&secrets, &saved.private_key_secret()).expect("the private key");
    let pasted = qishell_hosts::PastedKey::parse(&text).unwrap();
    assert!(pasted.is_encrypted());
    assert_eq!(
        secret(&secrets, &saved.passphrase_secret()).as_deref(),
        Some("correct horse")
    );
    assert_eq!(
        label(cx, handle, row(5)).as_deref(),
        Some("新服务器密钥 · 密钥 · ED25519 · deploy · 未使用")
    );
}

/// 一把没加密的 Ed25519 私钥的文件内容和它的公钥。
fn plain_key() -> (String, String) {
    let key = qishell_hosts::GeneratedKey::generate(qishell_hosts::KeyAlgorithm::Ed25519).unwrap();
    (
        key.encode("me@laptop", "").unwrap().to_string(),
        key.public_key_line("me@laptop"),
    )
}

#[gpui_kit::test]
fn a_pasted_key_is_read_for_its_public_key_and_kept_in_the_vault(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_secrets(cx);
    new_credential(cx, handle);
    fill(cx, handle, "credential-name", "笔记本的密钥");
    click(cx, handle, ("credential-key-source", 1usize));

    // 贴进来的是公钥：说清楚要的是私钥。
    let (text, public) = plain_key();
    cx.update(|cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(public.clone())));
    click(cx, handle, "credential-key-text");
    in_frame(cx, handle, |window, cx| window.press("ctrl-v", cx));
    save_credential(cx, handle);
    assert_eq!(
        error_of(cx, handle, "credential-key-text").as_deref(),
        Some("这是公钥（放在服务器上的那一半），请粘贴私钥")
    );

    cx.update(|cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.clone())));
    click(cx, handle, "credential-key-text");
    in_frame(cx, handle, |window, cx| {
        window.press("ctrl-a", cx);
        window.press("ctrl-v", cx);
    });
    assert_eq!(
        label(cx, handle, "credential-public-key").as_deref(),
        Some(public.as_str())
    );
    save_credential(cx, handle);

    assert!(!credential_dialog_open(cx, handle));
    let saved = credential(cx, &workspace, 5).expect("saved");
    assert_eq!(saved.user.as_ref(), "root", "用户名留空用 root");
    assert_eq!(saved.public_key.as_deref(), Some(public.as_str()));
    assert_eq!(
        secret(&secrets, &saved.private_key_secret()).as_deref(),
        Some(text.as_str())
    );
    // 没加密的私钥没有口令可存。
    assert_eq!(secret(&secrets, &saved.passphrase_secret()), None);
}

#[gpui_kit::test]
fn a_key_file_is_copied_into_the_vault_once(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_secrets(cx);
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("id_ed25519");
    let (text, public) = plain_key();
    std::fs::write(&path, text.replace('\n', "\r\n")).unwrap();

    new_credential(cx, handle);
    fill(cx, handle, "credential-name", "本机私钥");
    click(cx, handle, ("credential-key-source", 0usize));
    save_credential(cx, handle);
    assert_eq!(
        error_of(cx, handle, "credential-key-file").as_deref(),
        Some("请选择私钥文件")
    );
    fill(
        cx,
        handle,
        "credential-key-file",
        &folder.path().join("missing").to_string_lossy(),
    );
    save_credential(cx, handle);
    assert_eq!(
        error_of(cx, handle, "credential-key-file").as_deref(),
        Some("读不出这个文件")
    );

    fill(cx, handle, "credential-key-file", &path.to_string_lossy());
    save_credential(cx, handle);

    assert!(!credential_dialog_open(cx, handle));
    // 文件只读这一次：删掉它，保险库里的照样在。
    drop(folder);
    let saved = credential(cx, &workspace, 5).expect("saved");
    assert_eq!(saved.public_key.as_deref(), Some(public.as_str()));
    assert_eq!(
        secret(&secrets, &saved.private_key_secret()).as_deref(),
        Some(text.as_str())
    );
}

#[gpui_kit::test]
fn an_agent_credential_keeps_no_secret(cx: &mut TestAppContext) {
    let (handle, workspace, secrets) = open_with_secrets(cx);
    new_credential(cx, handle);
    click(cx, handle, ("credential-kind", 2usize));
    assert!(present(cx, handle, "credential-agent-hint"));
    assert!(!present(cx, handle, "credential-passphrase"));
    fill(cx, handle, "credential-name", "办公室 Agent");
    fill(cx, handle, "credential-user", "ops");
    save_credential(cx, handle);

    assert!(!credential_dialog_open(cx, handle));
    let saved = credential(cx, &workspace, 5).expect("saved");
    assert_eq!(saved.kind, qishell_hosts::CredentialKind::Agent);
    assert!(secrets.is_empty());
}

// ---- 编辑：双击、回车、右键菜单 ----

/// 右键凭据 `id`，菜单里各项的文字，从上到下（分隔线不算）；菜单留着开。
fn menu_labels(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: u64) -> Vec<String> {
    in_frame(cx, handle, |window, cx| window.right_click(row(id), cx));
    in_frame(cx, handle, |window, _| {
        let popup = window.within("popup-menu");
        (0..8_usize)
            .filter_map(|item| popup.try_find(item))
            .filter_map(|item| item.label().map(str::to_owned))
            .collect::<Vec<_>>()
    })
}

fn click_menu_item(cx: &mut TestAppContext, handle: WindowHandle<Root>, item: usize) {
    in_frame(cx, handle, |window, cx| {
        window.within("popup-menu").click(item, cx)
    });
}

#[gpui_kit::test]
fn a_double_click_or_enter_edits_a_credential(cx: &mut TestAppContext) {
    let (handle, _, _) = open(cx);
    dispatch(cx, handle, ShowCredentials);

    in_frame(cx, handle, |window, cx| window.double_click(row(2), cx));
    assert!(credential_dialog_open(cx, handle));
    assert_eq!(
        label(cx, handle, "credential-dialog-title").as_deref(),
        Some("编辑凭据")
    );
    assert_eq!(
        label(cx, handle, "credential-dialog-subtitle").as_deref(),
        Some("运维密码")
    );
    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    assert!(!credential_dialog_open(cx, handle));

    // 单击选中，↓ 移到下一行，回车编辑。
    click(cx, handle, row(1));
    in_frame(cx, handle, |window, cx| window.press("down", cx));
    in_frame(cx, handle, |window, cx| window.press("enter", cx));
    assert_eq!(
        label(cx, handle, "credential-dialog-subtitle").as_deref(),
        Some("运维密码")
    );
}

#[gpui_kit::test]
fn the_menu_edits_copies_a_public_key_and_deletes(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open(cx);
    dispatch(cx, handle, ShowCredentials);

    assert_eq!(menu_labels(cx, handle, 1), ["编辑…", "复制公钥", "删除"]);
    click_menu_item(cx, handle, 1);
    let public_key = credential(cx, &workspace, 1).unwrap().public_key.unwrap();
    let copied = cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some(public_key.as_ref()));

    // 只有密钥有「复制公钥」。
    assert_eq!(menu_labels(cx, handle, 2), ["编辑…", "删除"]);
    click_menu_item(cx, handle, 0);
    assert_eq!(
        label(cx, handle, "credential-dialog-subtitle").as_deref(),
        Some("运维密码")
    );
}

#[gpui_kit::test]
fn renaming_a_credentials_user_renames_every_host_using_it(cx: &mut TestAppContext) {
    let secrets = Arc::new(InMemorySecretStore::default());
    let (handle, workspace, _) = open_with(
        cx,
        WorkspaceServices {
            secrets: secrets.clone(),
            ..fake_services()
        },
    );
    let deploy = credential(cx, &workspace, 1).unwrap();
    secrets
        .set(
            &deploy.private_key_secret(),
            "-----BEGIN OPENSSH PRIVATE KEY-----",
        )
        .unwrap();
    dispatch(cx, handle, qishell::app::EditCredential(deploy.id));

    // 编辑密钥凭据时停在「当前私钥」，显示现在的公钥；写明有几台主机跟着改。
    in_frame(cx, handle, |window, _| {
        assert_eq!(
            window.find(("credential-key-source", 0usize)).label(),
            Some("当前私钥")
        );
        assert_eq!(
            window.find(("credential-key-source", 0usize)).selected(),
            Some(true)
        );
    });
    assert_eq!(
        label(cx, handle, "credential-public-key"),
        deploy.public_key.as_ref().map(ToString::to_string)
    );
    assert_eq!(
        label(cx, handle, "credential-usage").as_deref(),
        Some("有 2 台主机在用这条凭据，保存后它们跟着改")
    );

    fill(cx, handle, "credential-user", "release");
    save_credential(cx, handle);

    assert!(!credential_dialog_open(cx, handle));
    for host in [3, 4] {
        assert_eq!(
            saved_host(cx, &workspace, host).unwrap().user.as_ref(),
            "release"
        );
    }
    assert_eq!(saved_host(cx, &workspace, 1).unwrap().user.as_ref(), "root");
    // 私钥没换：保险库里的还在，公钥也没变。
    let edited = credential(cx, &workspace, 1).unwrap();
    assert_eq!(edited.public_key, deploy.public_key);
    assert_eq!(
        secret(&secrets, &deploy.private_key_secret()).as_deref(),
        Some("-----BEGIN OPENSSH PRIVATE KEY-----")
    );
}

#[gpui_kit::test]
fn editing_a_password_credential_starts_from_its_saved_password(cx: &mut TestAppContext) {
    let secrets = Arc::new(InMemorySecretStore::default());
    let (handle, workspace, _) = open_with(
        cx,
        WorkspaceServices {
            secrets: secrets.clone(),
            ..fake_services()
        },
    );
    let ops = credential(cx, &workspace, 2).unwrap();
    secrets.set(&ops.password_secret(), "old-secret").unwrap();

    dispatch(cx, handle, qishell::app::EditCredential(ops.id));
    fill(cx, handle, "credential-name", "运维");
    save_credential(cx, handle);

    assert!(!credential_dialog_open(cx, handle));
    assert_eq!(credential(cx, &workspace, 2).unwrap().name.as_ref(), "运维");
    assert_eq!(
        secret(&secrets, &ops.password_secret()).as_deref(),
        Some("old-secret")
    );

    // 改成新的密码。
    dispatch(cx, handle, qishell::app::EditCredential(ops.id));
    fill(cx, handle, "credential-password", "new-secret");
    save_credential(cx, handle);
    assert_eq!(
        secret(&secrets, &ops.password_secret()).as_deref(),
        Some("new-secret")
    );
}

#[gpui_kit::test]
fn a_locked_vault_keeps_a_credentials_password_out_of_the_form(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked();
    let (handle, workspace, _) = open_with(cx, vault.services());
    let ops = credential(cx, &workspace, 2).unwrap();
    vault
        .vault
        .set(&ops.password_secret(), "old-secret")
        .unwrap();
    dispatch(cx, handle, qishell::app::LockVault);

    dispatch(cx, handle, qishell::app::EditCredential(ops.id));
    assert_eq!(
        label(cx, handle, "credential-password-locked").as_deref(),
        Some("已保存，保险库锁定中 · 解锁后修改")
    );
    assert!(!present(cx, handle, "credential-password"));
    // 只改名字：不碰密码，也不用解锁。
    fill(cx, handle, "credential-name", "运维");
    save_credential(cx, handle);
    assert!(!credential_dialog_open(cx, handle));
    assert!(!present(cx, handle, "vault-unlock"));
    assert_eq!(credential(cx, &workspace, 2).unwrap().name.as_ref(), "运维");
    vault.vault.unlock(MASTER, false).unwrap();
    assert_eq!(
        vault.read(&ops.password_secret()).as_deref(),
        Some("old-secret")
    );
}

// ---- 删除 ----

#[gpui_kit::test]
fn deleting_a_key_in_use_names_its_hosts_and_leaves_them_without_a_password(
    cx: &mut TestAppContext,
) {
    let secrets = Arc::new(InMemorySecretStore::default());
    let (handle, workspace, _) = open_with(
        cx,
        WorkspaceServices {
            secrets: secrets.clone(),
            ..fake_services()
        },
    );
    let deploy = credential(cx, &workspace, 1).unwrap();
    secrets
        .set(
            &deploy.private_key_secret(),
            "-----BEGIN OPENSSH PRIVATE KEY-----",
        )
        .unwrap();
    secrets
        .set(&deploy.passphrase_secret(), "correct horse")
        .unwrap();
    dispatch(cx, handle, ShowCredentials);

    menu_labels(cx, handle, 1);
    click_menu_item(cx, handle, 3);
    assert_eq!(
        label(cx, handle, "credential-delete-text").as_deref(),
        Some(
            "确定删除「部署密钥」吗？\n有 2 台主机在用它：\ndb-01\nstaging-api\n\
             删除后，这些主机的认证改成「无密码」，用户名 deploy 保留。\n\
             保险库里的私钥和口令一起删除。"
        )
    );
    // 取消就什么都不动。
    click(cx, handle, "credential-delete-cancel");
    assert!(credential(cx, &workspace, 1).is_some());

    dispatch(cx, handle, qishell::app::DeleteCredential(deploy.id));
    click(cx, handle, "credential-delete-confirm");

    assert!(!present(cx, handle, "credential-delete"));
    assert!(credential(cx, &workspace, 1).is_none());
    assert!(!present(cx, handle, row(1)));
    for id in [3, 4] {
        let host = saved_host(cx, &workspace, id).unwrap();
        assert_eq!(host.credential, None);
        assert_eq!(host.auth, AuthKind::NoPassword);
        assert_eq!(host.user.as_ref(), "deploy");
    }
    assert_eq!(secret(&secrets, &deploy.private_key_secret()), None);
    assert_eq!(secret(&secrets, &deploy.passphrase_secret()), None);
}

#[gpui_kit::test]
fn deleting_a_password_credential_in_use_turns_its_hosts_back_to_a_password(
    cx: &mut TestAppContext,
) {
    let (handle, workspace, _) = open(cx);
    dispatch(
        cx,
        handle,
        qishell::app::DeleteCredential(qishell_hosts::CredentialId(2)),
    );
    assert_eq!(
        label(cx, handle, "credential-delete-text").as_deref(),
        Some(
            "确定删除「运维密码」吗？\n有 1 台主机在用它：\nweb-01\n\
             删除后，这些主机的认证改成「密码」，用户名 root 保留。\n\
             保险库里的密码一起删除。"
        )
    );
    click(cx, handle, "credential-delete-confirm");

    let web01 = saved_host(cx, &workspace, 1).unwrap();
    assert_eq!(web01.credential, None);
    assert_eq!(web01.auth, AuthKind::Password);
    assert_eq!(web01.user.as_ref(), "root");
}

#[gpui_kit::test]
fn deleting_an_unused_credential_just_asks(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open(cx);
    dispatch(
        cx,
        handle,
        qishell::app::DeleteCredential(qishell_hosts::CredentialId(4)),
    );
    assert_eq!(
        label(cx, handle, "credential-delete-text").as_deref(),
        Some("确定删除「旧 RSA 密钥」吗？\n保险库里的私钥和口令一起删除。")
    );
    click(cx, handle, "credential-delete-confirm");
    assert!(credential(cx, &workspace, 4).is_none());
}

// ---- 主机对话框：使用凭据、无密码、叠层新建 ----

fn host_dialog_open(cx: &mut TestAppContext, handle: WindowHandle<Root>) -> bool {
    present(cx, handle, "host-dialog")
}

fn save_host(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    click(cx, handle, "host-dialog-save");
}

#[gpui_kit::test]
fn a_host_logs_in_with_the_credential_it_picks(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open(cx);
    dispatch(cx, handle, qishell::app::NewHost);
    in_frame(cx, handle, |window, _| {
        let choices: Vec<_> = (0..3usize)
            .map(|ix| window.find(("host-auth", ix)).label().map(str::to_owned))
            .collect();
        assert_eq!(
            choices,
            [
                Some("密码".into()),
                Some("使用凭据".into()),
                Some("无密码".into())
            ]
        );
    });
    fill(cx, handle, "host-address", "10.0.1.33");
    click(cx, handle, ("host-auth", 1usize));
    assert!(!present(cx, handle, "host-password"));

    // 没选凭据：描红，不关。
    save_host(cx, handle);
    assert!(host_dialog_open(cx, handle));
    assert_eq!(
        error_of(cx, handle, "host-credential").as_deref(),
        Some("请选一条凭据")
    );

    click(cx, handle, "host-credential");
    let options: Vec<_> = (1..=4)
        .filter_map(|id| label(cx, handle, ("host-credential-option", id as usize)))
        .collect();
    assert_eq!(
        options,
        [
            "部署密钥（密钥 · deploy）",
            "运维密码（密码 · root）",
            "本机 Agent（SSH Agent · ops）",
            "旧 RSA 密钥（密钥 · admin）"
        ]
    );
    assert_eq!(
        label(cx, handle, "host-credential-new").as_deref(),
        Some("新建凭据…")
    );
    click(cx, handle, ("host-credential-option", 1usize));
    assert_eq!(
        label(cx, handle, "host-credential").as_deref(),
        Some("部署密钥（密钥 · deploy）")
    );
    assert_eq!(
        label(cx, handle, "host-auth-hint").as_deref(),
        Some("用户名跟着凭据走：deploy")
    );
    assert_eq!(error_of(cx, handle, "host-credential"), None);
    save_host(cx, handle);

    assert!(!host_dialog_open(cx, handle));
    let host = saved_host(cx, &workspace, 7).expect("saved");
    assert_eq!(host.credential, Some(qishell_hosts::CredentialId(1)));
    assert_eq!(host.user.as_ref(), "deploy");
    // 凭据面板上的「N 台主机」跟着变。
    dispatch(cx, handle, ShowCredentials);
    assert_eq!(
        label(cx, handle, row(1)).as_deref(),
        Some("部署密钥 · 密钥 · ED25519 · deploy · 3 台主机")
    );

    // 编辑时停在「使用凭据」和选的那条。
    dispatch(cx, handle, qishell::app::EditHost(HostId(7)));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find(("host-auth", 1usize)).selected(), Some(true));
    });
    assert_eq!(
        label(cx, handle, "host-credential").as_deref(),
        Some("部署密钥（密钥 · deploy）")
    );
}

#[gpui_kit::test]
fn a_host_without_a_password_tries_the_agent_and_the_default_keys(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open(cx);
    dispatch(cx, handle, qishell::app::EditHost(HostId(2)));
    click(cx, handle, ("host-auth", 2usize));
    assert_eq!(
        label(cx, handle, "host-auth-hint").as_deref(),
        Some("依次尝试 SSH Agent 和 ~/.ssh 下的默认私钥")
    );
    assert!(!present(cx, handle, "host-password"));
    fill(cx, handle, "host-user", "admin");
    save_host(cx, handle);

    let host = saved_host(cx, &workspace, 2).unwrap();
    assert_eq!(host.auth, AuthKind::NoPassword);
    assert_eq!(host.credential, None);
    assert_eq!(host.user.as_ref(), "admin");

    // 用凭据的主机改回「密码」：不再用凭据，用户名照表单。
    dispatch(cx, handle, qishell::app::EditHost(HostId(1)));
    click(cx, handle, ("host-auth", 0usize));
    save_host(cx, handle);
    let web01 = saved_host(cx, &workspace, 1).unwrap();
    assert_eq!(web01.credential, None);
    assert_eq!(web01.auth, AuthKind::Password);
}

#[gpui_kit::test]
fn a_credential_made_on_top_of_the_host_dialog_is_picked_when_saved(cx: &mut TestAppContext) {
    let (handle, workspace, _) = open(cx);
    dispatch(cx, handle, qishell::app::NewHost);
    fill(cx, handle, "host-name", "api-03");
    fill(cx, handle, "host-address", "10.0.1.33");
    click(cx, handle, ("host-auth", 1usize));
    click(cx, handle, "host-credential");
    click(cx, handle, "host-credential-new");

    // 叠一层凭据对话框；Esc 只关上面这一层。
    assert!(host_dialog_open(cx, handle));
    assert!(credential_dialog_open(cx, handle));
    assert_eq!(
        label(cx, handle, "credential-dialog-subtitle").as_deref(),
        Some("保存后自动选上")
    );
    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    assert!(!credential_dialog_open(cx, handle));
    assert!(host_dialog_open(cx, handle));
    assert_eq!(label(cx, handle, "host-credential").as_deref(), Some(""));

    click(cx, handle, "host-credential");
    click(cx, handle, "host-credential-new");
    click(cx, handle, ("credential-kind", 2usize));
    fill(cx, handle, "credential-name", "办公室 Agent");
    fill(cx, handle, "credential-user", "ops");
    save_credential(cx, handle);

    assert!(!credential_dialog_open(cx, handle));
    assert!(host_dialog_open(cx, handle));
    assert_eq!(
        label(cx, handle, "host-credential").as_deref(),
        Some("办公室 Agent（SSH Agent · ops）")
    );
    save_host(cx, handle);
    assert!(!host_dialog_open(cx, handle));
    let host = saved_host(cx, &workspace, 7).expect("saved");
    assert_eq!(host.credential, Some(qishell_hosts::CredentialId(5)));
    assert_eq!(host.user.as_ref(), "ops");
}

// ---- 用凭据登录 ----

/// 最后一次连接用的登录。
fn last_login(remote: &FakeRemoteTerminals) -> qishell_hosts::HostLogin {
    remote
        .targets()
        .last()
        .map(|(_, target)| target.login.clone())
        .expect("connected")
}

#[gpui_kit::test]
fn a_host_connects_with_its_credential_unlocking_only_for_a_saved_key(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked();
    let (handle, workspace, remote) = open_with(cx, vault.services());
    let deploy = credential(cx, &workspace, 1).unwrap();
    vault
        .vault
        .set(&deploy.private_key_secret(), "-----BEGIN OPENSSH PRIVATE KEY-----")
        .unwrap();
    dispatch(cx, handle, qishell::app::LockVault);

    // Agent 凭据什么秘密都不读：不弹解锁，直接连。
    dispatch(cx, handle, qishell::app::ConnectHost(HostId(5)));
    assert!(!present(cx, handle, "vault-unlock"));
    let login = last_login(&remote);
    assert_eq!(login.method, qishell_hosts::LoginMethod::Agent);
    assert_eq!(login.user, "ops");

    // 密钥凭据的私钥存在锁着的保险库里：先解锁再连，用凭据的用户名和保险库里的私钥。
    dispatch(cx, handle, qishell::app::ConnectHost(HostId(3)));
    assert_eq!(
        label(cx, handle, "vault-unlock-reason").as_deref(),
        Some("连接 db-01 要用保存的凭据")
    );
    assert_eq!(remote.targets().len(), 1);
    fill(cx, handle, "vault-unlock-password", MASTER);
    click(cx, handle, "vault-unlock-confirm");
    assert!(!present(cx, handle, "vault-unlock"));
    let login = last_login(&remote);
    assert_eq!(login.method, qishell_hosts::LoginMethod::Key);
    assert_eq!(login.user, "deploy");
    assert_eq!(
        login.key,
        Some(qishell_hosts::SavedKey::new(deploy.secret_id.as_str()))
    );

    // 删了凭据，主机改成「无密码」：再连就依次试 Agent 和默认私钥，用户名保留。
    dispatch(cx, handle, qishell::app::DeleteCredential(deploy.id));
    click(cx, handle, "credential-delete-confirm");
    dispatch(cx, handle, qishell::app::ConnectHost(HostId(3)));
    let login = last_login(&remote);
    assert_eq!(login.method, qishell_hosts::LoginMethod::NoPassword);
    assert_eq!(login.user, "deploy");
}
