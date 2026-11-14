//! SFTP 传输（工单 22）：确认框的四个入口、队列卡片和它的工具条、问题框（冲突、重试、续传）、断开、
//! 绿点和关闭确认。连接是假的（`FakeSftpProvider`），真协议的上传、下载、续传在 backend 的 `sftp_tests`。
// 参考 since2006/shell-rs @ b2a307a: tests/ui/transfer.rs

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{InputEvent as _, point};
use micaterm::app::{OpenHostSftp, SftpAction, SftpTabCommand};
use micaterm::backend::sftp::{
    ConflictFacts, FileFacts, SftpEvent, TransferChoice, TransferDetail, TransferPhase,
    TransferProgress, TransferQuestion, TransferQuestionKind,
};
use micaterm::sftp_tab::{CardSide, SftpTab};

use crate::support::*;

struct Fixture {
    handle: WindowHandle<Root>,
    workspace: Entity<Workspace>,
    sftp: Arc<FakeSftpProvider>,
    local: FakeLocalDirectory,
}

/// 开着主机 1 的 SFTP 标签，两侧都读完了。
fn open(cx: &mut TestAppContext) -> Fixture {
    let sftp = Arc::new(FakeSftpProvider::default());
    let local = FakeLocalDirectory::default();
    let services = WorkspaceServices {
        hosts: HostData::seed(),
        sftp: sftp.clone(),
        local_files: Arc::new(local.clone()),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    in_frame(cx, handle, |window, _| window.activate_window());
    let fixture = Fixture {
        handle,
        workspace,
        sftp,
        local,
    };
    dispatch(cx, handle, OpenHostSftp(HostId(1)));
    wait(cx, &fixture, "两侧读完目录", |cx, fixture| {
        value(cx, fixture, "remote-path").as_deref() == Some("/home/tester")
            && value(cx, fixture, "local-path").as_deref() == Some(LOCAL_DOWNLOADS)
    });
    fixture
}

/// 工作线程在真线程上：按真实时间等，同时拨测试时钟（标签每 16ms 收一次事件）。
fn wait(
    cx: &mut TestAppContext,
    fixture: &Fixture,
    what: &str,
    mut done: impl FnMut(&mut TestAppContext, &Fixture) -> bool,
) {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        cx.executor().advance_clock(Duration::from_millis(20));
        in_frame(cx, fixture.handle, |_, _| {});
        if done(cx, fixture) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("等不到：{what}");
}

fn value(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) -> Option<String> {
    in_frame(cx, fixture.handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.value().map(str::to_owned))
    })
}

fn label(
    cx: &mut TestAppContext,
    fixture: &Fixture,
    id: impl Into<ElementId> + 'static,
) -> Option<String> {
    in_frame(cx, fixture.handle, move |window, _| {
        window
            .try_find(id)
            .and_then(|element| element.label().map(str::to_owned))
    })
}

fn visible(cx: &mut TestAppContext, fixture: &Fixture, id: impl Into<ElementId> + 'static) -> bool {
    in_frame(cx, fixture.handle, move |window, _| {
        window.try_find(id).is_some()
    })
}

fn click(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) {
    in_frame(cx, fixture.handle, move |window, cx| window.click(id, cx));
}

fn press(cx: &mut TestAppContext, fixture: &Fixture, key: &'static str) {
    in_frame(cx, fixture.handle, move |window, cx| window.press(key, cx));
}

fn tab(cx: &mut TestAppContext, fixture: &Fixture) -> Entity<SftpTab> {
    cx.update(|cx| fixture.workspace.read(cx).active_sftp_tab())
        .expect("an SFTP tab in front")
}

fn command(cx: &mut TestAppContext, fixture: &Fixture, command: SftpTabCommand) {
    let tab = tab(cx, fixture);
    let id = cx.update(|cx| tab.read(cx).id());
    dispatch(cx, fixture.handle, SftpAction::new(id, command));
}

/// 这一侧选中一个文件，焦点在这一侧的列表。
fn select(cx: &mut TestAppContext, fixture: &Fixture, remote: bool, name: &'static str) {
    command(
        cx,
        fixture,
        SftpTabCommand::FocusCard {
            side: CardSide::from_remote(remote),
        },
    );
    let side = if remote { "remote-card" } else { "local-card" };
    in_frame(cx, fixture.handle, move |window, cx| {
        window
            .within(side)
            .click(ElementId::Name(format!("name:{name}").into()), cx)
    });
}

/// 选下载目录时选到的：引擎只收这台电脑上的完整路径，Windows 上 `/picked` 不算。
fn picked_folder() -> &'static str {
    if cfg!(windows) {
        "C:\\picked"
    } else {
        "/picked"
    }
}

/// 本地「下载」目录里的文件，引擎看到的路径。
fn local_file(name: &str) -> PathBuf {
    PathBuf::from(LOCAL_DOWNLOADS).join(name)
}

fn upload_paths(cx: &mut TestAppContext, fixture: &Fixture, names: &[&str]) {
    let paths = names.iter().map(|name| local_file(name)).collect();
    command(
        cx,
        fixture,
        SftpTabCommand::UploadPaths {
            paths,
            target: "/home/tester".into(),
        },
    );
}

/// 确认一批上传，等工作线程收到。
fn start_upload(cx: &mut TestAppContext, fixture: &Fixture, name: &'static str) {
    let before = fixture.sftp.requests.lock().unwrap().len();
    upload_paths(cx, fixture, &[name]);
    click(cx, fixture, "upload-confirm");
    wait(cx, fixture, "工作线程收到这一批", |_, fixture| {
        fixture.sftp.requests.lock().unwrap().len() > before
    });
}

/// 队头那批的状态。
fn head_status(cx: &mut TestAppContext, fixture: &Fixture) -> Option<String> {
    label(cx, fixture, "transfer-status")
}

fn wait_head(cx: &mut TestAppContext, fixture: &Fixture, status: &str) {
    let what = format!("队头变成「{status}」");
    wait(cx, fixture, &what, |cx, fixture| {
        head_status(cx, fixture).as_deref() == Some(status)
    });
}

fn wait_visible(cx: &mut TestAppContext, fixture: &Fixture, id: &'static str) {
    wait(cx, fixture, id, |cx, fixture| visible(cx, fixture, id));
}

fn conflict(id: u64) -> TransferQuestion {
    TransferQuestion::new(
        id,
        TransferQuestionKind::Conflict,
        "/home/tester/nginx.conf",
        anyhow::anyhow!("目标已存在"),
    )
    .with_conflict(Some(ConflictFacts::new(
        FileFacts::new(2458, None),
        Some(FileFacts::new(1536, None)),
        false,
    )))
}

// ---------- 入口 ----------

#[gpui_kit::test]
fn f5_in_the_local_list_confirms_and_uploads_into_the_remote_folder(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, false, "文件 甲.txt");
    press(cx, &fixture, "f5");

    assert!(visible(cx, &fixture, "upload-confirm"));
    assert_eq!(
        value(cx, &fixture, "upload-target").as_deref(),
        Some("/home/tester")
    );
    let summary = label(cx, &fixture, "transfer-summary").unwrap();
    assert!(summary.starts_with("上传 1 个文件到 "), "{summary}");
    // 确认之前什么都不传。
    assert!(fixture.sftp.requests.lock().unwrap().is_empty());
    assert!(!visible(cx, &fixture, "transfer-queue"));

    click(cx, &fixture, "upload-confirm");
    wait_head(cx, &fixture, "正在扫描");
    let requests = fixture.sftp.requests.lock().unwrap();
    assert_eq!(requests[0].sources(), [local_file("文件 甲.txt")]);
    assert_eq!(requests[0].destination().as_str(), "/home/tester");
}

/// 七列照 26 号：进度是最后一列，定宽 148，贴着行的右边，不出界。
#[gpui_kit::test]
fn the_queue_columns_fit_the_row(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    wait_head(cx, &fixture, "正在扫描");
    let (row, status) = in_frame(cx, fixture.handle, |window, _| {
        (
            window.find(("queue-entry", 1usize)).bounds(),
            window.find("transfer-status").bounds(),
        )
    });
    assert!(status.right() <= row.right() + px(1.), "{status:?} {row:?}");
    assert_eq!(status.size.width, px(148.));
}

#[gpui_kit::test]
fn f5_in_the_remote_list_confirms_and_downloads_into_a_chosen_folder(cx: &mut TestAppContext) {
    let fixture = open(cx);
    select(cx, &fixture, true, "文件 甲.txt");
    press(cx, &fixture, "f5");

    assert!(visible(cx, &fixture, "download-confirm"));
    assert_eq!(
        value(cx, &fixture, "download-target").as_deref(),
        Some(LOCAL_DOWNLOADS)
    );
    click(cx, &fixture, "download-browse");
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories && !options.files && !options.multiple);
        Some(vec![picked_folder().into()])
    });
    cx.run_until_parked();
    assert_eq!(
        value(cx, &fixture, "download-target").as_deref(),
        Some(picked_folder())
    );
    click(cx, &fixture, "download-confirm");
    wait(cx, &fixture, "工作线程收到下载", |_, fixture| {
        !fixture.sftp.downloads.lock().unwrap().is_empty()
    });
    let downloads = fixture.sftp.downloads.lock().unwrap();
    assert_eq!(
        downloads[0].sources()[0].as_str(),
        "/home/tester/文件 甲.txt"
    );
    assert_eq!(
        downloads[0].destination(),
        std::path::Path::new(picked_folder())
    );
}

#[gpui_kit::test]
fn the_confirmation_refuses_a_target_it_cannot_use_and_escape_sends_nothing(
    cx: &mut TestAppContext,
) {
    let fixture = open(cx);
    upload_paths(cx, &fixture, &["文件 甲.txt"]);
    // 焦点已经在目标输入框里，文字选中着。
    press(cx, &fixture, "backspace");
    click(cx, &fixture, "upload-confirm");
    assert!(visible(cx, &fixture, "upload-target-error"));
    assert!(visible(cx, &fixture, "upload-confirm"), "框还开着");

    press(cx, &fixture, "escape");
    assert!(!visible(cx, &fixture, "upload-confirm"));
    cx.update(|cx| {
        let tab = tab_of(&fixture, cx);
        assert_eq!(tab.read(cx).unfinished_transfers(), 0);
    });

    // 下载的目标是本机目录，要完整路径。
    command(
        cx,
        &fixture,
        SftpTabCommand::DownloadPaths {
            paths: vec!["/home/tester/文件 甲.txt".into()],
            target: "相对/路径".into(),
        },
    );
    click(cx, &fixture, "download-confirm");
    assert!(visible(cx, &fixture, "download-target-error"));
}

fn tab_of(fixture: &Fixture, cx: &App) -> Entity<SftpTab> {
    fixture.workspace.read(cx).active_sftp_tab().unwrap()
}

#[gpui_kit::test]
fn enter_in_the_target_field_confirms(cx: &mut TestAppContext) {
    let fixture = open(cx);
    upload_paths(cx, &fixture, &["文件 甲.txt"]);
    in_frame(cx, fixture.handle, |window, cx| {
        window.input("/固定目标", cx);
        window.press("enter", cx);
    });
    wait(cx, &fixture, "工作线程收到这一批", |_, fixture| {
        !fixture.sftp.requests.lock().unwrap().is_empty()
    });
    assert_eq!(
        fixture.sftp.requests.lock().unwrap()[0]
            .destination()
            .as_str(),
        "/固定目标"
    );
}

#[gpui_kit::test]
fn the_native_picker_leads_to_the_same_confirmation(cx: &mut TestAppContext) {
    let fixture = open(cx);
    command(cx, &fixture, SftpTabCommand::ChooseFiles);
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && options.multiple);
        Some(vec!["/tmp/甲".into(), "/tmp/乙".into()])
    });
    cx.run_until_parked();
    assert!(visible(cx, &fixture, "upload-confirm"));
    assert_eq!(
        value(cx, &fixture, "upload-target").as_deref(),
        Some("/home/tester")
    );
}

/// 「上传文件夹…」（工单 35）：系统的文件夹选择框只选文件夹，选中的整个走同一个确认框和传输队列。
#[gpui_kit::test]
fn the_folder_picker_uploads_the_chosen_folder_through_the_queue(cx: &mut TestAppContext) {
    let fixture = open(cx);
    let picked = tempfile::tempdir().unwrap();
    let folder = picked.path().join("工程");
    std::fs::create_dir_all(folder.join("子目录")).unwrap();
    command(cx, &fixture, SftpTabCommand::ChooseFolder);
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    let answer = folder.clone();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files, "{options:?}");
        assert_eq!(options.prompt.as_deref(), Some("选择要上传的文件夹"));
        Some(vec![answer])
    });
    cx.run_until_parked();
    assert_eq!(
        value(cx, &fixture, "upload-target").as_deref(),
        Some("/home/tester")
    );
    let summary = label(cx, &fixture, "transfer-summary").unwrap();
    assert!(summary.starts_with("上传 1 个文件夹到 "), "{summary}");

    click(cx, &fixture, "upload-confirm");
    wait(cx, &fixture, "工作线程收到这一批", |_, fixture| {
        !fixture.sftp.requests.lock().unwrap().is_empty()
    });
    let requests = fixture.sftp.requests.lock().unwrap();
    assert_eq!(requests[0].sources(), [folder]);
    assert_eq!(requests[0].destination().as_str(), "/home/tester");
}

#[gpui_kit::test]
fn dropping_files_from_outside_on_a_remote_folder_asks_to_upload_there(cx: &mut TestAppContext) {
    let fixture = open(cx);
    in_frame(cx, fixture.handle, |window, cx| {
        let position = window
            .within("remote-card")
            .find("file:目录")
            .bounds()
            .center();
        let paths = gpui_kit::ExternalPaths(vec![PathBuf::from("/tmp/资源管理器的文件")].into());
        window.dispatch_event(
            gpui_kit::FileDropEvent::Entered { position, paths }.to_platform_input(),
            cx,
        );
        window.dispatch_event(
            gpui_kit::FileDropEvent::Submit { position }.to_platform_input(),
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(
        value(cx, &fixture, "upload-target").as_deref(),
        Some("/home/tester/目录")
    );
}

#[gpui_kit::test]
fn dragging_between_the_sides_asks_first(cx: &mut TestAppContext) {
    let fixture = open(cx);
    in_frame(cx, fixture.handle, |window, cx| {
        let from = window
            .within("local-card")
            .find("name:文件 甲.txt")
            .bounds()
            .center();
        let to = window
            .within("remote-card")
            .find("file:目录")
            .bounds()
            .center();
        window.drag(from, to, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        value(cx, &fixture, "upload-target").as_deref(),
        Some("/home/tester/目录")
    );
    press(cx, &fixture, "escape");

    in_frame(cx, fixture.handle, |window, cx| {
        let from = window
            .within("remote-card")
            .find("name:文件 乙.txt")
            .bounds()
            .center();
        let to = window
            .within("local-card")
            .find("file:目录")
            .bounds()
            .center();
        window.drag(from, to, cx);
    });
    cx.run_until_parked();
    let target = value(cx, &fixture, "download-target").unwrap();
    assert!(target.ends_with("目录"), "{target}");
}

// ---------- 队列 ----------

#[gpui_kit::test]
fn batches_wait_their_turn_and_the_queue_goes_away_when_cleared(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    wait_head(cx, &fixture, "正在扫描");
    assert_eq!(
        label(cx, &fixture, "transfer-queue-summary").as_deref(),
        Some("1 批进行中")
    );

    // 第二批：框里说要排队，确认以后等着，工作线程还没收到。
    upload_paths(cx, &fixture, &["文件 乙.txt"]);
    assert!(visible(cx, &fixture, "transfer-queued-note"));
    click(cx, &fixture, "upload-confirm");
    cx.run_until_parked();
    assert_eq!(
        label(cx, &fixture, ("queue-entry", 2usize)).as_deref(),
        Some("等待中")
    );
    assert_eq!(
        label(cx, &fixture, "transfer-queue-summary").as_deref(),
        Some("1 批进行中 · 1 批等待")
    );
    assert_eq!(fixture.sftp.requests.lock().unwrap().len(), 1);

    // 第一批结束，引擎闲下来，轮到第二批。
    fixture.sftp.emit(SftpEvent::Progress(TransferProgress::new(
        TransferPhase::Completed,
    )));
    fixture.sftp.emit(SftpEvent::Idle);
    wait(cx, &fixture, "第二批开始", |_, fixture| {
        fixture.sftp.requests.lock().unwrap().len() == 2
    });
    assert_eq!(
        fixture.sftp.requests.lock().unwrap()[1].sources(),
        [local_file("文件 乙.txt")]
    );
    assert_eq!(
        label(cx, &fixture, ("queue-entry", 1usize)).as_deref(),
        Some("已完成")
    );

    // 进度、在传的文件自己一行。
    fixture.sftp.emit(SftpEvent::Progress(
        TransferProgress::new(TransferPhase::Transferring)
            .with_bytes(512, 1024)
            .with_current("/local/文件 乙.txt", "/home/tester/文件 乙.txt", 256, 1024),
    ));
    wait_head(cx, &fixture, "50%");
    assert_eq!(
        label(cx, &fixture, ("queue-file", 2usize)).as_deref(),
        Some("/local/文件 乙.txt")
    );
    assert_eq!(
        label(cx, &fixture, ("queue-file-status", 2usize)).as_deref(),
        Some("25%")
    );
    // 「文件大小」一列（原来的「已传输」）写这批一共多大。
    assert_eq!(
        label(cx, &fixture, ("queue-size", 2usize)).as_deref(),
        Some("1.0 KB")
    );

    // 清除已完成只清结束了的，全清完才收起。
    click(cx, &fixture, "clear-finished-transfers");
    assert!(!visible(cx, &fixture, ("queue-entry", 1usize)));
    assert!(visible(cx, &fixture, "transfer-queue"));
    fixture.sftp.emit(SftpEvent::Progress(TransferProgress::new(
        TransferPhase::Completed,
    )));
    fixture.sftp.emit(SftpEvent::Idle);
    wait(cx, &fixture, "第二批结束", |cx, fixture| {
        label(cx, fixture, ("queue-entry", 2usize)).as_deref() == Some("已完成")
    });
    click(cx, &fixture, "clear-finished-transfers");
    assert!(!visible(cx, &fixture, "transfer-queue"));
}

#[gpui_kit::test]
fn the_queue_stops_resumes_and_takes_a_batch_off(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    wait_head(cx, &fixture, "正在扫描");

    click(cx, &fixture, "cancel-transfer");
    wait_head(cx, &fixture, "已停止");
    wait(cx, &fixture, "能继续", |cx, fixture| {
        cx.update(|cx| tab_of(fixture, cx).read(cx).can_resume_transfer())
    });
    // 绿点是「有传输在进行」，停着的不算；关闭确认还是要问。
    let id = cx.update(|cx| fixture.workspace.read(cx).tab_ids()[0]);
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).tab_dot(id, cx)),
        Some(StatusDot::None)
    );

    click(cx, &fixture, "resume-transfer");
    wait_head(cx, &fixture, "0%");
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).tab_dot(id, cx)),
        Some(StatusDot::Busy)
    );
    // 还是原来那批，没有重新发。
    assert_eq!(fixture.sftp.requests.lock().unwrap().len(), 1);

    click(cx, &fixture, "cancel-transfer");
    wait_head(cx, &fixture, "已停止");
    wait(cx, &fixture, "能移出", |cx, fixture| {
        cx.update(|cx| tab_of(fixture, cx).read(cx).can_remove_transfer())
    });
    click(cx, &fixture, "remove-transfer");
    wait(cx, &fixture, "移出", |cx, fixture| {
        label(cx, fixture, ("queue-entry", 1usize)).as_deref() == Some("已移出")
    });
    assert_eq!(
        cx.update(|cx| tab_of(&fixture, cx).read(cx).unfinished_transfers()),
        0
    );
}

/// 移出的一批不能被之后的「重新连接」（引擎的 `Resume`）接着传：引擎要忘掉它。
#[gpui_kit::test]
fn a_batch_taken_off_is_not_picked_up_by_a_reconnect(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    wait_head(cx, &fixture, "正在扫描");
    click(cx, &fixture, "cancel-transfer");
    wait_head(cx, &fixture, "已停止");
    wait(cx, &fixture, "能移出", |cx, fixture| {
        cx.update(|cx| tab_of(fixture, cx).read(cx).can_remove_transfer())
    });
    click(cx, &fixture, "remove-transfer");
    wait(cx, &fixture, "引擎闲下来", |cx, fixture| {
        cx.update(|cx| tab_of(fixture, cx).read(cx).can_reconnect())
    });

    command(cx, &fixture, SftpTabCommand::Reconnect);
    wait(cx, &fixture, "只重连", |_, fixture| {
        *fixture.sftp.reconnects.lock().unwrap() == 1
    });
}

#[gpui_kit::test]
fn discarding_drops_the_stopped_batch_and_the_next_one_goes(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    wait_head(cx, &fixture, "正在扫描");
    upload_paths(cx, &fixture, &["文件 乙.txt"]);
    click(cx, &fixture, "upload-confirm");
    click(cx, &fixture, "cancel-transfer");
    wait_head(cx, &fixture, "已停止");
    // 停着的一批占着队头：第二批不开始。
    assert_eq!(fixture.sftp.requests.lock().unwrap().len(), 1);
    wait(cx, &fixture, "能丢弃", |cx, fixture| {
        cx.update(|cx| tab_of(fixture, cx).read(cx).can_remove_transfer())
    });

    click(cx, &fixture, "discard-transfer");
    wait(cx, &fixture, "第二批开始", |_, fixture| {
        fixture.sftp.requests.lock().unwrap().len() == 2
    });
    assert_eq!(
        label(cx, &fixture, ("queue-entry", 1usize)).as_deref(),
        Some("已丢弃续传进度")
    );
}

#[gpui_kit::test]
fn a_batch_unfolds_to_how_its_items_ended_and_a_rough_end_is_summed_up(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    wait_head(cx, &fixture, "正在扫描");

    in_frame(cx, fixture.handle, |window, cx| {
        window.click(("queue-expand", 1usize), cx)
    });
    assert!(visible(cx, &fixture, "transfer-detail-empty"));

    fixture.sftp.emit(SftpEvent::Progress(
        TransferProgress::new(TransferPhase::Completed).with_details(vec![
            TransferDetail::done("/home/tester/甲"),
            TransferDetail::skipped("/home/tester/乙"),
            TransferDetail::failed("/home/tester/丙", anyhow::anyhow!("权限不足")),
        ]),
    ));
    wait(cx, &fixture, "这批结束", |cx, fixture| {
        label(cx, fixture, ("queue-entry", 1usize)).as_deref() == Some("部分失败（1 项）")
    });
    assert_eq!(
        label(
            cx,
            &fixture,
            ElementId::Name("transfer-detail:/home/tester/丙".into())
        )
        .as_deref(),
        Some("失败 /home/tester/丙")
    );
    // 结束后汇总：有跳过或失败时弹通知，全部完成不打扰。
    let notes = in_frame(cx, fixture.handle, |window, cx| {
        window.notifications(cx).len()
    });
    assert_eq!(notes, 1);
}

#[gpui_kit::test]
fn a_finished_batch_refreshes_the_side_it_went_to(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    wait_head(cx, &fixture, "正在扫描");
    let listed = fixture.sftp.listed.lock().unwrap().len();
    fixture.sftp.emit(SftpEvent::Progress(TransferProgress::new(
        TransferPhase::Completed,
    )));
    fixture.sftp.emit(SftpEvent::Idle);
    wait(cx, &fixture, "远程重读", |_, fixture| {
        fixture.sftp.listed.lock().unwrap().len() > listed
    });

    // 下载完重读本地。
    let local = fixture.local.listed.lock().unwrap().len();
    command(
        cx,
        &fixture,
        SftpTabCommand::DownloadPaths {
            paths: vec!["/home/tester/文件 甲.txt".into()],
            target: picked_folder().into(),
        },
    );
    click(cx, &fixture, "download-confirm");
    wait(cx, &fixture, "工作线程收到下载", |_, fixture| {
        !fixture.sftp.downloads.lock().unwrap().is_empty()
    });
    wait_head(cx, &fixture, "0%");
    fixture.sftp.emit(SftpEvent::Progress(TransferProgress::new(
        TransferPhase::Completed,
    )));
    wait(cx, &fixture, "本地重读", |_, fixture| {
        fixture.local.listed.lock().unwrap().len() > local
    });
}

#[gpui_kit::test]
fn the_height_of_the_queue_can_be_dragged(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    wait_head(cx, &fixture, "正在扫描");
    let height = |cx: &mut TestAppContext| {
        in_frame(cx, fixture.handle, |window, _| {
            window.find("transfer-queue").bounds().size.height
        })
    };
    let before = height(cx);
    in_frame(cx, fixture.handle, |window, cx| {
        let from = window.find("sftp-queue-gap").bounds().center();
        window.drag(from, point(from.x, from.y - px(60.)), cx);
    });
    cx.run_until_parked();
    assert!(height(cx) > before, "{:?} -> {:?}", before, height(cx));
}

// ---------- 问题 ----------

#[gpui_kit::test]
fn a_conflict_shows_both_sides_and_the_answer_goes_for_the_rest_when_ticked(
    cx: &mut TestAppContext,
) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    fixture.sftp.emit(SftpEvent::Question(conflict(900)));
    wait_visible(cx, &fixture, "transfer-question");

    let text = label(cx, &fixture, "transfer-question-text").unwrap();
    assert!(text.contains("nginx.conf"), "{text}");
    assert!(text.contains("要传的 2.4 KB（本机）"), "{text}");
    assert!(text.contains("已有的 1.5 KB（web-01）"), "{text}");
    click(cx, &fixture, "transfer-question-check");
    click(cx, &fixture, "transfer-question-overwrite");
    wait(cx, &fixture, "回答到工作线程", |_, fixture| {
        !fixture.sftp.answers.lock().unwrap().is_empty()
    });
    let (request, answer) = fixture.sftp.answers.lock().unwrap()[0];
    assert_eq!(request, 900);
    assert_eq!(answer.choice(), TransferChoice::Overwrite);
    assert!(answer.apply_to_all());
    assert!(!visible(cx, &fixture, "transfer-question"));
}

#[gpui_kit::test]
fn closing_a_question_with_escape_cancels_the_batch(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    fixture.sftp.emit(SftpEvent::Question(conflict(901)));
    wait_visible(cx, &fixture, "transfer-question");
    press(cx, &fixture, "escape");
    wait_head(cx, &fixture, "已停止");
    let (_, answer) = fixture.sftp.answers.lock().unwrap()[0];
    assert_eq!(answer.choice(), TransferChoice::Cancel);
}

#[gpui_kit::test]
fn an_error_can_be_retried_or_skipped(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    fixture.sftp.emit(SftpEvent::Question(TransferQuestion::new(
        902,
        TransferQuestionKind::Error,
        "/home/tester/文件 甲.txt",
        anyhow::anyhow!("权限不足"),
    )));
    wait_visible(cx, &fixture, "transfer-question");
    assert!(visible(cx, &fixture, "transfer-question-skip"));
    assert!(!visible(cx, &fixture, "transfer-question-check"));
    click(cx, &fixture, "transfer-question-retry");
    wait(cx, &fixture, "回答到工作线程", |_, fixture| {
        !fixture.sftp.answers.lock().unwrap().is_empty()
    });
    assert_eq!(
        fixture.sftp.answers.lock().unwrap()[0].1.choice(),
        TransferChoice::Retry
    );
}

#[gpui_kit::test]
fn an_unfinished_file_asks_to_resume_or_start_over(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    fixture.sftp.emit(SftpEvent::Question(TransferQuestion::new(
        903,
        TransferQuestionKind::Resume,
        "/home/tester/文件 甲.txt",
        anyhow::anyhow!("发现未完成的文件"),
    )));
    wait_visible(cx, &fixture, "transfer-question");
    assert!(visible(cx, &fixture, "transfer-question-resume"));
    click(cx, &fixture, "transfer-question-restart");
    wait(cx, &fixture, "回答到工作线程", |_, fixture| {
        !fixture.sftp.answers.lock().unwrap().is_empty()
    });
    assert_eq!(
        fixture.sftp.answers.lock().unwrap()[0].1.choice(),
        TransferChoice::Restart
    );
}

/// 批次自己停了（或者重连去了）：框上的问题已经没人等，收起来，下一轮再问。
#[gpui_kit::test]
fn a_question_goes_away_when_the_batch_stops_or_reconnects(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    fixture.sftp.emit(SftpEvent::Question(conflict(904)));
    wait_visible(cx, &fixture, "transfer-question");
    fixture.sftp.emit(SftpEvent::Progress(TransferProgress::new(
        TransferPhase::Reconnecting,
    )));
    wait(cx, &fixture, "问题收起", |cx, fixture| {
        !visible(cx, fixture, "transfer-question")
    });
    assert!(fixture.sftp.answers.lock().unwrap().is_empty());

    fixture.sftp.emit(SftpEvent::Question(conflict(905)));
    wait_visible(cx, &fixture, "transfer-question");
    fixture.sftp.emit(SftpEvent::Progress(TransferProgress::new(
        TransferPhase::Stopped,
    )));
    wait(cx, &fixture, "问题收起", |cx, fixture| {
        !visible(cx, fixture, "transfer-question")
    });
    assert!(fixture.sftp.answers.lock().unwrap().is_empty());
}

/// 问题叠在别的框上面，答完回到下面的框。
#[gpui_kit::test]
fn a_question_stacks_on_a_confirmation_that_is_open(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    upload_paths(cx, &fixture, &["文件 乙.txt"]);
    assert!(visible(cx, &fixture, "upload-confirm"));
    fixture.sftp.emit(SftpEvent::Question(conflict(906)));
    wait_visible(cx, &fixture, "transfer-question");
    click(cx, &fixture, "transfer-question-skip");
    assert!(!visible(cx, &fixture, "transfer-question"));
    assert!(visible(cx, &fixture, "upload-confirm"));
}

// ---------- 断开 ----------

#[gpui_kit::test]
fn a_transfer_asked_for_while_disconnected_offers_to_reconnect(cx: &mut TestAppContext) {
    let fixture = open(cx);
    fixture
        .sftp
        .emit(SftpEvent::Disconnected("连接断开".into()));
    wait(cx, &fixture, "断开", |cx, fixture| {
        cx.update(|cx| tab_of(fixture, cx).read(cx).can_reconnect())
            && cx.update(|cx| {
                tab_of(fixture, cx).read(cx).connection_state()
                    == micaterm::sftp_tab::ConnectionState::Disconnected
            })
    });
    select(cx, &fixture, false, "文件 甲.txt");
    press(cx, &fixture, "f5");
    assert!(visible(cx, &fixture, "sftp-reconnect"));
    assert!(!visible(cx, &fixture, "upload-confirm"));
}

/// 断线后重连都失败了：批次停着等「继续」，这时「继续」就是重连。
#[gpui_kit::test]
fn a_batch_that_gave_up_reconnecting_waits_for_resume(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    fixture
        .sftp
        .emit(SftpEvent::Disconnected("连接断开".into()));
    fixture.sftp.emit(SftpEvent::Progress(TransferProgress::new(
        TransferPhase::Reconnecting,
    )));
    wait_head(cx, &fixture, "正在重连");
    fixture.sftp.emit(SftpEvent::Progress(TransferProgress::new(
        TransferPhase::Stopped,
    )));
    fixture.sftp.emit(SftpEvent::Idle);
    wait_head(cx, &fixture, "已停止");
    wait(cx, &fixture, "能继续", |cx, fixture| {
        cx.update(|cx| tab_of(fixture, cx).read(cx).can_resume_transfer())
    });
    click(cx, &fixture, "resume-transfer");
    wait(cx, &fixture, "连上去", |_, fixture| {
        *fixture.sftp.reconnects.lock().unwrap() == 1
    });
}

// ---------- 绿点和关闭 ----------

#[gpui_kit::test]
fn closing_with_a_transfer_running_asks_and_closes_its_dialogs(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    wait_head(cx, &fixture, "正在扫描");
    let id = cx.update(|cx| fixture.workspace.read(cx).tab_ids()[0]);
    assert_eq!(
        cx.update(|cx| fixture.workspace.read(cx).tab_dot(id, cx)),
        Some(StatusDot::Busy)
    );

    dispatch(cx, fixture.handle, CloseFocusedCard);
    assert!(visible(cx, &fixture, "close-tabs"));
    let text = label(cx, &fixture, "close-tabs-text").unwrap();
    assert!(text.contains("还有 1 批传输没完成"), "{text}");
    assert!(text.contains("续传记录保留"), "{text}");
    click(cx, &fixture, "close-tabs-cancel");

    // 原型：框里说正在传哪个文件、这一批传到多少。
    fixture.sftp.emit(SftpEvent::Progress(
        TransferProgress::new(TransferPhase::Transferring)
            .with_bytes(37, 100)
            .with_current(
                "/local/backup.tar.gz",
                "/home/tester/backup.tar.gz",
                37,
                100,
            ),
    ));
    wait_head(cx, &fixture, "37%");
    dispatch(cx, fixture.handle, CloseFocusedCard);
    let text = label(cx, &fixture, "close-tabs-text").unwrap();
    assert!(
        text.contains("还有 1 批传输没完成（backup.tar.gz，37%）"),
        "{text}"
    );
    click(cx, &fixture, "close-tabs-cancel");
    assert!(cx.update(|cx| fixture.workspace.read(cx).active_sftp_tab().is_some()));

    // 问题框开着时确认关闭：框跟着标签一起走，不留下没人回答的框。
    fixture.sftp.emit(SftpEvent::Question(conflict(907)));
    wait_visible(cx, &fixture, "transfer-question");
    dispatch(cx, fixture.handle, CloseFocusedCard);
    click(cx, &fixture, "close-tabs-confirm");
    assert!(cx.update(|cx| fixture.workspace.read(cx).active_sftp_tab().is_none()));
    assert!(!visible(cx, &fixture, "transfer-question"));
}

/// 退出时停着的一批也算没完成（工单 23 的框读的是同一个数）。
#[gpui_kit::test]
fn quitting_with_a_stopped_batch_asks(cx: &mut TestAppContext) {
    let fixture = open(cx);
    start_upload(cx, &fixture, "文件 甲.txt");
    wait_head(cx, &fixture, "正在扫描");
    click(cx, &fixture, "cancel-transfer");
    wait_head(cx, &fixture, "已停止");

    assert!(in_frame(cx, fixture.handle, micaterm::app::quit_held_back));
    let text = label(cx, &fixture, "quit-confirm-text").unwrap();
    assert!(text.contains("还有 1 批传输没完成"), "{text}");
}

/// 续传记录没有地方放（取不到数据目录）：窗口出来就告诉用户。
#[gpui_kit::test]
fn a_missing_resume_folder_is_reported_once_the_window_is_up(cx: &mut TestAppContext) {
    let services = WorkspaceServices {
        sftp_problem: Some("找不到数据目录，这次的传输不能续传：无法读取".into()),
        ..fake_services()
    };
    let (handle, _) = open_workspace_with(cx, settings_store(), services);
    let shown = in_frame(cx, handle, |window, cx| window.notifications(cx).len());
    assert_eq!(shown, 1);
}

/// 编辑器打不开的文件，框里的「下载…」走同一个确认框，目标是本地一侧的当前目录。
#[gpui_kit::test]
fn a_file_the_editor_refuses_can_be_downloaded(cx: &mut TestAppContext) {
    let fixture = open(cx);
    fixture.sftp.files.lock().unwrap().insert(
        "/home/tester/blob.bin".into(),
        (vec![0xff, 0xfe, 0x00], 100),
    );
    command(
        cx,
        &fixture,
        SftpTabCommand::Edit {
            side: CardSide::Remote,
            path: Some("/home/tester/blob.bin".into()),
        },
    );
    wait_visible(cx, &fixture, "editor-refused");
    click(cx, &fixture, "editor-refused-download");
    wait_visible(cx, &fixture, "download-confirm");
    assert_eq!(
        value(cx, &fixture, "download-target").as_deref(),
        Some(LOCAL_DOWNLOADS)
    );
    let summary = label(cx, &fixture, "transfer-summary").unwrap();
    assert!(summary.ends_with("下载 1 个文件"), "{summary}");
}
