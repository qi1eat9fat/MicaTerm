//! 主机对话框的「高级」组（工单 34）：「登录后探测系统」开关，默认开。
//!
//! 关掉时登录后不另开 exec 通道跑 `uname` 等，系统徽章只用已经记下的。有的堡垒机再开一个通道
//! 就断掉整条连接；探测把连接弄断时连接这边会自动关掉它，用户在这里重新打开。

use gpui_kit::component::switch::Switch;
use gpui_kit::*;

use qishell_hosts::HostDraft;

use super::HostDialog;
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::form::{FieldErrors, form_group};

/// 「登录后探测系统」开关，无障碍名称是它的名字。
pub const FIELD_PROBE_OS: &str = "host-probe-os";

pub(super) struct AdvancedSection {
    probe_os: bool,
}

impl AdvancedSection {
    pub(super) fn new(draft: &HostDraft) -> Self {
        Self {
            probe_os: draft.probe_os,
        }
    }

    pub(super) fn fill(&self, draft: &mut HostDraft, _: &mut FieldErrors) {
        draft.probe_os = self.probe_os;
    }

    pub(super) fn render(&self, tokens: &Tokens, cx: &mut Context<HostDialog>) -> AnyElement {
        let label = t!("host.dialog.probe_os");
        let switch = Switch::new(FIELD_PROBE_OS)
            .checked(self.probe_os)
            .accessibility_label(label.clone())
            .on_click(cx.listener(|dialog, checked: &bool, _, cx| {
                dialog.advanced.probe_os = *checked;
                cx.notify();
            }));
        let row = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .mb(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(div().text_size(px(12.5)).child(label))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .line_height(relative(1.45))
                            .text_color(tokens.faint)
                            .child(t!("host.dialog.probe_os_hint")),
                    ),
            )
            .child(div().flex_none().child(switch))
            .into_any_element();
        form_group(t!("host.dialog.advanced"), [row], tokens)
    }
}
