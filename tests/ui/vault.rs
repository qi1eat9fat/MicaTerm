//! 保险库界面（工单 13）的 UI 测试：第一次保存密码时设主密码或不设置、保险库锁着时的密码框、
//! 用到秘密时才解锁（每次运行最多一次，无密码不弹）、主机栏底部的锁、设置 › 安全（更换主密码、
//! 在本机记住、立即锁定、重置），以及拷到别的电脑、没设主密码时的「这里打不开」。
//!
//! 保险库是真的（临时目录 + 进程内钥匙串，`support/vault.rs`），远程终端是假的。

use qishell::app::{ConnectHost, EditHost, NewHost, OpenSettings};

use crate::support::*;

/// 主机栏的第一台主机 web-01 的密码条目。
fn web01_password() -> SecretRef {
    SecretRef::password(1)
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

fn label(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &'static str) -> Option<String> {
    in_frame(cx, handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.label().map(str::to_owned))
    })
}

fn click(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &'static str) {
    in_frame(cx, handle, move |window, cx| window.click(id, cx));
}

/// 把字段 `field` 的内容换成 `text`。
fn fill(cx: &mut TestAppContext, handle: WindowHandle<Root>, field: &'static str, text: &str) {
    in_frame(cx, handle, move |window, cx| window.click(field, cx));
    in_frame(cx, handle, |window, cx| {
        window.press("ctrl-a", cx);
        window.press("backspace", cx);
        if !text.is_empty() {
            window.input(text, cx);
        }
    });
}

/// 新建一台主机，填上地址和密码，点「保存」。
fn save_new_host_with_password(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    dispatch(cx, handle, NewHost);
    fill(cx, handle, "host-address", "10.0.1.33");
    fill(cx, handle, "host-password", "hunter2");
    click(cx, handle, "host-dialog-save");
}

fn vault_status(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Option<VaultStatus> {
    cx.update(|cx| workspace.read(cx).vault().read(cx).status())
}

fn connected_hosts(remote: &FakeRemoteTerminals) -> Vec<u64> {
    remote
        .targets()
        .into_iter()
        .map(|(_, target)| target.host)
        .collect()
}

#[gpui_kit::test]
fn the_first_saved_password_asks_for_a_master_password(cx: &mut TestAppContext) {
    let vault = TestVault::absent();
    let (handle, workspace, _) = open_with_vault(cx, &vault);
    dispatch(cx, handle, NewHost);
    // 还没建保险库：密码框下提示第一次保存时会问。
    assert_eq!(
        label(cx, handle, "host-password-hint").as_deref(),
        Some("第一次保存秘密时会问你要不要设主密码，可以跳过")
    );
    fill(cx, handle, "host-address", "10.0.1.33");
    fill(cx, handle, "host-password", "hunter2");
    click(cx, handle, "host-dialog-save");

    assert_eq!(
        label(cx, handle, "vault-password-dialog").as_deref(),
        Some("设置主密码")
    );
    // 「在本机记住」默认勾选；主机对话框还开着，主机还没存。
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("vault-remember").checked(), Some(true));
        assert!(window.find("vault-password-skip").visible());
        assert!(window.try_find("host-dialog").is_some());
    });
    assert_eq!(host_names(cx, &workspace).len(), 6);

    fill(cx, handle, "vault-master-password", MASTER);
    fill(cx, handle, "vault-master-password-again", MASTER);
    click(cx, handle, "vault-password-confirm");

    assert!(!visible(cx, handle, "vault-password-dialog"));
    assert!(!visible(cx, handle, "host-dialog"));
    assert_eq!(host_names(cx, &workspace).last().unwrap(), "10.0.1.33");
    assert_eq!(vault.status(), VaultStatus::Unlocked { remembered: true });
    assert_eq!(
        vault.read(&SecretRef::password(7)).as_deref(),
        Some("hunter2")
    );
    // 记住了：钥匙串里有数据密钥，下次启动自动解锁。
    assert_eq!(
        vault.reopened().status(),
        VaultStatus::Unlocked { remembered: true }
    );
    // 设了主密码，主机栏底部出现锁定按钮。
    assert!(visible(cx, handle, "lock-vault"));
    assert_eq!(
        vault_status(cx, &workspace),
        Some(VaultStatus::Unlocked { remembered: true })
    );
}

#[gpui_kit::test]
fn not_setting_a_master_password_still_saves_the_password(cx: &mut TestAppContext) {
    let vault = TestVault::absent();
    let (handle, workspace, _) = open_with_vault(cx, &vault);
    save_new_host_with_password(cx, handle);

    click(cx, handle, "vault-password-skip");

    assert!(!visible(cx, handle, "host-dialog"));
    assert_eq!(host_names(cx, &workspace).len(), 7);
    assert_eq!(vault.status(), VaultStatus::Unprotected);
    assert_eq!(
        vault.read(&SecretRef::password(7)).as_deref(),
        Some("hunter2")
    );
    // 没设主密码：没有锁定这回事，主机栏底部没有锁。
    assert!(!visible(cx, handle, "lock-vault"));
    assert!(!visible(cx, handle, "unlock-vault"));

    // 之后再存密码不再问。
    dispatch(cx, handle, EditHost(qishell_hosts::HostId(1)));
    fill(cx, handle, "host-password", "hunter3");
    click(cx, handle, "host-dialog-save");
    assert!(!visible(cx, handle, "vault-password-dialog"));
    assert!(!visible(cx, handle, "host-dialog"));
    assert_eq!(vault.read(&web01_password()).as_deref(), Some("hunter3"));
}

#[gpui_kit::test]
fn cancelling_the_master_password_keeps_the_host_dialog_open(cx: &mut TestAppContext) {
    let vault = TestVault::absent();
    let (handle, workspace, _) = open_with_vault(cx, &vault);
    save_new_host_with_password(cx, handle);

    // 两次不一样：再输一次那一栏描红，不设。
    fill(cx, handle, "vault-master-password", MASTER);
    fill(cx, handle, "vault-master-password-again", "wrong horse");
    click(cx, handle, "vault-password-confirm");
    assert_eq!(
        label(cx, handle, "vault-master-password-again-error").as_deref(),
        Some("两次输入的主密码不一样")
    );
    assert_eq!(vault.status(), VaultStatus::Absent);

    click(cx, handle, "vault-password-cancel");
    assert!(!visible(cx, handle, "vault-password-dialog"));
    assert!(visible(cx, handle, "host-dialog"));
    assert_eq!(host_names(cx, &workspace).len(), 6);
    assert_eq!(vault.status(), VaultStatus::Absent);
}

#[gpui_kit::test]
fn a_saved_password_shows_as_locked_until_the_vault_is_unlocked(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked()
        .with_secret(&web01_password(), "hunter2")
        .locked();
    let (handle, workspace, _) = open_with_vault(cx, &vault);
    // 锁着：主机栏底部是锁着的锁。
    assert!(visible(cx, handle, "unlock-vault"));

    dispatch(cx, handle, EditHost(qishell_hosts::HostId(1)));
    assert_eq!(
        label(cx, handle, "host-password-locked").as_deref(),
        Some("已保存，保险库锁定中 · 解锁后修改")
    );
    assert!(!visible(cx, handle, "host-password"));

    click(cx, handle, "host-password-unlock");
    assert!(visible(cx, handle, "vault-unlock"));
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("vault-unlock-remember").checked(), Some(true));
    });
    // 主密码不对：描红，下面写原因，还锁着。
    fill(cx, handle, "vault-unlock-password", "wrong horse");
    click(cx, handle, "vault-unlock-confirm");
    assert_eq!(
        label(cx, handle, "vault-unlock-password-error").as_deref(),
        Some("主密码不对")
    );
    assert_eq!(vault.status(), VaultStatus::Locked);

    fill(cx, handle, "vault-unlock-password", MASTER);
    click(cx, handle, "vault-unlock-confirm");
    assert!(!visible(cx, handle, "vault-unlock"));
    assert_eq!(vault.status(), VaultStatus::Unlocked { remembered: true });
    // 解开后换回密码框，可以改了。
    assert!(!visible(cx, handle, "host-password-locked"));
    assert!(visible(cx, handle, "host-password"));
    assert!(visible(cx, handle, "lock-vault"));
    fill(cx, handle, "host-password", "hunter3");
    click(cx, handle, "host-dialog-save");
    assert!(!visible(cx, handle, "host-dialog"));
    assert_eq!(vault.read(&web01_password()).as_deref(), Some("hunter3"));
    assert_eq!(host_names(cx, &workspace).len(), 6);
}

#[gpui_kit::test]
fn saving_a_host_while_locked_leaves_its_saved_password_alone(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked()
        .with_secret(&web01_password(), "hunter2")
        .locked();
    let (handle, workspace, _) = open_with_vault(cx, &vault);

    dispatch(cx, handle, EditHost(qishell_hosts::HostId(1)));
    fill(cx, handle, "host-name", "web-01a");
    click(cx, handle, "host-dialog-save");

    // 不用解锁就能存：密码没动。
    assert!(!visible(cx, handle, "vault-unlock"));
    assert!(!visible(cx, handle, "host-dialog"));
    assert_eq!(host_names(cx, &workspace)[0], "web-01a");
    vault.vault.unlock(MASTER, false).unwrap();
    assert_eq!(vault.read(&web01_password()).as_deref(), Some("hunter2"));
}

#[gpui_kit::test]
fn connecting_unlocks_only_when_a_saved_secret_is_needed(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked()
        .with_secret(&web01_password(), "hunter2")
        .locked();
    let (handle, _, remote) = open_with_vault(cx, &vault);

    // web-02 没存密码：不弹，直接连（现场问密码）。
    dispatch(cx, handle, ConnectHost(qishell_hosts::HostId(2)));
    assert!(!visible(cx, handle, "vault-unlock"));
    assert_eq!(connected_hosts(&remote), vec![2]);

    // web-01 存了密码：先解锁再连。
    dispatch(cx, handle, ConnectHost(qishell_hosts::HostId(1)));
    assert!(visible(cx, handle, "vault-unlock"));
    assert_eq!(
        label(cx, handle, "vault-unlock-reason").as_deref(),
        Some("连接 web-01 要用保存的密码")
    );
    assert_eq!(connected_hosts(&remote), vec![2]);
    fill(cx, handle, "vault-unlock-password", MASTER);
    click(cx, handle, "vault-unlock-confirm");
    assert!(!visible(cx, handle, "vault-unlock"));
    assert_eq!(connected_hosts(&remote), vec![2, 1]);
    assert_eq!(vault.read(&web01_password()).as_deref(), Some("hunter2"));

    // 解开了：再连不弹。
    dispatch(cx, handle, ConnectHost(qishell_hosts::HostId(1)));
    assert!(!visible(cx, handle, "vault-unlock"));
    assert_eq!(connected_hosts(&remote), vec![2, 1, 1]);
}

#[gpui_kit::test]
fn cancelling_the_unlock_connects_anyway_and_does_not_ask_again(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked()
        .with_secret(&web01_password(), "hunter2")
        .locked();
    let (handle, _, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(qishell_hosts::HostId(1)));
    click(cx, handle, "vault-unlock-cancel");
    // 取消了照样连，密码现场问。
    assert!(!visible(cx, handle, "vault-unlock"));
    assert_eq!(connected_hosts(&remote), vec![1]);
    assert_eq!(vault.status(), VaultStatus::Locked);

    // 每次运行最多问一次。
    dispatch(cx, handle, ConnectHost(qishell_hosts::HostId(1)));
    assert!(!visible(cx, handle, "vault-unlock"));
    assert_eq!(connected_hosts(&remote), vec![1, 1]);

    // 手动「解锁…」一直可用。
    click(cx, handle, "unlock-vault");
    assert!(visible(cx, handle, "vault-unlock"));
    assert_eq!(
        label(cx, handle, "vault-unlock-reason").as_deref(),
        Some("输入主密码解锁保险库")
    );
}

#[gpui_kit::test]
fn escape_on_the_unlock_card_is_a_cancel(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked()
        .with_secret(&web01_password(), "hunter2")
        .locked();
    let (handle, _, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(qishell_hosts::HostId(1)));
    in_frame(cx, handle, |window, cx| window.press("escape", cx));
    assert!(!visible(cx, handle, "vault-unlock"));
    assert_eq!(connected_hosts(&remote), vec![1]);
}

#[gpui_kit::test]
fn the_lock_button_locks_and_the_next_use_asks_again(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked().with_secret(&web01_password(), "hunter2");
    let (handle, _, remote) = open_with_vault(cx, &vault);
    assert_eq!(
        label(cx, handle, "lock-vault").as_deref(),
        Some("锁定保险库")
    );

    click(cx, handle, "lock-vault");
    assert_eq!(vault.status(), VaultStatus::Locked);
    assert!(!visible(cx, handle, "lock-vault"));
    assert_eq!(
        label(cx, handle, "unlock-vault").as_deref(),
        Some("解锁保险库")
    );

    dispatch(cx, handle, ConnectHost(qishell_hosts::HostId(1)));
    assert!(visible(cx, handle, "vault-unlock"));
    assert!(connected_hosts(&remote).is_empty());
}

#[gpui_kit::test]
fn a_new_password_while_locked_unlocks_before_it_is_saved(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked().locked();
    let (handle, workspace, _) = open_with_vault(cx, &vault);
    save_new_host_with_password(cx, handle);

    assert!(visible(cx, handle, "vault-unlock"));
    assert_eq!(host_names(cx, &workspace).len(), 6);
    fill(cx, handle, "vault-unlock-password", MASTER);
    click(cx, handle, "vault-unlock-confirm");

    assert!(!visible(cx, handle, "host-dialog"));
    assert_eq!(host_names(cx, &workspace).len(), 7);
    assert_eq!(
        vault.read(&SecretRef::password(7)).as_deref(),
        Some("hunter2")
    );
}

fn open_security(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    dispatch(cx, handle, OpenSettings);
    in_frame(cx, handle, |window, cx| {
        assert_eq!(
            window.find(("settings-category", 2_usize)).label(),
            Some("安全")
        );
        window.click(("settings-category", 2_usize), cx);
    });
}

#[gpui_kit::test]
fn settings_change_the_master_password(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked().with_secret(&web01_password(), "hunter2");
    let (handle, _, _) = open_with_vault(cx, &vault);
    open_security(cx, handle);
    in_frame(cx, handle, |window, _| {
        for id in [
            "security-change-master",
            "security-remember",
            "security-lock",
            "security-reset",
        ] {
            assert!(window.find(id).visible(), "{id}");
        }
        assert_eq!(window.find("security-remember").checked(), Some(false));
    });

    click(cx, handle, "security-change-master");
    assert_eq!(
        label(cx, handle, "vault-password-dialog").as_deref(),
        Some("更换主密码")
    );
    fill(cx, handle, "vault-current-password", "wrong horse");
    fill(cx, handle, "vault-master-password", "battery staple");
    fill(cx, handle, "vault-master-password-again", "battery staple");
    click(cx, handle, "vault-password-confirm");
    assert_eq!(
        label(cx, handle, "vault-current-password-error").as_deref(),
        Some("主密码不对")
    );

    fill(cx, handle, "vault-current-password", MASTER);
    click(cx, handle, "vault-password-confirm");
    assert!(!visible(cx, handle, "vault-password-dialog"));
    // 设置对话框还开着。
    assert!(visible(cx, handle, "settings"));

    // 新的主密码能打开，旧的不行；存的秘密还在。
    let reopened = vault.reopened();
    assert!(reopened.unlock(MASTER, false).is_err());
    reopened.unlock("battery staple", false).unwrap();
    assert_eq!(
        qishell_hosts::SecretStore::get(&reopened, &web01_password())
            .unwrap()
            .as_deref()
            .map(String::as_str),
        Some("hunter2")
    );
}

#[gpui_kit::test]
fn settings_lock_and_remember(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked();
    let (handle, _, _) = open_with_vault(cx, &vault);
    open_security(cx, handle);

    in_frame(cx, handle, |window, cx| {
        window.click("security-remember", cx)
    });
    assert_eq!(vault.status(), VaultStatus::Unlocked { remembered: true });
    assert_eq!(vault.keychain.vault_ids().len(), 1);
    in_frame(cx, handle, |window, _| {
        assert_eq!(window.find("security-remember").checked(), Some(true));
    });

    click(cx, handle, "security-lock");
    assert_eq!(vault.status(), VaultStatus::Locked);
    // 锁着时那一行换成「解锁…」。
    assert!(visible(cx, handle, "security-unlock"));
    assert!(!visible(cx, handle, "security-lock"));
}

#[gpui_kit::test]
fn settings_set_a_master_password_later(cx: &mut TestAppContext) {
    let vault = TestVault::unprotected().with_secret(&web01_password(), "hunter2");
    let (handle, _, _) = open_with_vault(cx, &vault);
    open_security(cx, handle);
    // 没设主密码：没有锁定和在本机记住。
    assert!(!visible(cx, handle, "security-lock"));
    assert!(!visible(cx, handle, "security-remember"));

    click(cx, handle, "security-set-master");
    assert_eq!(
        label(cx, handle, "vault-password-dialog").as_deref(),
        Some("设置主密码")
    );
    // 从设置里打开没有「不设置」。
    assert!(!visible(cx, handle, "vault-password-skip"));
    fill(cx, handle, "vault-master-password", MASTER);
    fill(cx, handle, "vault-master-password-again", MASTER);
    click(cx, handle, "vault-password-confirm");

    assert_eq!(vault.status(), VaultStatus::Unlocked { remembered: true });
    assert!(visible(cx, handle, "security-lock"));
    // 已存的秘密照样能读。
    assert_eq!(vault.read(&web01_password()).as_deref(), Some("hunter2"));
}

#[gpui_kit::test]
fn settings_reset_the_vault_after_confirming(cx: &mut TestAppContext) {
    let vault = TestVault::unlocked().with_secret(&web01_password(), "hunter2");
    let (handle, workspace, _) = open_with_vault(cx, &vault);
    open_security(cx, handle);

    click(cx, handle, "security-reset");
    assert!(visible(cx, handle, "vault-reset"));
    click(cx, handle, "vault-reset-cancel");
    assert!(!visible(cx, handle, "vault-reset"));
    assert_eq!(vault.read(&web01_password()).as_deref(), Some("hunter2"));

    click(cx, handle, "security-reset");
    click(cx, handle, "vault-reset-confirm");
    assert!(!visible(cx, handle, "vault-reset"));
    assert_eq!(vault.status(), VaultStatus::Absent);
    assert_eq!(vault.read(&web01_password()), None);
    // 主机保留；主密码清掉了，锁按钮不见了。
    assert_eq!(host_names(cx, &workspace).len(), 6);
    assert!(!visible(cx, handle, "lock-vault"));
    assert!(visible(cx, handle, "security-set-master"));
}

#[gpui_kit::test]
fn a_vault_from_another_computer_asks_once_and_keeps_the_hosts(cx: &mut TestAppContext) {
    let vault = TestVault::unprotected()
        .with_secret(&web01_password(), "hunter2")
        .on_another_computer();
    assert_eq!(vault.status(), VaultStatus::KeyLost);
    let (handle, workspace, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(qishell_hosts::HostId(1)));
    assert_eq!(
        label(cx, handle, "vault-key-lost-text").as_deref(),
        Some(
            "这个保险库在另一台电脑上创建、没设主密码，这里打不开。重置后清空所有保存的密码、口令和私钥，主机和凭据都保留；不重置就这次现场输入密码。"
        )
    );
    // 不重置：这次现场询问密码，照样连。
    click(cx, handle, "vault-key-lost-cancel");
    assert_eq!(connected_hosts(&remote), vec![1]);
    assert_eq!(vault.status(), VaultStatus::KeyLost);

    // 这次运行不再问。
    dispatch(cx, handle, ConnectHost(qishell_hosts::HostId(1)));
    assert!(!visible(cx, handle, "vault-key-lost"));
    assert_eq!(connected_hosts(&remote), vec![1, 1]);
    assert_eq!(host_names(cx, &workspace).len(), 6);
}

#[gpui_kit::test]
fn resetting_a_vault_from_another_computer_lets_passwords_be_saved_again(cx: &mut TestAppContext) {
    let vault = TestVault::unprotected()
        .with_secret(&web01_password(), "hunter2")
        .on_another_computer();
    let (handle, workspace, remote) = open_with_vault(cx, &vault);

    dispatch(cx, handle, ConnectHost(qishell_hosts::HostId(1)));
    click(cx, handle, "vault-key-lost-confirm");
    assert!(!visible(cx, handle, "vault-key-lost"));
    assert_eq!(connected_hosts(&remote), vec![1]);
    assert_eq!(vault.status(), VaultStatus::Absent);
    assert_eq!(host_names(cx, &workspace).len(), 6);

    // 重置后第一次保存密码，照常问要不要设主密码。
    save_new_host_with_password(cx, handle);
    assert!(visible(cx, handle, "vault-password-dialog"));
    click(cx, handle, "vault-password-skip");
    assert_eq!(vault.status(), VaultStatus::Unprotected);
    assert_eq!(
        vault.read(&SecretRef::password(7)).as_deref(),
        Some("hunter2")
    );
}
