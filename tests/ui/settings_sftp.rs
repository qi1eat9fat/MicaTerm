//! 设置 › SFTP（工单 25）：同时传输数量、本地默认打开目录、图标风格、显示隐藏文件，
//! 改动写进设置存储，并在 SFTP 标签上生效。
//! 引擎的并发数是进程全局量，并行的测试会互相改它，这里只断言设置值（`settings::apply` 交给引擎）。

use std::path::Path;
use std::time::Duration;

use qishell::app::{ConnectHost, OpenSettings, OpenSftp, ToggleHiddenFiles};
use qishell::settings::SftpSettings;
use qishell::sftp_tab::{CardSide, FileIconStyle, ShowHiddenFiles};
use qishell_terminal::TerminalLifecycle;

use crate::support::*;

/// 设置里「SFTP」在左栏的位置。
const SFTP_CATEGORY: usize = 2;

struct Fixture {
    handle: WindowHandle<Root>,
    workspace: Entity<Workspace>,
    local: FakeLocalDirectory,
}

fn open(cx: &mut TestAppContext) -> Fixture {
    let local = FakeLocalDirectory::default();
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        remote_terminals: Arc::new(FakeRemoteTerminals::default()),
        sftp: Arc::new(FakeSftpProvider::default()),
        local_files: Arc::new(local.clone()),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    Fixture {
        handle,
        workspace,
        local,
    }
}

fn open_sftp_settings(cx: &mut TestAppContext, fixture: &Fixture) {
    dispatch(cx, fixture.handle, OpenSettings);
    in_frame(cx, fixture.handle, |window, cx| {
        window.click(("settings-category", SFTP_CATEGORY), cx)
    });
}

fn close_settings(cx: &mut TestAppContext, fixture: &Fixture) {
    in_frame(cx, fixture.handle, |window, cx| {
        window.click("settings-close", cx)
    });
}

fn sftp_settings(cx: &mut TestAppContext, fixture: &Fixture) -> SftpSettings {
    cx.update(|cx| {
        fixture
            .workspace
            .read(cx)
            .settings()
            .read(cx)
            .settings()
            .sftp
    })
}

/// 工作线程在真线程上：按真实时间等，同时拨测试时钟（标签每 16ms 收一次事件）。
fn wait_until(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, handle, |_, _| {});
        if done(cx) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// 元素的无障碍值或文字。
fn value(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) -> Option<String> {
    in_frame(cx, fixture.handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.value().map(str::to_owned))
    })
}

fn label(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) -> Option<String> {
    in_frame(cx, fixture.handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.label().map(str::to_owned))
    })
}

/// 连上主机 `host`，打开它的 SFTP 标签，等本地一侧读完 `local`。
fn open_tab(cx: &mut TestAppContext, fixture: &Fixture, host: u64, card: u64, local: &str) {
    dispatch(cx, fixture.handle, ConnectHost(HostId(host)));
    let workspace = fixture.workspace.clone();
    assert!(
        wait_until(cx, fixture.handle, |cx| {
            cx.update(|cx| {
                workspace
                    .read(cx)
                    .card(CardId(card), cx)
                    .map(|card| card.read(cx).terminal().read(cx).lifecycle(cx))
            }) == Some(TerminalLifecycle::Running)
        }),
        "卡片没有连上"
    );
    dispatch(cx, fixture.handle, OpenSftp);
    let local = local.to_string();
    let listed = wait_until(cx, fixture.handle, |cx| {
        value(cx, fixture, "local-path").as_deref() == Some(local.as_str())
            && value(cx, fixture, "remote-path").as_deref() == Some("/home/tester")
    });
    assert!(
        listed,
        "没有读完：本地 {:?}，远程 {:?}",
        value(cx, fixture, "local-path"),
        value(cx, fixture, "remote-path")
    );
}

#[gpui_kit::test]
fn the_sftp_category_sits_between_terminal_and_security_with_its_defaults(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_sftp_settings(cx, &fixture);

    in_frame(cx, fixture.handle, |window, _| {
        assert_eq!(
            window.find(("settings-category", 1_usize)).label(),
            Some("终端")
        );
        let sftp = window.find(("settings-category", SFTP_CATEGORY));
        assert_eq!(sftp.label(), Some("SFTP"));
        assert_eq!(sftp.selected(), Some(true));
        assert_eq!(
            window.find(("settings-category", 3_usize)).label(),
            Some("安全")
        );

        assert_eq!(window.find("sftp-parallel").label(), Some("3"));
        // 默认的目录只显示，不写进设置；这里的路径来自注入的「下载」文件夹。
        let directory = window.find("sftp-local-directory");
        assert_eq!(directory.value(), Some(""));
        assert_eq!(
            directory.label(),
            Some(format!("本地默认打开目录，默认 {LOCAL_DOWNLOADS}").as_str())
        );
        // 输入框和「选择…」同一行，都在对话框里。
        let choose = window.find("sftp-local-directory-choose").bounds();
        let field = directory.bounds();
        assert!(field.size.width > px(150.), "{field:?}");
        assert!(field.right() <= choose.left());
        assert!(choose.right() <= window.find("settings").bounds().right());
        assert_eq!(
            window.find(("sftp-icon-style", 0_usize)).label(),
            Some("彩色")
        );
        assert_eq!(
            window.find(("sftp-icon-style", 0_usize)).selected(),
            Some(true)
        );
        assert_eq!(
            window.find(("sftp-icon-style", 1_usize)).label(),
            Some("单色")
        );
        assert_eq!(window.find("sftp-show-hidden-local").checked(), Some(false));
        assert_eq!(
            window.find("sftp-show-hidden-remote").checked(),
            Some(false)
        );
    });
    let settings = sftp_settings(cx, &fixture);
    assert_eq!(settings.parallel, 3);
    assert_eq!(settings.local_directory, None);
}

#[gpui_kit::test]
fn the_parallel_transfer_count_offers_one_to_ten_and_is_stored(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_sftp_settings(cx, &fixture);

    in_frame(cx, fixture.handle, |window, cx| {
        window.click("sftp-parallel", cx)
    });
    in_frame(cx, fixture.handle, |window, cx| {
        let mut menu = window.within("popup-menu");
        assert_eq!(menu.find(0_usize).label(), Some("1"));
        assert_eq!(menu.find(9_usize).label(), Some("10"));
        assert!(menu.try_find(10_usize).is_none());
        menu.click(4_usize, cx);
    });

    assert_eq!(sftp_settings(cx, &fixture).parallel, 5);
    in_frame(cx, fixture.handle, |window, _| {
        assert_eq!(window.find("sftp-parallel").label(), Some("5"));
    });
}

#[gpui_kit::test]
fn a_chosen_local_directory_is_where_new_sftp_tabs_start(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_sftp_settings(cx, &fixture);

    in_frame(cx, fixture.handle, |window, cx| {
        window.click("sftp-local-directory-choose", cx)
    });
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories && !options.files && !options.multiple);
        Some(vec!["/local/tester/工作".into()])
    });
    cx.run_until_parked();

    assert_eq!(
        sftp_settings(cx, &fixture).local_directory.as_deref(),
        Some(Path::new("/local/tester/工作"))
    );
    assert_eq!(
        value(cx, &fixture, "sftp-local-directory").as_deref(),
        Some("/local/tester/工作")
    );

    close_settings(cx, &fixture);
    open_tab(cx, &fixture, 1, 1, "/local/tester/工作");
}

#[gpui_kit::test]
fn a_local_directory_that_is_gone_falls_back_to_downloads(cx: &mut TestAppContext) {
    let fixture = open(cx);
    fixture
        .local
        .unreadable
        .lock()
        .unwrap()
        .push("/别的电脑/工作".into());
    open_sftp_settings(cx, &fixture);
    in_frame(cx, fixture.handle, |window, cx| {
        window.click("sftp-local-directory", cx)
    });
    in_frame(cx, fixture.handle, |window, cx| {
        window.input("/别的电脑/工作", cx)
    });
    assert_eq!(
        sftp_settings(cx, &fixture).local_directory.as_deref(),
        Some(Path::new("/别的电脑/工作"))
    );
    close_settings(cx, &fixture);

    open_tab(cx, &fixture, 1, 1, LOCAL_DOWNLOADS);
    // 设置本身不动：目录也许只是暂时不在（比如移动硬盘）。
    assert_eq!(
        sftp_settings(cx, &fixture).local_directory.as_deref(),
        Some(Path::new("/别的电脑/工作"))
    );
}

#[gpui_kit::test]
fn clearing_the_local_directory_goes_back_to_downloads(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_sftp_settings(cx, &fixture);
    in_frame(cx, fixture.handle, |window, cx| {
        window.click("sftp-local-directory", cx)
    });
    in_frame(cx, fixture.handle, |window, cx| {
        window.input("/local/tester/工作", cx)
    });
    assert!(sftp_settings(cx, &fixture).local_directory.is_some());

    in_frame(cx, fixture.handle, |window, cx| {
        window.press("ctrl-a", cx);
        window.press("delete", cx);
    });
    assert_eq!(sftp_settings(cx, &fixture).local_directory, None);
    close_settings(cx, &fixture);

    open_tab(cx, &fixture, 1, 1, LOCAL_DOWNLOADS);
}

#[gpui_kit::test]
fn changing_the_local_directory_leaves_an_open_tab_where_it_is(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture, 1, 1, LOCAL_DOWNLOADS);
    open_sftp_settings(cx, &fixture);
    in_frame(cx, fixture.handle, |window, cx| {
        window.click("sftp-local-directory", cx)
    });
    in_frame(cx, fixture.handle, |window, cx| {
        window.input("/local/tester/工作", cx)
    });
    close_settings(cx, &fixture);
    cx.executor().advance_clock(Duration::from_millis(100));

    assert_eq!(
        value(cx, &fixture, "local-path").as_deref(),
        Some(LOCAL_DOWNLOADS)
    );
}

/// 文件列表里这一行的图标是不是彩色的。
fn row_is_colored(cx: &mut TestAppContext, fixture: &Fixture, row: &str) -> bool {
    in_frame(cx, fixture.handle, |window, _| {
        window
            .within("remote-card")
            .within(ElementId::Name(format!("name:{row}").into()))
            .try_find("color-icon")
            .is_some()
    })
}

#[gpui_kit::test]
fn the_icon_style_changes_the_preview_and_open_lists_at_once(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture, 1, 1, LOCAL_DOWNLOADS);
    assert!(row_is_colored(cx, &fixture, "文件 甲.txt"));
    open_sftp_settings(cx, &fixture);
    let preview_colored = |cx: &mut TestAppContext, fixture: &Fixture| {
        in_frame(cx, fixture.handle, |window, _| {
            window
                .within(("sftp-icon-preview", 1_usize))
                .try_find("color-icon")
                .is_some()
        })
    };
    assert!(preview_colored(cx, &fixture));

    in_frame(cx, fixture.handle, |window, cx| {
        window.click(("sftp-icon-style", 1_usize), cx)
    });

    assert_eq!(
        sftp_settings(cx, &fixture).file_icon_style,
        FileIconStyle::Monochrome
    );
    cx.update(|cx| assert_eq!(FileIconStyle::current(cx), FileIconStyle::Monochrome));
    assert!(!preview_colored(cx, &fixture));
    assert!(!row_is_colored(cx, &fixture, "文件 甲.txt"));
    in_frame(cx, fixture.handle, |window, cx| {
        assert_eq!(
            window.find(("sftp-icon-style", 1_usize)).selected(),
            Some(true)
        );
        window.click(("sftp-icon-style", 0_usize), cx);
    });
    assert!(row_is_colored(cx, &fixture, "文件 甲.txt"));
}

#[gpui_kit::test]
fn each_side_has_its_own_hidden_files_switch_and_open_lists_follow(cx: &mut TestAppContext) {
    let fixture = open(cx);
    open_tab(cx, &fixture, 1, 1, LOCAL_DOWNLOADS);
    let in_effect = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            (
                ShowHiddenFiles::get(CardSide::Local, cx),
                ShowHiddenFiles::get(CardSide::Remote, cx),
            )
        })
    };
    assert_eq!(in_effect(cx), (false, false));
    open_sftp_settings(cx, &fixture);

    in_frame(cx, fixture.handle, |window, cx| {
        window.click("sftp-show-hidden-remote", cx)
    });
    let settings = sftp_settings(cx, &fixture);
    assert!(settings.show_hidden.remote && !settings.show_hidden.local);
    assert_eq!(in_effect(cx), (false, true));

    in_frame(cx, fixture.handle, |window, cx| {
        assert_eq!(window.find("sftp-show-hidden-remote").checked(), Some(true));
        assert_eq!(window.find("sftp-show-hidden-local").checked(), Some(false));
        window.click("sftp-show-hidden-local", cx);
    });
    assert_eq!(in_effect(cx), (true, true));

    // 文件卡片的右键菜单切的是同一个设置：开关跟着变。
    dispatch(cx, fixture.handle, ToggleHiddenFiles(CardSide::Local));
    in_frame(cx, fixture.handle, |window, _| {
        assert_eq!(window.find("sftp-show-hidden-local").checked(), Some(false));
    });
    assert_eq!(in_effect(cx), (false, true));
}

#[gpui_kit::test]
fn the_hidden_files_switch_changes_the_file_count_of_an_open_list(cx: &mut TestAppContext) {
    let fixture = open(cx);
    // 假的本地目录 `/dotfiles` 里有两个点开头的文件。
    let store = cx.update(|cx| fixture.workspace.read(cx).settings().clone());
    cx.update(|cx| {
        store.update(cx, |store, cx| {
            store.update(
                |settings| settings.sftp.local_directory = Some("/dotfiles".into()),
                cx,
            )
        })
    });
    open_tab(cx, &fixture, 1, 1, "/dotfiles");
    let status = |cx: &mut TestAppContext, fixture: &Fixture| label(cx, fixture, "local-status");
    assert!(
        status(cx, &fixture).is_some_and(|text| text.ends_with("隐藏 2 项")),
        "{:?}",
        status(cx, &fixture)
    );

    open_sftp_settings(cx, &fixture);
    in_frame(cx, fixture.handle, |window, cx| {
        window.click("sftp-show-hidden-local", cx)
    });
    close_settings(cx, &fixture);

    assert!(
        status(cx, &fixture).is_some_and(|text| text.starts_with("3 项")),
        "{:?}",
        status(cx, &fixture)
    );
}
