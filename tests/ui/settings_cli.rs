//! 设置 › Agent（Agent CLI 工单 01）：「启用外部 Agent」开关，端点的状态说明；
//! 开关和主机的变化交到在听的服务上（在临时端点上真的听，命令那一端发请求核对）；
//! 保险库锁着挡下 `exec` 时弹提醒（工单 02）；命令行工具和 Agent Skills 的安装行（工单 05，
//! 用临时目录当用户目录和程序目录，不碰真的）。

use std::path::{Path, PathBuf};

use std::time::Duration;

use micaterm::app::OpenSettings;
use micaterm::cli::{CliBackend, CliService, CliTarget, Console, ErrorCode, Request, send_request};
use micaterm_ssh::ExecStream;

use crate::support::*;

/// 设置里「Agent」在左栏的位置。
const CLI_CATEGORY: usize = 4;

fn open_category(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    dispatch(cx, handle, OpenSettings);
    in_frame(cx, handle, |window, cx| {
        window.click(("settings-category", CLI_CATEGORY), cx)
    });
}

/// 只给这个测试用的端点（Unix 上的套接字放在 `dir` 里）。
fn endpoint(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(
            r"\\.\pipe\micaterm-cli-ui-test-{}-{name}",
            std::process::id()
        ))
    } else {
        dir.path().join("cli.sock")
    }
}

/// 假的后端：命令原样回到 stdout，退出码 0。不连任何主机。
pub(crate) struct EchoBackend;

impl CliBackend for EchoBackend {
    fn exec(
        &self,
        _: &CliTarget,
        command: &str,
        output: &mut dyn FnMut(ExecStream, &[u8]) -> std::io::Result<()>,
    ) -> Result<i32, micaterm::cli::CliError> {
        let _ = output(ExecStream::Stdout, command.as_bytes());
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

/// 用命令那一端发一个请求：退出码和 stdout。
fn ask(endpoint: &Path, request: Request) -> (i32, String) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = send_request(
        endpoint,
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
}

#[gpui_kit::test]
fn the_cli_page_has_the_switch_off_and_says_why_there_is_no_endpoint(cx: &mut TestAppContext) {
    let (handle, workspace) = open_workspace(cx);
    open_category(cx, handle);
    in_frame(cx, handle, |window, cx| {
        assert_eq!(
            window.find(("settings-category", CLI_CATEGORY)).label(),
            Some("Agent")
        );
        assert_eq!(window.find("cli-enabled").checked(), Some(false));
        // 测试里没有端点：说这次运行用不了。
        assert_eq!(
            window.find("cli-status").label(),
            Some("No data folder this session; the external CLI is unavailable")
        );
        window.click("cli-enabled", cx);
    });
    let enabled = cx.update(|cx| {
        workspace
            .read(cx)
            .settings()
            .read(cx)
            .settings()
            .cli
            .enabled
    });
    assert!(enabled);
}

#[gpui_kit::test]
fn the_switch_and_the_hosts_reach_the_listening_cli(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir, "wiring");
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        cli: CliService::start_at(endpoint.clone(), Arc::new(EchoBackend)),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);

    // 开关关着：说没启用。
    let (code, out) = ask(&endpoint, Request::List { query: None });
    assert_eq!(code, micaterm::cli::FAILURE_EXIT);
    assert!(out.contains("[not_enabled]"), "{out}");

    open_category(cx, handle);
    in_frame(cx, handle, |window, cx| {
        assert!(
            window
                .find("cli-status")
                .label()
                .is_some_and(|label| label.contains("micaterm hosts list"))
        );
        window.click("cli-enabled", cx);
    });
    cx.run_until_parked();

    let (code, out) = ask(&endpoint, Request::List { query: None });
    assert_eq!(code, 0, "{out}");
    let listed: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    let seeded = cx.update(|cx| workspace.read(cx).listed_hosts(cx).len());
    assert_eq!(listed.len(), seeded);

    // 删掉一台主机，CLI 里也跟着少一台。
    let first = cx.update(|cx| workspace.read(cx).listed_hosts(cx)[0]);
    cx.update(|cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .hosts()
                .update(cx, |store, cx| store.remove(first, cx));
        })
    });
    cx.run_until_parked();
    let (_, out) = ask(&endpoint, Request::List { query: None });
    let listed: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(listed.len(), seeded - 1);
}

#[gpui_kit::test]
fn a_locked_vault_holds_exec_back_and_asks_the_user_to_unlock(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = endpoint(&dir, "vault");
    // web-01（种子数据的 1 号）的密码在保险库里，保险库锁着。
    let vault = TestVault::unlocked()
        .with_secret(&SecretRef::password(1), "hunter2")
        .locked();
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        cli: CliService::start_at(endpoint.clone(), Arc::new(EchoBackend)),
        ..vault.services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    cx.update(|cx| {
        let settings = workspace.read(cx).settings().clone();
        settings.update(cx, |store, cx| {
            store.update(|settings| settings.cli.enabled = true, cx)
        })
    });
    cx.run_until_parked();
    let web01 = cx.update(|cx| {
        workspace
            .read(cx)
            .hosts()
            .read(cx)
            .host(micaterm_hosts::HostId(1))
            .unwrap()
            .public_id
            .to_string()
    });

    let (code, out) = ask(
        &endpoint,
        Request::Exec {
            host: web01.clone(),
            command: "id".into(),
        },
    );
    assert_eq!(code, micaterm::cli::FAILURE_EXIT);
    assert!(out.contains(ErrorCode::VaultLocked.as_str()), "{out}");

    // 窗口壳定时来问，弹一条提醒，不弹解锁框。
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    in_frame(cx, handle, |window, cx| {
        assert_eq!(window.notifications(cx).len(), 1);
        assert!(!window.has_active_dialog(cx));
    });

    // 解锁以后照常执行。
    cx.update(|cx| {
        let vault = workspace.read(cx).vault().clone();
        vault.update(cx, |store, cx| {
            store
                .unlock(zeroize::Zeroizing::new(MASTER.to_string()), false, cx)
                .detach()
        })
    });
    cx.run_until_parked();
    let (code, out) = ask(
        &endpoint,
        Request::Exec {
            host: web01,
            command: "id".into(),
        },
    );
    assert_eq!((code, out.as_str()), (0, "id"));
}

/// 一个假的程序目录和用户目录：程序旁边有 `micaterm-cli`，PATH 记在文件里。
pub(crate) fn install_paths(root: &Path) -> micaterm::cli::IntegrationPaths {
    let exe = root.join("app").join("micaterm-cli.exe");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "build 1").unwrap();
    micaterm::cli::IntegrationPaths {
        home: root.join("home"),
        bin_link: root.join("app").join("bin").join("micaterm.exe"),
        exe,
        user_path: Some(micaterm::cli::UserPath::File(root.join("PATH.txt"))),
    }
}

#[gpui_kit::test]
fn the_command_and_a_skill_are_installed_and_removed_from_the_page(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let paths = install_paths(root.path());
    let services = WorkspaceServices {
        cli_install: Some(paths.clone()),
        ..fake_services()
    };
    let (handle, _) = open_workspace_with(cx, settings_store(), services);
    open_category(cx, handle);
    cx.run_until_parked();

    in_frame(cx, handle, |window, cx| {
        let note = window
            .find("cli-binary-status")
            .label()
            .unwrap_or_default()
            .to_string();
        assert!(note.contains("Not installed"), "{note}");
        window.click("cli-binary", cx);
    });
    cx.run_until_parked();
    assert!(paths.bin_link.exists() || std::fs::symlink_metadata(&paths.bin_link).is_ok());
    in_frame(cx, handle, |window, cx| {
        let note = window
            .find("cli-binary-status")
            .label()
            .unwrap_or_default()
            .to_string();
        assert!(note.contains("Installed"), "{note}");
        // Claude Code 的 Skill：装上，内容就是这一版的。
        window.click("cli-skill-claude-install", cx);
    });
    cx.run_until_parked();
    let skill = paths
        .home
        .join(".claude")
        .join("skills")
        .join("micaterm")
        .join("SKILL.md");
    assert_eq!(
        std::fs::read_to_string(&skill).unwrap(),
        micaterm::cli::SKILL
    );
    in_frame(cx, handle, |window, cx| {
        assert_eq!(
            window.find("cli-skill-claude-status").label(),
            Some("Installed")
        );
        window.click("cli-skill-claude-remove", cx);
    });
    cx.run_until_parked();
    assert!(!skill.exists());
    in_frame(cx, handle, |window, cx| window.click("cli-binary", cx));
    cx.run_until_parked();
    assert!(std::fs::symlink_metadata(&paths.bin_link).is_err());
}

/// Antigravity CLI（2026-11-24，取代 WorkBuddy）：只装 Skill，装到
/// `~/.gemini/antigravity-cli/skills`；那一行说本机有没有它的命令行
/// （安装脚本放在 `%LOCALAPPDATA%\agy\bin` / `~/.local/bin` 的 `agy`）。
#[gpui_kit::test]
fn the_antigravity_skill_says_whether_the_cli_is_here(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let paths = install_paths(root.path());
    let services = WorkspaceServices {
        cli_install: Some(paths.clone()),
        ..fake_services()
    };
    let (handle, _) = open_workspace_with(cx, settings_store(), services);
    open_category(cx, handle);
    cx.run_until_parked();
    let note = |cx: &mut TestAppContext| {
        in_frame(cx, handle, |window, _| {
            window
                .find("cli-skill-antigravity-status")
                .label()
                .unwrap_or_default()
                .to_string()
        })
    };
    let before = note(cx);
    assert!(before.starts_with("Not Installed"), "{before}");
    assert!(before.contains("Antigravity CLI not detected"), "{before}");

    // 装了 Antigravity CLI 以后再装 Skill：写到它的 skills 目录，这一行说检测到了。
    let cli = if cfg!(windows) {
        paths
            .home
            .join("AppData")
            .join("Local")
            .join("agy")
            .join("bin")
            .join("agy.exe")
    } else {
        paths.home.join(".local").join("bin").join("agy")
    };
    std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
    std::fs::write(&cli, "").unwrap();
    in_frame(cx, handle, |window, cx| {
        window.click("cli-skill-antigravity-install", cx)
    });
    cx.run_until_parked();
    let skill = paths
        .home
        .join(".gemini")
        .join("antigravity-cli")
        .join("skills")
        .join("micaterm")
        .join("SKILL.md");
    assert_eq!(
        std::fs::read_to_string(&skill).unwrap(),
        micaterm::cli::SKILL
    );
    let after = note(cx);
    assert!(after.starts_with("Installed"), "{after}");
    assert!(after.contains("Antigravity CLI detected"), "{after}");
}

#[gpui_kit::test]
fn an_older_copy_of_the_command_is_brought_up_to_date_at_startup(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let paths = install_paths(root.path());
    if cfg!(not(windows)) {
        // macOS、Linux 上是链接，总是指向这一版：没有要更新的副本。
        return;
    }
    // 上一个版本装的副本，和一个旧版本的 Skill。
    std::fs::create_dir_all(paths.bin_link.parent().unwrap()).unwrap();
    // 长度不同：同一版靠大小加修改时间认，测试里两份几乎同时写下，修改时间可能一样。
    std::fs::write(&paths.bin_link, "build 0, an older one").unwrap();
    let skill = paths
        .home
        .join(".codex")
        .join("skills")
        .join("micaterm")
        .join("SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(&skill, "an older skill").unwrap();

    let services = WorkspaceServices {
        cli_install: Some(paths.clone()),
        ..fake_services()
    };
    open_workspace_with(cx, settings_store(), services);
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&paths.bin_link).unwrap(), "build 1");
    assert_eq!(
        std::fs::read_to_string(&skill).unwrap(),
        micaterm::cli::SKILL
    );
    // 没装过的不替用户装。
    assert!(!paths.home.join(".claude").exists());
}

/// 窗口窄时设置行的控件换到标题和说明下面（2026-11-16 用户）：不再把说明挤成一个字一行；
/// 宽窗口照旧放在右边。拿「Agent 命令」的输入框和页面标题比左边。
#[gpui_kit::test]
fn a_narrow_window_puts_the_control_under_its_title(cx: &mut TestAppContext) {
    let (handle, _workspace) = open_workspace(cx);
    open_category(cx, handle);
    let lefts = |cx: &mut TestAppContext| {
        in_frame(cx, handle, |window, _| {
            (
                window.find("settings-title").bounds().left(),
                window.find("agent-command").bounds().left(),
            )
        })
    };

    cx.simulate_window_resize(handle.into(), gpui_kit::size(px(1400.), px(800.)));
    cx.run_until_parked();
    let (title, input) = lefts(cx);
    assert!(
        input - title > px(200.),
        "宽窗口：在右边 {title:?} {input:?}"
    );

    cx.simulate_window_resize(handle.into(), gpui_kit::size(px(760.), px(800.)));
    cx.run_until_parked();
    let (title, input) = lefts(cx);
    assert!(
        (input - title).abs() < px(2.),
        "窄窗口：换到下面 {title:?} {input:?}"
    );
}
