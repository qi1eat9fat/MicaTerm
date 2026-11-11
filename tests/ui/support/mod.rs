//! 各测试模块共用的东西：导入、打开窗口壳、逐帧驱动、派发 action。
//!
//! 每个测试模块开头写 `use crate::support::*;`，导入、夹具和假实现一起带进来。
//! 注意：不要写 `use gpui_kit::*`，它会把 `#[test]` 遮蔽掉，要用的类型在这里显式导入。
//!
//! 用法：
//! ```ignore
//! #[gpui_kit::test]
//! fn some_behavior(cx: &mut TestAppContext) {
//!     let (handle, workspace) = open_workspace(cx);
//!     dispatch(cx, handle, ToggleHostSidebar);          // 派发 action（同快捷键、按钮）
//!     in_frame(cx, handle, |window, cx| {               // 画一帧再查元素、点按钮、按键
//!         assert!(window.find("start-card").visible());
//!         window.click("expand-host-sidebar", cx);
//!         window.press("ctrl-shift-b", cx);
//!     });
//!     cx.update(|cx| assert!(workspace.read(cx).is_host_sidebar_open()));
//! }
//! ```
//!
//! 元素要能被查到，需要 `.id(..)` 加 `.test_support()`；`ElementSnapshot` 里没有文字和样式，
//! 要断言的文字放进 `aria_label`，样式类状态给实体加只读查询方法。

mod credentials;
mod hosts;
mod remote;
mod sftp;
mod terminal;
mod vault;

pub use std::sync::Arc;

pub use credentials::*;
pub use gpui_kit::component::ActiveTheme as _;
pub use gpui_kit::component::{Root, WindowExt as _};
pub use gpui_kit::test::TestWindowExt;
pub use gpui_kit::{
    Action, App, AppContext as _, ElementId, Entity, TestAppContext, WindowHandle, px, size,
};
pub use hosts::*;
pub use qishell::app::{CloseCard, NewLocalTerminalTab, ToggleHostSidebar};
pub use qishell::card::CardId;
pub use qishell::settings::AppSettings;
pub use qishell::settings::{Appearance, SettingsStore};
pub use qishell::tabs::TabId;
pub use qishell::theme::Tokens;
pub use qishell::workspace::{Workspace, WorkspaceServices};
pub use qishell_terminal::{FixedLocalTerminalTransportProvider, LocalShell, LocalShellInfo};
pub use qishell_terminal::{RightClick, TerminalColors, TerminalFont, TerminalInteraction};
pub use remote::*;
pub use sftp::*;
pub use terminal::*;
pub use vault::*;
// 卡片分屏（工单 05）。
pub use gpui_kit::VisualTestContext;
pub use qishell::app::{CloseFocusedCard, SplitDown, SplitRight, ToggleZoomCard};
pub use qishell::split::NodeId;
// 程序图块与状态点（工单 06）。
pub use qishell::card::StatusDot;
// 主机数据（工单 09 起）：内存里的，不碰数据目录。
pub use qishell::host::HostData;

/// 每个夹具都先做的事：QiShell 的全局量（文案、字体、快捷键）。
pub fn init_app(cx: &mut TestAppContext) {
    cx.update(qishell::init);
}

/// 只放在内存里的设置：测试不碰真实的 `settings.json`。
pub fn settings_store() -> SettingsStore {
    SettingsStore::in_memory()
}

/// 注入窗口壳的服务，全部是假实现：测试不碰真实的终端、钥匙串和网络。
/// 后续工单每给 `WorkspaceServices` 加一项服务，就在这里填上它的假实现。
///
/// 各工单加字段时只改这一处；别的夹具一律写 `WorkspaceServices { 字段, ..fake_services() }`。
pub fn fake_services() -> WorkspaceServices {
    WorkspaceServices {
        local_terminals: fake_local_terminals(),
        hosts: HostData::empty(),
        hosts_problem: None,
        // 秘密只在内存里（工单 11）。
        secrets: Arc::new(qishell_hosts::secrets::InMemorySecretStore::default()),
        // 远程终端不连网络，测试连接总是成功（工单 12）。
        remote_terminals: Arc::new(FakeRemoteTerminals::default()),
        connection_tester: Arc::new(FakeConnectionTester::succeeding()),
        // 没有保险库：秘密只在上面的内存存储里，不弹保险库的框（工单 13 的夹具见 `vault.rs`）。
        vault: None,
        vault_problem: None,
        // SFTP 不连网络，本地目录不碰真实磁盘（工单 20）。
        sftp: Arc::new(FakeSftpProvider::default()),
        local_files: Arc::new(FakeLocalDirectory::default()),
        sftp_problem: None,
        // 没有数据目录：不记窗口的大小和位置（工单 26 的测试给临时目录）。
        data_dir: None,
    }
}

/// 假服务，本地终端用给定的假 provider：测试拿着它看启动了什么、关了什么。
pub fn services_with_terminals(
    local_terminals: Arc<FixedLocalTerminalTransportProvider>,
) -> WorkspaceServices {
    WorkspaceServices {
        local_terminals,
        ..fake_services()
    }
}

/// 用内存设置和假服务打开窗口壳。
pub fn open_workspace(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<Workspace>) {
    open_workspace_with(cx, settings_store(), fake_services())
}

/// 用给定的设置和服务打开窗口壳：走生产代码同一个注入式构造 `Workspace::new_with_services`。
///
/// 动画关掉：否则对话框会按真实时间滑入，负载高时元素在查到和点下之间会挪位置。
pub fn open_workspace_with(
    cx: &mut TestAppContext,
    settings: SettingsStore,
    services: WorkspaceServices,
) -> (WindowHandle<Root>, Entity<Workspace>) {
    init_app(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let mut workspace = None;
    let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
        let settings = cx.new(|_| settings);
        let view = cx.new(|cx| Workspace::new_with_services(settings, services, window, cx));
        workspace = Some(view.clone());
        Root::new(view, window, cx)
    });
    cx.run_until_parked();
    (handle, workspace.expect("workspace created"))
}

/// 画一帧，对这一帧执行 `body`，再让它引发的事情（action、通知）都跑完。
pub fn in_frame<R>(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    body: impl FnOnce(&mut gpui_kit::Window, &mut App) -> R,
) -> R {
    let result = cx
        .update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            body(window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    result
}

/// 像按钮和快捷键那样派发一个 action：从当前焦点往上送，由窗口壳处理。
/// action 是延迟执行的，这里顺带跑完。
pub fn dispatch(cx: &mut TestAppContext, handle: WindowHandle<Root>, action: impl Action) {
    in_frame(cx, handle, |_, _| {});
    cx.dispatch_action(handle.into(), action);
    cx.run_until_parked();
}
