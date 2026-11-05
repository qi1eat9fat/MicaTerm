//! 主机对话框的「常规」组：名称（留空时用地址）、地址、端口。

use gpui_kit::component::input::InputState;
use gpui_kit::*;

use qishell_hosts::HostDraft;

use super::HostDialog;
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::form::{FieldErrors, FieldNote, form_field, form_group, text_input};

pub const FIELD_NAME: &str = "host-name";
pub const FIELD_ADDRESS: &str = "host-address";
pub const FIELD_PORT: &str = "host-port";

/// 端口输入框的宽度（原型 `.in.narrow`）。
const PORT_WIDTH: Pixels = px(80.);

pub(super) struct GeneralSection {
    pub(super) name: Entity<InputState>,
    address: Entity<InputState>,
    port: Entity<InputState>,
}

impl GeneralSection {
    pub(super) fn new(
        draft: &HostDraft,
        window: &mut Window,
        cx: &mut Context<HostDialog>,
    ) -> Self {
        Self {
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("host.dialog.name_placeholder"))
                    .default_value(draft.name.clone())
            }),
            address: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("host.dialog.address_placeholder"))
                    .default_value(draft.address.clone())
            }),
            port: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(qishell_hosts::SSH_PORT.to_string())
                    .default_value(draft.port.to_string())
            }),
        }
    }

    /// 各输入框和它的字段名：改了就收起那个字段的错误。
    pub(super) fn inputs(&self) -> Vec<(&'static str, Entity<InputState>)> {
        vec![
            (FIELD_NAME, self.name.clone()),
            (FIELD_ADDRESS, self.address.clone()),
            (FIELD_PORT, self.port.clone()),
        ]
    }

    /// 把这一组的值写进 `draft`；端口不是 1–65535 的数时记下错误。地址、名称的规则由
    /// `HostDraft::validated` 统一查。
    pub(super) fn fill(&self, draft: &mut HostDraft, errors: &mut FieldErrors, cx: &App) {
        draft.name = self.name.read(cx).value();
        draft.address = self.address.read(cx).value();
        match parse_port(&self.port.read(cx).value()) {
            Some(port) => draft.port = port,
            None => errors.set(FIELD_PORT, t!("host.draft.port")),
        }
    }

    /// 这台主机现在叫什么：名称，留空时是地址（跳板路线的最后一站，工单 15）。
    pub(super) fn shown_name(&self, cx: &App) -> SharedString {
        let name = self.name.read(cx).value();
        let name = name.trim();
        if name.is_empty() {
            let address = self.address.read(cx).value();
            match address.trim() {
                "" => t!("host.dialog.this_host"),
                address => address.to_string().into(),
            }
        } else {
            name.to_string().into()
        }
    }

    pub(super) fn render(&self, errors: &FieldErrors, tokens: &Tokens) -> AnyElement {
        let address_error = errors.get(FIELD_ADDRESS).is_some();
        let port_error = errors.get(FIELD_PORT).is_some();
        form_group(
            t!("host.dialog.general"),
            [
                form_field(
                    Some(t!("host.dialog.name")),
                    text_input(
                        FIELD_NAME,
                        t!("host.dialog.name"),
                        &self.name,
                        false,
                        tokens,
                    ),
                    Vec::new(),
                    tokens,
                ),
                form_field(
                    Some(t!("host.dialog.address")),
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(div().flex_1().min_w_0().child(text_input(
                            FIELD_ADDRESS,
                            t!("host.dialog.address"),
                            &self.address,
                            address_error,
                            tokens,
                        )))
                        .child(div().flex_none().w(PORT_WIDTH).child(text_input(
                            FIELD_PORT,
                            t!("host.dialog.port"),
                            &self.port,
                            port_error,
                            tokens,
                        ))),
                    [
                        FieldNote::error_of(FIELD_ADDRESS, errors),
                        FieldNote::error_of(FIELD_PORT, errors),
                    ]
                    .into_iter()
                    .flatten()
                    .collect(),
                    tokens,
                ),
            ],
            tokens,
        )
    }
}

/// 端口框里的值，是 1 到 65535 的数时（来自 shell-rs 的 `shared/dialog.rs`）。
pub fn parse_port(port: &str) -> Option<u16> {
    port.trim().parse::<u16>().ok().filter(|port| *port != 0)
}

#[cfg(test)]
mod tests {
    use super::parse_port;

    #[test]
    fn a_port_is_a_number_from_1_to_65535() {
        assert_eq!(parse_port("22"), Some(22));
        assert_eq!(parse_port(" 8080 "), Some(8080));
        assert_eq!(parse_port("65535"), Some(65535));
        for port in ["0", "65536", "ssh", ""] {
            assert_eq!(parse_port(port), None, "{port:?}");
        }
    }
}
