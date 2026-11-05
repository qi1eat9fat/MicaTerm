// 保险库的行为测试：只经公开接口（`Vault` 的方法和它实现的 `SecretStore`）和磁盘上的 `vault.json`。
// 钥匙串用 `InMemoryDataKeyStore`；换一个新的空实例就是「拷到了另一台电脑」。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use super::{InMemoryDataKeyStore, KdfParams, Vault, VaultError, VaultStatus};
use crate::secrets::{SecretRef, SecretStore, SecretsLocked};

struct Computer {
    keychain: Arc<InMemoryDataKeyStore>,
}

impl Computer {
    fn new() -> Self {
        Self {
            keychain: Arc::new(InMemoryDataKeyStore::default()),
        }
    }

    fn open(&self, path: &Path) -> Vault {
        Vault::open(
            path.to_path_buf(),
            self.keychain.clone(),
            KdfParams::FAST_FOR_TESTS,
        )
        .unwrap()
    }
}

fn data_dir() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault.json");
    (dir, path)
}

fn read(vault: &Vault, secret: &SecretRef) -> Option<String> {
    vault
        .get(secret)
        .unwrap()
        .map(|value| value.as_str().to_string())
}

#[test]
fn without_a_master_password_secrets_open_on_this_computer() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    assert_eq!(vault.status(), VaultStatus::Absent);

    vault.set_up(None, false).unwrap();
    assert_eq!(vault.status(), VaultStatus::Unprotected);
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    assert_eq!(
        read(&vault, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );

    // 下次启动：钥匙串里有数据密钥，不用解锁。
    let again = computer.open(&path);
    assert_eq!(again.status(), VaultStatus::Unprotected);
    assert_eq!(
        read(&again, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );
    assert_eq!(read(&again, &SecretRef::password(2)), None);

    // 秘密进了 vault.json，但不是明文。
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("password:host-1"));
    assert!(!written.contains("hunter2"));
}

fn error_of(result: anyhow::Result<impl Sized>) -> anyhow::Error {
    match result {
        Ok(_) => panic!("expected a failure"),
        Err(error) => error,
    }
}

fn vault_error(result: anyhow::Result<impl Sized>) -> VaultError {
    error_of(result)
        .downcast_ref::<VaultError>()
        .cloned()
        .expect("a vault error")
}

/// 没设主密码的保险库拷到另一台电脑：那里的钥匙串没有数据密钥，存过的秘密打不开，
/// 但能认出来（界面据此提示「在另一台电脑上创建、没设主密码」），没存过的照常是「没有」。
#[test]
fn without_its_key_in_the_keychain_an_unprotected_vault_is_lost() {
    let (_dir, path) = data_dir();
    let vault = Computer::new().open(&path);
    vault.set_up(None, false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();

    let elsewhere = Computer::new().open(&path);
    assert_eq!(elsewhere.status(), VaultStatus::KeyLost);
    assert!(elsewhere.contains(&SecretRef::password(1)));
    assert_eq!(
        vault_error(elsewhere.get(&SecretRef::password(1))),
        VaultError::KeyLost
    );
    assert_eq!(read(&elsewhere, &SecretRef::password(2)), None);
    assert_eq!(
        vault_error(elsewhere.set(&SecretRef::password(2), "x")),
        VaultError::KeyLost
    );
}

/// 打不开的保险库，用户确认后重置：所有秘密清空，回到还没建保险库的样子，
/// 下次保存秘密时重新问要不要设主密码。
#[test]
fn resetting_a_lost_vault_empties_it_and_starts_over() {
    let (_dir, path) = data_dir();
    let home = Computer::new();
    let vault = home.open(&path);
    vault.set_up(None, false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();

    let elsewhere = Computer::new();
    let lost = elsewhere.open(&path);
    lost.reset().unwrap();
    assert_eq!(lost.status(), VaultStatus::Absent);
    assert!(!lost.contains(&SecretRef::password(1)));
    assert_eq!(read(&lost, &SecretRef::password(1)), None);
    assert_eq!(elsewhere.open(&path).status(), VaultStatus::Absent);

    lost.set_up(None, false).unwrap();
    lost.set(&SecretRef::password(1), "new one").unwrap();
    assert_eq!(
        read(&elsewhere.open(&path), &SecretRef::password(1)).as_deref(),
        Some("new one")
    );
}

/// 重置换一个新的保险库 id，并删掉本机钥匙串里旧的数据密钥，不留残余。
#[test]
fn resetting_forgets_the_old_key() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    vault.set_up(None, false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    let old_id = vault.vault_id().unwrap();
    assert_eq!(computer.keychain.vault_ids(), std::slice::from_ref(&old_id));

    vault.reset().unwrap();
    assert_eq!(vault.vault_id(), None);
    assert!(computer.keychain.vault_ids().is_empty());

    vault.set_up(None, false).unwrap();
    assert_ne!(vault.vault_id().unwrap(), old_id);
    assert_eq!(read(&vault, &SecretRef::password(1)), None);
}

fn is_locked(result: anyhow::Result<impl Sized>) -> bool {
    error_of(result).downcast_ref::<SecretsLocked>().is_some()
}

/// 设了主密码、没选「在本机记住」：下次启动是锁着的，读存过的秘密返回「已锁定」，
/// 错误的主密码打不开，正确的才行。
#[test]
fn a_master_password_locks_the_vault_until_it_is_typed() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    vault.set_up(Some("correct horse"), false).unwrap();
    assert_eq!(vault.status(), VaultStatus::Unlocked { remembered: false });
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    assert!(computer.keychain.vault_ids().is_empty());

    let again = computer.open(&path);
    assert_eq!(again.status(), VaultStatus::Locked);
    assert!(again.contains(&SecretRef::password(1)));
    assert!(is_locked(again.get(&SecretRef::password(1))));
    assert!(is_locked(again.set(&SecretRef::password(2), "x")));
    // 没存过的不必解锁就知道没有。
    assert_eq!(read(&again, &SecretRef::password(2)), None);

    assert_eq!(
        vault_error(again.unlock("wrong horse", false)),
        VaultError::WrongPassword
    );
    assert_eq!(again.status(), VaultStatus::Locked);
    again.unlock("correct horse", false).unwrap();
    assert_eq!(again.status(), VaultStatus::Unlocked { remembered: false });
    assert_eq!(
        read(&again, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );
}

/// 「在本机记住」把数据密钥（不是主密码）存进本机钥匙串：这台电脑上自动解锁；
/// 别的电脑上照样用主密码解锁，这正是保险库能跟着文件夹走的原因。
#[test]
fn a_remembered_vault_unlocks_by_itself_here_and_with_its_password_elsewhere() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    vault.set_up(Some("correct horse"), true).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    assert_eq!(vault.status(), VaultStatus::Unlocked { remembered: true });

    let again = computer.open(&path);
    assert_eq!(again.status(), VaultStatus::Unlocked { remembered: true });
    assert_eq!(
        read(&again, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );

    let elsewhere = Computer::new().open(&path);
    assert_eq!(elsewhere.status(), VaultStatus::Locked);
    elsewhere.unlock("correct horse", true).unwrap();
    assert_eq!(
        elsewhere.status(),
        VaultStatus::Unlocked { remembered: true }
    );
    assert_eq!(
        read(&elsewhere, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );
}

/// 解锁时不勾「在本机记住」，就把以前记住的那份从钥匙串里删掉。
#[test]
fn unlocking_without_remembering_forgets_the_key_here() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    vault.set_up(Some("correct horse"), true).unwrap();
    vault.lock().unwrap();
    vault.unlock("correct horse", false).unwrap();
    assert!(computer.keychain.vault_ids().is_empty());
    assert_eq!(computer.open(&path).status(), VaultStatus::Locked);
}

/// 手动锁定：丢掉内存里的数据密钥。没设主密码的保险库没有锁定这回事。
#[test]
fn locking_needs_a_master_password() {
    let (_dir, path) = data_dir();
    let vault = Computer::new().open(&path);
    vault.set_up(Some("correct horse"), true).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    vault.lock().unwrap();
    assert_eq!(vault.status(), VaultStatus::Locked);
    assert!(is_locked(vault.get(&SecretRef::password(1))));

    let (_other, unprotected_path) = data_dir();
    let unprotected = Computer::new().open(&unprotected_path);
    unprotected.set_up(None, false).unwrap();
    assert_eq!(
        vault_error(unprotected.lock()),
        VaultError::NoMasterPassword
    );
    assert_eq!(
        vault_error(unprotected.unlock("anything", false)),
        VaultError::NoMasterPassword
    );
}

fn written(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// 先不设主密码，之后补设：已存的秘密照样能读，而且没有重新加密（密文一字不变）；
/// 不勾「在本机记住」时钥匙串里那份删掉，从此只有主密码能打开，换到别的电脑也能打开。
#[test]
fn a_master_password_set_later_keeps_the_old_secrets() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    vault.set_up(None, false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    let before = written(&path)["secrets"].clone();

    vault.set_master_password("correct horse", false).unwrap();
    assert_eq!(vault.status(), VaultStatus::Unlocked { remembered: false });
    assert_eq!(
        read(&vault, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );
    assert_eq!(written(&path)["secrets"], before);
    assert!(computer.keychain.vault_ids().is_empty());
    assert_eq!(
        vault_error(vault.set_master_password("again", false)),
        VaultError::HasMasterPassword
    );

    let elsewhere = Computer::new().open(&path);
    assert_eq!(elsewhere.status(), VaultStatus::Locked);
    elsewhere.unlock("correct horse", false).unwrap();
    assert_eq!(
        read(&elsewhere, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );
}

/// 补设主密码时勾了「在本机记住」：这台电脑上照旧自动打开。
#[test]
fn a_master_password_set_later_can_stay_remembered_here() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    vault.set_up(None, false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    vault.set_master_password("correct horse", true).unwrap();

    let again = computer.open(&path);
    assert_eq!(again.status(), VaultStatus::Unlocked { remembered: true });
    assert_eq!(
        read(&again, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );
}

/// 更换主密码：要先给对现在的主密码；换完旧的打不开、新的打得开，秘密不重新加密，
/// 记住的那份（数据密钥没变）照样能用。
#[test]
fn changing_the_master_password_rewraps_the_same_key() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    vault.set_up(Some("correct horse"), true).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    let before = written(&path);

    assert_eq!(
        vault_error(vault.change_master_password("wrong horse", "battery staple")),
        VaultError::WrongPassword
    );
    vault
        .change_master_password("correct horse", "battery staple")
        .unwrap();
    let after = written(&path);
    assert_eq!(after["secrets"], before["secrets"]);
    assert_eq!(after["vault_id"], before["vault_id"]);
    assert_ne!(after["kdf"]["salt"], before["kdf"]["salt"]);

    let elsewhere = Computer::new().open(&path);
    assert_eq!(
        vault_error(elsewhere.unlock("correct horse", false)),
        VaultError::WrongPassword
    );
    elsewhere.unlock("battery staple", false).unwrap();
    assert_eq!(
        read(&elsewhere, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );

    let here = computer.open(&path);
    assert_eq!(here.status(), VaultStatus::Unlocked { remembered: true });
    assert_eq!(
        read(&here, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );
}

/// 锁着也能换主密码：核对通过就解锁了。没设主密码的保险库没有主密码可换。
#[test]
fn a_locked_vault_can_change_its_master_password() {
    let (_dir, path) = data_dir();
    let vault = Computer::new().open(&path);
    vault.set_up(Some("correct horse"), false).unwrap();
    vault.lock().unwrap();
    vault
        .change_master_password("correct horse", "battery staple")
        .unwrap();
    assert_eq!(vault.status(), VaultStatus::Unlocked { remembered: false });

    let (_other, unprotected_path) = data_dir();
    let unprotected = Computer::new().open(&unprotected_path);
    unprotected.set_up(None, false).unwrap();
    assert_eq!(
        vault_error(unprotected.change_master_password("a", "b")),
        VaultError::NoMasterPassword
    );
}

/// 文件里的字段：格式版本号、保险库 id、Argon2id 参数和 salt、被包裹的数据密钥、每条秘密各自的 nonce。
#[test]
fn the_file_says_its_format_and_how_it_is_keyed() {
    let (_dir, path) = data_dir();
    let vault = Computer::new().open(&path);
    vault.set_up(Some("correct horse"), false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    vault.set(&SecretRef::password(2), "hunter2").unwrap();

    let file = written(&path);
    assert_eq!(file["format"], 1);
    assert_eq!(file["vault_id"].as_str().unwrap().len(), 32);
    assert_eq!(file["kdf"]["algorithm"], "argon2id");
    assert_eq!(file["kdf"]["version"], 19);
    assert_eq!(file["kdf"]["memory_kib"], 8);
    assert_eq!(file["kdf"]["passes"], 1);
    assert_eq!(file["kdf"]["parallelism"], 1);
    assert_eq!(file["kdf"]["salt"].as_str().unwrap().len(), 32);
    assert_eq!(file["wrapped_key"]["nonce"].as_str().unwrap().len(), 48);
    let one = &file["secrets"]["password:host-1"];
    let two = &file["secrets"]["password:host-2"];
    // 同样的内容，每条一个 nonce，密文也不同。
    assert_ne!(one["nonce"], two["nonce"]);
    assert_ne!(one["sealed"], two["sealed"]);

    let (_other, unprotected_path) = data_dir();
    Computer::new()
        .open(&unprotected_path)
        .set_up(None, false)
        .unwrap();
    let unprotected = written(&unprotected_path);
    assert_eq!(unprotected["format"], 1);
    assert!(unprotected["kdf"].is_null());
    assert!(unprotected["wrapped_key"].is_null());
}

/// 更新版本写的保险库这个版本不碰：打开就报格式太新，不会误当成坏了去重置。
#[test]
fn a_vault_from_a_newer_version_is_refused() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    computer
        .open(&path)
        .set_up(Some("correct horse"), false)
        .unwrap();
    let mut file = written(&path);
    file["format"] = 2.into();
    std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();

    let opened = Vault::open(
        path.clone(),
        computer.keychain.clone(),
        KdfParams::FAST_FOR_TESTS,
    );
    assert_eq!(vault_error(opened), VaultError::TooNew(2));
}

/// 新版本加的字段，旧版本改秘密、换主密码时原样留着。
#[test]
fn fields_a_newer_version_added_survive_a_rewrite() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    computer
        .open(&path)
        .set_up(Some("correct horse"), false)
        .unwrap();
    let mut file = written(&path);
    file["later"] = serde_json::json!({ "kept": true });
    std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();

    let vault = computer.open(&path);
    vault.unlock("correct horse", false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    vault
        .change_master_password("correct horse", "battery staple")
        .unwrap();
    assert_eq!(written(&path)["later"], file["later"]);
}

/// 越界的 Argon2 开销（被改过的文件）在跑之前就拒绝，不会把内存耗尽。
#[test]
fn costs_out_of_bounds_are_refused_before_anything_runs() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    computer
        .open(&path)
        .set_up(Some("correct horse"), false)
        .unwrap();
    let mut file = written(&path);
    file["kdf"]["memory_kib"] = (4u64 << 30).into();
    std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
    let opened = Vault::open(
        path.clone(),
        computer.keychain.clone(),
        KdfParams::FAST_FOR_TESTS,
    );
    assert!(matches!(vault_error(opened), VaultError::Damaged(_)));
}

/// 密文绑在条目名上：把一台主机的密码挪给另一台，打不开。
#[test]
fn a_secret_moved_to_another_entry_does_not_open() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    vault.set_up(None, false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    let mut file = written(&path);
    file["secrets"]["password:host-2"] = file["secrets"]["password:host-1"].clone();
    std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();

    let vault = computer.open(&path);
    assert!(matches!(
        vault_error(vault.get(&SecretRef::password(2))),
        VaultError::Damaged(_)
    ));
}

/// 写入先写临时文件再改名：写完不留临时文件；写不成时 `vault.json` 和内存里的保险库都原样不变。
#[test]
fn writes_replace_the_file_in_one_step() {
    let (_dir, path) = data_dir();
    let vault = Computer::new().open(&path);
    vault.set_up(None, false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    let temporary = path.with_extension("json.tmp");
    assert!(!temporary.exists());
    let before = std::fs::read(&path).unwrap();

    // 让临时文件写不出来：同名的位置被一个目录占着。
    std::fs::create_dir(&temporary).unwrap();
    assert!(vault.set(&SecretRef::password(1), "changed").is_err());
    assert!(vault.set(&SecretRef::password(2), "new").is_err());
    assert!(vault.delete(&SecretRef::password(1)).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        read(&vault, &SecretRef::password(1)).as_deref(),
        Some("hunter2")
    );
    assert!(!vault.contains(&SecretRef::password(2)));

    std::fs::remove_dir(&temporary).unwrap();
    vault.set(&SecretRef::password(1), "changed").unwrap();
    assert_eq!(
        read(&vault, &SecretRef::password(1)).as_deref(),
        Some("changed")
    );
}

/// 删除不需要数据密钥：锁着时删主机，它的密码照样从保险库里删掉。
#[test]
fn deleting_works_while_locked() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    vault.set_up(Some("correct horse"), false).unwrap();
    vault.set(&SecretRef::password(1), "hunter2").unwrap();
    vault.lock().unwrap();
    vault.delete(&SecretRef::password(1)).unwrap();
    assert!(!vault.contains(&SecretRef::password(1)));
    assert!(!computer.open(&path).contains(&SecretRef::password(1)));
}

/// 还没建保险库时写秘密：明确说「还没建」，界面据此先问要不要设主密码。
#[test]
fn writing_before_the_vault_is_set_up_says_so() {
    let (_dir, path) = data_dir();
    let vault = Computer::new().open(&path);
    assert_eq!(
        vault_error(vault.set(&SecretRef::password(1), "hunter2")),
        VaultError::NotSetUp
    );
    assert!(vault.delete(&SecretRef::password(1)).is_ok());
    assert!(!path.exists());
    vault.set_up(None, false).unwrap();
    assert_eq!(
        vault_error(vault.set_up(None, false)),
        VaultError::AlreadySetUp
    );
}

/// 钥匙串里那份和文件对不上（被换过）：当作没有，不会拿错的钥匙去开。
#[test]
fn a_key_in_the_keychain_that_does_not_fit_is_ignored() {
    let (_dir, path) = data_dir();
    let computer = Computer::new();
    let vault = computer.open(&path);
    vault.set_up(Some("correct horse"), true).unwrap();
    let id = vault.vault_id().unwrap();
    use super::DataKeyStore as _;
    computer
        .keychain
        .set(&id, &super::DataKey::from_bytes([9; 32]))
        .unwrap();
    assert_eq!(computer.open(&path).status(), VaultStatus::Locked);
}
