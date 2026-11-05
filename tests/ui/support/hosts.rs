//! 主机栏、主机对话框（工单 11）的夹具：注入主机数据和内存版秘密存储，逐个找主机行。

use std::sync::Arc;

use gpui_kit::component::Root;
use gpui_kit::{Entity, TestAppContext, WindowHandle};
use qishell::workspace::{Workspace, WorkspaceServices};
pub use qishell_hosts::secrets::InMemorySecretStore;
pub use qishell_hosts::{
    AuthKind, Host, HostDatabase, HostDraft, HostId, HostOs, SecretStore as _,
};

use qishell_hosts::HostData;

use super::{fake_services, open_workspace_with, settings_store};

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
    change: impl FnOnce(&mut qishell::host::HostStore, &mut gpui_kit::Context<qishell::host::HostStore>),
) {
    let store = cx.update(|cx| workspace.read(cx).hosts().clone());
    store.update(cx, change);
    cx.run_until_parked();
}
