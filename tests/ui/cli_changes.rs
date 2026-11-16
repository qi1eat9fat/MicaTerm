//! 外部 CLI 改主机和凭据（Agent CLI 工单 04）：命令那一端发请求，服务排着，窗口壳定时取来照表单那样
//! 做，秘密写进保险库，写完回话。在临时端点上真的听；请求线程要等窗口壳来取，所以在另一条线程上
//! 发请求，这边推着时钟让窗口壳跑。

use std::path::{Path, PathBuf};
use std::time::Duration;

use micaterm::cli::{
    CliBackend, CliService, CliTarget, Console, CredentialDetails, ErrorCode, HostDetails,
    HostFields, Request, send_request,
};
use micaterm_ssh::ExecStream;

use crate::support::*;

/// 不连任何主机的后端：这里只测改动。
struct NoBackend;

impl CliBackend for NoBackend {
    fn exec(
        &self,
        _: &CliTarget,
        _: &str,
        _: &mut dyn FnMut(ExecStream, &[u8]) -> std::io::Result<()>,
    ) -> Result<i32, micaterm::cli::CliError> {
        Ok(0)
    }

    fn upload(
        &self,
        _: &CliTarget,
        _: &std::path::Path,
        _: &str,
        _: &mut dyn FnMut(micaterm::cli::TransferCounters) -> std::io::Result<()>,
    ) -> Result<micaterm::cli::TransferSummary, micaterm::cli::CliError> {
        Ok(Default::default())
    }

    fn download(
        &self,
        _: &CliTarget,
        _: &str,
        _: &std::path::Path,
        _: &mut dyn FnMut(micaterm::cli::TransferCounters) -> std::io::Result<()>,
    ) -> Result<micaterm::cli::TransferSummary, micaterm::cli::CliError> {
        Ok(Default::default())
    }

    fn sync(
        &self,
        _: &CliTarget,
        _: &std::path::Path,
        _: &str,
        _: bool,
        _: &mut dyn FnMut(micaterm::cli::TransferCounters) -> std::io::Result<()>,
    ) -> Result<micaterm::cli::TransferSummary, micaterm::cli::CliError> {
        Ok(Default::default())
    }
}

/// 只给这个测试用的端点（Unix 上的套接字放在 `dir` 里）。
fn endpoint(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(
            r"\\.\pipe\micaterm-cli-changes-test-{}-{name}",
            std::process::id()
        ))
    } else {
        dir.path().join("cli.sock")
    }
}

/// 打开窗口（种子主机），外部 CLI 在 `endpoint` 上听、已启用。
fn open(
    cx: &mut TestAppContext,
    endpoint: &Path,
    services: WorkspaceServices,
) -> Entity<Workspace> {
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        cli: CliService::start_at(endpoint.to_path_buf(), Arc::new(NoBackend)),
        ..services
    };
    let (_, workspace) = open_workspace_with(cx, settings_store(), services);
    cx.update(|cx| {
        let settings = workspace.read(cx).settings().clone();
        settings.update(cx, |store, cx| {
            store.update(|settings| settings.cli.enabled = true, cx)
        })
    });
    cx.run_until_parked();
    workspace
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

fn fields(json: &str) -> HostFields {
    serde_json::from_str(json).unwrap()
}

fn host_names(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Vec<String> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .hosts()
            .read(cx)
            .hosts()
            .iter()
            .map(|host| host.name.to_string())
            .collect()
    })
}

#[gpui_kit::test]
fn a_host_is_created_changed_and_deleted_and_its_password_goes_to_the_vault(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir, "hosts");
    let vault = TestVault::unprotected();
    let workspace = open(cx, &endpoint, vault.services());

    let (code, out) = ask(
        cx,
        &endpoint,
        Request::CreateHost {
            fields: fields(r#"{"name": "cache", "host": "10.0.3.7", "password": "hunter2"}"#),
        },
    );
    assert_eq!(code, 0, "{out}");
    let created: HostDetails = serde_json::from_str(&out).unwrap();
    assert_eq!(created.password_saved, Some(true));
    assert!(host_names(cx, &workspace).contains(&"cache".to_string()));
    let secret = cx.update(|cx| {
        let store = workspace.read(cx).hosts().read(cx);
        store
            .hosts()
            .iter()
            .find(|host| host.name.as_ref() == "cache")
            .unwrap()
            .password_secret()
    });
    assert_eq!(vault.read(&secret).as_deref(), Some("hunter2"));

    // 只改给出的字段；ID 不变。
    let (code, out) = ask(
        cx,
        &endpoint,
        Request::UpdateHost {
            host: created.id.clone(),
            fields: fields(r#"{"port": 2222}"#),
        },
    );
    assert_eq!(code, 0, "{out}");
    let updated: HostDetails = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (updated.id.as_str(), updated.port),
        (created.id.as_str(), 2222)
    );
    assert_eq!(updated.host, "10.0.3.7");

    // 表单的校验照样管：地址不能空。
    let (code, out) = ask(
        cx,
        &endpoint,
        Request::UpdateHost {
            host: created.id.clone(),
            fields: fields(r#"{"host": " "}"#),
        },
    );
    assert_eq!(code, micaterm::cli::FAILURE_EXIT);
    assert!(out.contains("[bad_request]"), "{out}");

    let (code, out) = ask(cx, &endpoint, Request::DeleteHost { host: created.id });
    assert_eq!(code, 0, "{out}");
    assert!(!host_names(cx, &workspace).contains(&"cache".to_string()));
}

#[gpui_kit::test]
fn a_locked_vault_refuses_a_change_that_writes_a_secret_and_changes_nothing(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir, "locked");
    let vault = TestVault::unlocked().locked();
    let workspace = open(cx, &endpoint, vault.services());
    let before = host_names(cx, &workspace);

    let (code, out) = ask(
        cx,
        &endpoint,
        Request::CreateHost {
            fields: fields(r#"{"host": "10.0.3.8", "password": "x"}"#),
        },
    );
    assert_eq!(code, micaterm::cli::FAILURE_EXIT);
    assert!(out.contains(ErrorCode::VaultLocked.as_str()), "{out}");
    assert_eq!(host_names(cx, &workspace), before, "整个改动不做");

    // 不写秘密的改动照做。
    let (code, out) = ask(
        cx,
        &endpoint,
        Request::CreateHost {
            fields: fields(r#"{"host": "10.0.3.9", "auth": "no_password"}"#),
        },
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(host_names(cx, &workspace).len(), before.len() + 1);
}

#[gpui_kit::test]
fn a_credential_is_made_hosts_use_it_and_deleting_it_sets_them_free(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir, "credentials");
    let vault = TestVault::unprotected();
    let workspace = open(cx, &endpoint, vault.services());

    let (code, out) = ask(
        cx,
        &endpoint,
        Request::CreateCredential {
            fields: serde_json::from_str(r#"{"name": "部署", "user": "deploy", "password": "pw"}"#)
                .unwrap(),
        },
    );
    assert_eq!(code, 0, "{out}");
    let credential: CredentialDetails = serde_json::from_str(&out).unwrap();
    assert_eq!(credential.password_saved, Some(true));

    // 列表里有它。
    let (code, out) = ask(cx, &endpoint, Request::ListCredentials { query: None });
    assert_eq!(code, 0);
    let listed: Vec<CredentialDetails> = serde_json::from_str(&out).unwrap();
    assert_eq!(listed.len(), 1);

    // web-01 改用它：用户名跟凭据。
    let web = cx.update(|cx| {
        workspace.read(cx).hosts().read(cx).hosts()[0]
            .public_id
            .to_string()
    });
    let (code, out) = ask(
        cx,
        &endpoint,
        Request::UpdateHost {
            host: web.clone(),
            fields: fields(&format!(r#"{{"credential": "{}"}}"#, credential.id)),
        },
    );
    assert_eq!(code, 0, "{out}");
    let host: HostDetails = serde_json::from_str(&out).unwrap();
    assert_eq!(host.user, "deploy");

    // 删掉凭据：web-01 改回自己用密码登录，回话里列出它。
    let (code, out) = ask(
        cx,
        &endpoint,
        Request::DeleteCredential {
            credential: credential.id,
        },
    );
    assert_eq!(code, 0, "{out}");
    let deleted: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(deleted["released"][0]["id"], web.as_str());
    let auth = cx.update(|cx| workspace.read(cx).hosts().read(cx).hosts()[0].credential);
    assert_eq!(auth, None);
}
