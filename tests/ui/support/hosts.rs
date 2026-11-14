//! 主机栏、主机对话框（工单 11）的夹具：注入主机数据和内存版秘密存储，逐个找主机行。

use std::sync::Arc;

use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{ElementId, Entity, ScrollDelta, TestAppContext, WindowHandle, point, px};
use micaterm::workspace::{Workspace, WorkspaceServices};
pub use micaterm_hosts::secrets::InMemorySecretStore;
pub use micaterm_hosts::{
    AuthKind, Host, HostDatabase, HostDraft, HostId, HostOs, SecretStore as _,
};

use micaterm_hosts::HostData;

use super::{fake_services, in_frame, open_workspace_with, settings_store};

/// 用 `data` 和一个内存版秘密存储打开窗口壳；返回秘密存储，测试拿它看存了哪些密码。
pub fn open_with_hosts(
    cx: &mut TestAppContext,
    data: HostData,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<InMemorySecretStore>,
) {
    let secrets = Arc::new(InMemorySecretStore::default());
    let services = WorkspaceServices {
        hosts: data,
        secrets: secrets.clone(),
        ..fake_services()
    };
    let (handle, workspace) = open_workspace_with(cx, settings_store(), services);
    (handle, workspace, secrets)
}

/// 用 `HostData::seed()` 的六台主机打开：web-01、web-02、db-01、staging-api（2222）、
/// qa-runner、dev-box，id 依次是 1 到 6。
pub fn open_with_seed(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Root>,
    Entity<Workspace>,
    Arc<InMemorySecretStore>,
) {
    open_with_hosts(cx, HostData::seed())
}

/// 主机栏里那一行的元素 id。
pub fn host_row(id: u64) -> (&'static str, usize) {
    ("host-row", id as usize)
}

/// 保存的主机的名字，按列表顺序。
pub fn host_names(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Vec<String> {
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

/// 主机栏列出的主机（搜索时只有匹配的），从上到下。
pub fn listed(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> Vec<u64> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .listed_hosts(cx)
            .into_iter()
            .map(|id| id.0)
            .collect()
    })
}

/// 保存的那台主机，按 id。
pub fn saved_host(cx: &mut TestAppContext, workspace: &Entity<Workspace>, id: u64) -> Option<Host> {
    cx.update(|cx| {
        workspace
            .read(cx)
            .hosts()
            .read(cx)
            .host(HostId(id))
            .cloned()
    })
}

/// 改一下 store 里的主机数据（不经界面），比如记下探测到的系统。
pub fn edit_hosts(
    cx: &mut TestAppContext,
    workspace: &Entity<Workspace>,
    change: impl FnOnce(&mut micaterm::host::HostStore, &mut gpui_kit::Context<micaterm::host::HostStore>),
) {
    let store = cx.update(|cx| workspace.read(cx).hosts().clone());
    store.update(cx, change);
    cx.run_until_parked();
}

/// 主机表单页（标签里的，UI 重做工单 07）放不下全部字段时要滚动：把 `id` 滚进看得见的地方。
pub fn reveal(cx: &mut TestAppContext, handle: WindowHandle<Root>, id: &ElementId) {
    reveal_in(cx, handle, "host-dialog", id);
}

/// 把 `id` 在能滚动的 `scroller` 里滚进看得见的地方（主机表单页、凭据对话框的正文）。
pub fn reveal_in(
    cx: &mut TestAppContext,
    handle: WindowHandle<Root>,
    scroller: &'static str,
    id: &ElementId,
) {
    for _ in 0..40 {
        let place = in_frame(cx, handle, |window, _| {
            let target = window.find(id.clone());
            let Some(page) = window.try_find(scroller).map(|page| page.bounds()) else {
                return (true, false);
            };
            // 整个落在页面里才算看得见（只露一半时点中间会点到别处）。
            let bounds = target.bounds();
            let shown =
                target.visible() && bounds.top() >= page.top() && bounds.bottom() <= page.bottom();
            (shown, bounds.top() < page.center().y)
        });
        match place {
            (true, _) => return,
            // 在上面就往上滚，在下面就往下滚。
            (false, above) => in_frame(cx, handle, |window, cx| {
                let step = if above { 80. } else { -80. };
                window.scroll(scroller, ScrollDelta::Pixels(point(px(0.), px(step))), cx)
            }),
        }
    }
    panic!("{id:?} 滚不进看得见的地方");
}

/// 点主机表单的「保存」：按钮跟在表单后面，字段多（代理、跳板）时先滚过去。
pub fn click_host_save(cx: &mut TestAppContext, handle: WindowHandle<Root>) {
    reveal(cx, handle, &"host-dialog-save".into());
    in_frame(cx, handle, |window, cx| {
        window.click("host-dialog-save", cx)
    });
}
