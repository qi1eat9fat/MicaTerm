// 参考 since2006/shell-rs @ b2a307a: src/host/host_dialog.rs 的 `confirm_delete_host`、
// `describe_host_delete` 重写：样式按全部界面原型的 `.dlg.confirm`，去掉端口转发。

//! 删除主机的确认（原型「删除被当作跳板的主机」）：写明有几台主机把它当作跳板并列出名字；
//! 删除后已打开的卡片不受影响。确认框本身是共用的 `ui::confirm`。

use gpui_kit::*;

use micaterm_hosts::HostId;

use crate::host::HostStore;
use crate::i18n::t;
use crate::ui::DialogLayer;
use crate::ui::confirm::{Confirm, ConfirmLine, open_confirm_dialog};

/// 问一下再删除 `host`。主机已经不在了就什么都不做。
pub fn open_delete_host_dialog(
    store: Entity<HostStore>,
    host: HostId,
    window: &mut Window,
    cx: &mut App,
) {
    let (name, jump_users) = {
        let data = store.read(cx);
        let Some(name) = data.host(host).map(|host| host.name.clone()) else {
            return;
        };
        let users: Vec<SharedString> = data
            .jumping_through(&[host])
            .into_iter()
            .filter_map(|id| data.host(id).map(|host| host.name.clone()))
            .collect();
        (name, users)
    };
    let mut lines = vec![ConfirmLine::Text(t!("host.delete.question", name = name))];
    if !jump_users.is_empty() {
        lines.push(ConfirmLine::Text(t!(
            "host.delete.jump_users",
            count = jump_users.len()
        )));
        lines.extend(jump_users.into_iter().map(ConfirmLine::Item));
        lines.push(ConfirmLine::Text(t!("host.delete.jump_users_after")));
    }
    lines.push(ConfirmLine::Text(t!("host.delete.cards_stay")));
    open_confirm_dialog(
        Confirm {
            id: "host-delete",
            title: t!("host.delete.title"),
            lines,
            cancel: t!("host.delete.cancel"),
            confirm: t!("host.delete.confirm"),
        },
        DialogLayer::new(),
        Box::new(move |_, _, cx| {
            store.update(cx, |store, cx| store.remove(host, cx));
            None
        }),
        Box::new(|_, _| {}),
        window,
        cx,
    );
}
