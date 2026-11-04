// 参考 since2006/shell-rs @ b2a307a: src/host/host_dialog.rs 的 `confirm_delete_host`、
// `describe_host_delete` 重写：样式按全部界面原型的 `.dlg.confirm`，去掉端口转发。

//! 删除主机的确认（原型「删除被当作跳板的主机」）：写明有几台主机把它当作跳板并列出名字；
//! 删除后已打开的卡片不受影响。

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use qishell_hosts::HostId;

use crate::host::HostStore;
use crate::i18n::t;
use crate::theme::Tokens;

/// 确认框的宽（原型 `.dlg.confirm`）。
const WIDTH: Pixels = px(420.);

pub struct DeleteHostDialog {
    store: Entity<HostStore>,
    host: HostId,
    name: SharedString,
    /// 把它当作跳板的主机的名字。
    jump_users: Vec<SharedString>,
    focus_handle: FocusHandle,
}

impl DeleteHostDialog {
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = self.host;
        self.store.update(cx, |store, cx| store.remove(host, cx));
        window.close_dialog(cx);
    }
}

impl Focusable for DeleteHostDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DeleteHostDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let paragraph = |text: SharedString| div().child(text);
        div()
            .id("host-delete")
            .test_support()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .text_color(tokens.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(46.))
                    .px(px(18.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("host.delete.title")),
            )
            .child(
                div()
                    .id("host-delete-text")
                    .test_support()
                    .aria_label(self.description())
                    .flex()
                    .flex_col()
                    .px(px(18.))
                    .pb(px(10.))
                    .text_size(px(12.5))
                    .line_height(relative(1.6))
                    .text_color(tokens.muted)
                    .child(paragraph(t!("host.delete.question", name = self.name)))
                    .when(!self.jump_users.is_empty(), |text| {
                        text.child(paragraph(t!(
                            "host.delete.jump_users",
                            count = self.jump_users.len()
                        )))
                        .children(self.jump_users.iter().map(|name| {
                            div().pl(px(18.)).child(format!("\u{2022} {name}"))
                        }))
                        .child(paragraph(t!("host.delete.jump_users_after")))
                    })
                    .child(paragraph(t!("host.delete.cards_stay"))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(12.))
                    .pb(px(14.))
                    .pl(px(18.))
                    .pr(px(14.))
                    .child(div().flex_1())
                    .child(
                        Button::new("host-delete-cancel")
                            .small()
                            .label(t!("host.delete.cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("host-delete-confirm")
                            .danger()
                            .small()
                            .label(t!("host.delete.confirm"))
                            .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                    ),
            )
    }
}

impl DeleteHostDialog {
    /// 确认框里的全部文字，一行一句，测试靠它断言。
    fn description(&self) -> SharedString {
        let mut lines = vec![t!("host.delete.question", name = self.name)];
        if !self.jump_users.is_empty() {
            lines.push(t!("host.delete.jump_users", count = self.jump_users.len()));
            lines.extend(self.jump_users.iter().cloned());
            lines.push(t!("host.delete.jump_users_after"));
        }
        lines.push(t!("host.delete.cards_stay"));
        lines.join("\n").into()
    }
}

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
        let users = data
            .jumping_through(&[host])
            .into_iter()
            .filter_map(|id| data.host(id).map(|host| host.name.clone()))
            .collect();
        (name, users)
    };
    let view = cx.new(|cx| DeleteHostDialog {
        store,
        host,
        name,
        jump_users,
        focus_handle: cx.focus_handle(),
    });
    let focus = view.read(cx).focus_handle.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .w(WIDTH)
            .p_0()
            .rounded(px(14.))
            .close_button(false)
            .overlay_closable(false)
            .child(view.clone())
    });
    window.focus(&focus, cx);
}
