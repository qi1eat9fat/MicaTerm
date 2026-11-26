// 参考 since2006/shell-rs @ b2a307a: src/host/host_dialog.rs 的 `RouteChoice`、`JumpHost`、`add_hop`、
// `remove_hop`、`committed_route`、`proxy_change`、`route_field`、`jump_box`、`proxy_box`、
// `available_jump_hosts` 重写。
// 裁剪：跳板的选择器不用可搜索的下拉框，「添加跳板主机」在下面展开一列可选的主机；
// 改造：样式按全部界面原型（路线是一串胶囊，跳板一行一台，可以上移、下移、移除）。

//! 主机对话框的「连接方式」组：直接 / SSH 跳板 / 代理（spec 第 4 节，工单 15）。
//!
//! - SSH 跳板：路线「本机 → 跳板… → 当前主机」，跳板列表（一行一台，可排序、移除），
//!   「添加跳板主机」展开可选的主机（除了这台主机自己和已经加了的）。被删除的跳板主机留在
//!   原位，红色的「已删除的主机」，保存前要移除它；
//! - 代理：HTTP / SOCKS5、地址、端口、可选的用户名和密码。密码不进 `HostDraft`，保存时单独
//!   写进保险库，条目按「用户名@代理地址:端口」，同一个代理的主机共用。
//!
//! 跳板和代理不叠加；跳板主机自己的连接方式不起作用，只按这里列的一跳一跳走。

use std::rc::Rc;

use gpui_kit::component::input::InputState;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use zeroize::Zeroizing;

use micaterm_hosts::{HostDraft, HostId, ProxyKind, ProxySettings, Route, SecretRef};

use super::HostDialog;
use super::general::parse_port;
use crate::host::HostStore;
use crate::i18n::t;
use crate::settings::segmented;
use crate::theme::{Tokens, WEIGHT_NORMAL, space, ui_text};
use crate::ui::empty::{EmptyPlacement, EmptyState};
use crate::ui::form::{FieldErrors, FieldNote, form_field, locked_secret};
use crate::ui::icon_button::TextTooltip;
use crate::ui::modal::{ModalInputKind, modal_input};
use crate::vault::{SecretField, VaultStore};

/// 连接方式的分段按钮。
pub const ROUTE_CHOICE: &str = "host-route";
/// 路线「本机 → … → 当前主机」，无障碍名称是整条路线。
pub const ROUTE_CHAIN: &str = "host-route-chain";
/// 跳板列表；跳板的错误（没有跳板、有已删除的）记在它上面。
pub const FIELD_JUMPS: &str = "host-jumps";
/// 跳板列表的一行，`(JUMP_ROW, 第几个)`；无障碍名称是主机名或「已删除的主机」。
pub const JUMP_ROW: &str = "host-jump";
pub const JUMP_UP: &str = "host-jump-up";
pub const JUMP_DOWN: &str = "host-jump-down";
pub const JUMP_REMOVE: &str = "host-jump-remove";
/// 「添加跳板主机」按钮。
pub const ADD_JUMP: &str = "host-jump-add";
/// 展开后可选的一台主机，`(JUMP_CANDIDATE, 主机 id)`。
pub const JUMP_CANDIDATE: &str = "host-jump-candidate";
/// 代理类型的分段按钮。
pub const PROXY_KIND: &str = "host-proxy-kind";
pub const FIELD_PROXY_HOST: &str = "host-proxy-host";
pub const FIELD_PROXY_PORT: &str = "host-proxy-port";
pub const FIELD_PROXY_USER: &str = "host-proxy-user";
pub const FIELD_PROXY_PASSWORD: &str = "host-proxy-password";
/// 保险库锁着时代替代理密码框的那一行，和它的「解锁…」。
pub const PROXY_PASSWORD_LOCKED: &str = "host-proxy-password-locked";
pub const PROXY_PASSWORD_UNLOCK: &str = "host-proxy-password-unlock";

/// 新建代理时端口框里的值（原型）。
const DEFAULT_PROXY_PORT: u16 = 1080;

/// 主机怎么连过去。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum RouteChoice {
    #[default]
    Direct,
    Jump,
    Proxy,
}

impl RouteChoice {
    const ALL: [RouteChoice; 3] = [RouteChoice::Direct, RouteChoice::Jump, RouteChoice::Proxy];

    fn label(self) -> SharedString {
        match self {
            RouteChoice::Direct => t!("host.dialog.route_direct"),
            RouteChoice::Jump => t!("host.dialog.route_jump"),
            RouteChoice::Proxy => t!("host.dialog.route_proxy"),
        }
    }

    fn of(route: &Route) -> Self {
        match route {
            Route::Direct => RouteChoice::Direct,
            Route::Jump(_) => RouteChoice::Jump,
            Route::Proxy(_) => RouteChoice::Proxy,
        }
    }
}

/// 可以当跳板的一台主机，打开对话框时的样子。
#[derive(Clone, Debug)]
struct JumpHost {
    id: HostId,
    name: SharedString,
    /// `user@address:port`，灰字。
    address: SharedString,
}

pub(super) struct RouteSection {
    choice: RouteChoice,
    /// 跳板，按顺序；`None` 是已删除的主机，留在原位。
    hops: Vec<Option<HostId>>,
    /// 可以当跳板的主机：除了编辑的这台以外的全部。
    candidates: Vec<JumpHost>,
    /// 「添加跳板主机」展开着。
    pub(super) picking: bool,
    proxy_kind: ProxyKind,
    proxy_host: Entity<InputState>,
    proxy_port: Entity<InputState>,
    proxy_user: Entity<InputState>,
    /// 代理密码框：条目是编辑的主机原来的代理密码；锁着又存过时显示「已保存，保险库锁定中」。
    proxy_password: SecretField,
    store: Entity<HostStore>,
    _vault: Option<Subscription>,
}

impl RouteSection {
    /// `editing` 是编辑的主机，新建时是 `None`。
    pub(super) fn new(
        draft: &HostDraft,
        editing: Option<HostId>,
        store: &Entity<HostStore>,
        window: &mut Window,
        cx: &mut Context<HostDialog>,
    ) -> Self {
        let candidates = store
            .read(cx)
            .hosts()
            .iter()
            .filter(|host| Some(host.id) != editing)
            .map(|host| JumpHost {
                id: host.id,
                name: host.name.clone(),
                address: format!("{}@{}:{}", host.user, host.address, host.port).into(),
            })
            .collect();
        let hops = match &draft.route {
            Route::Jump(hops) => hops.clone(),
            _ => Vec::new(),
        };
        let proxy = match &draft.route {
            Route::Proxy(proxy) => Some(proxy.clone()),
            _ => None,
        };
        let input = |value: SharedString,
                     placeholder: SharedString,
                     window: &mut Window,
                     cx: &mut Context<HostDialog>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let proxy_host = input(
            proxy.as_ref().map(|p| p.host.clone()).unwrap_or_default(),
            "127.0.0.1".into(),
            window,
            cx,
        );
        let proxy_port = input(
            proxy
                .as_ref()
                .map_or(DEFAULT_PROXY_PORT, |p| p.port)
                .to_string()
                .into(),
            DEFAULT_PROXY_PORT.to_string().into(),
            window,
            cx,
        );
        let proxy_user = input(
            proxy
                .as_ref()
                .and_then(|p| p.user.clone())
                .unwrap_or_default(),
            t!("host.dialog.optional"),
            window,
            cx,
        );
        let mut proxy_password = SecretField::new(
            proxy.as_ref().and_then(ProxySettings::password_secret),
            t!("host.dialog.optional"),
            window,
            cx,
        );
        proxy_password.load_saved(&HostStore::reader(store), window, cx, |dialog| {
            &mut dialog.route.proxy_password
        });
        // 锁着的代理密码解开了就换回密码框、读回存的（工单 13）。
        let watch = VaultStore::global(cx).map(|vault| {
            cx.observe_in(&vault, window, |dialog, _, window, cx| {
                let read = HostStore::reader(&dialog.route.store);
                dialog
                    .route
                    .proxy_password
                    .vault_changed(&read, window, cx, |dialog| &mut dialog.route.proxy_password);
                cx.notify();
            })
        });
        Self {
            choice: RouteChoice::of(&draft.route),
            hops,
            candidates,
            picking: false,
            proxy_kind: proxy.map(|p| p.kind).unwrap_or(ProxyKind::Socks5),
            proxy_host,
            proxy_port,
            proxy_user,
            proxy_password,
            store: store.clone(),
            _vault: watch,
        }
    }

    /// 对话框要关了：清空代理密码框。
    pub(super) fn forget_secrets(&self, window: &mut Window, cx: &mut App) {
        self.proxy_password.clear(window, cx);
    }

    /// 各输入框和它的字段名：改了就收起那个字段的错误。
    pub(super) fn inputs(&self) -> Vec<(&'static str, Entity<InputState>)> {
        vec![
            (FIELD_PROXY_HOST, self.proxy_host.clone()),
            (FIELD_PROXY_PORT, self.proxy_port.clone()),
            (FIELD_PROXY_USER, self.proxy_user.clone()),
            (FIELD_PROXY_PASSWORD, self.proxy_password.input().clone()),
        ]
    }

    /// 把选的连接方式写进 `draft.route`；代理端口不是数、填了密码没填用户名时记下错误。
    /// 没有跳板、有已删除的跳板、代理地址为空由 `HostDraft::validated` 统一查。
    pub(super) fn fill(&self, draft: &mut HostDraft, errors: &mut FieldErrors, cx: &App) {
        draft.route = match self.choice {
            RouteChoice::Direct => Route::Direct,
            RouteChoice::Jump => Route::Jump(self.hops.clone()),
            RouteChoice::Proxy => {
                let port = parse_port(&self.proxy_port.read(cx).value()).unwrap_or_else(|| {
                    errors.set(FIELD_PROXY_PORT, t!("host.draft.proxy_port"));
                    DEFAULT_PROXY_PORT
                });
                let user = self.proxy_user.read(cx).value().trim().to_string();
                if user.is_empty() && !self.proxy_password.typed(cx).is_empty() {
                    errors.set(
                        FIELD_PROXY_USER,
                        t!("host.dialog.proxy_password_needs_user"),
                    );
                }
                let host = self.proxy_host.read(cx).value().trim().to_string();
                Route::Proxy(ProxySettings::new(self.proxy_kind, host, port).with_user(user))
            }
        };
    }

    /// 代理密码框里现在的字（「测试连接」用表单里的，存没存过都一样）。
    pub(super) fn typed_proxy_password(&self, cx: &App) -> Zeroizing<String> {
        match self.choice {
            RouteChoice::Proxy if !self.proxy_password.is_locked() => self.proxy_password.typed(cx),
            _ => Zeroizing::default(),
        }
    }

    /// 保存时代理密码要怎么改：`None` 不动；`Some((条目, None))` 删掉；`Some((条目, Some(..)))` 写进去。
    /// `route` 是校验过、要保存的路线。
    pub(super) fn proxy_password_change(
        &self,
        route: &Route,
        cx: &App,
    ) -> Option<(SecretRef, Option<Zeroizing<String>>)> {
        let Route::Proxy(proxy) = route else {
            return None;
        };
        let secret = proxy.password_secret()?;
        // 原来存的值只在条目没变时才算数（改了代理的用户名、地址、端口就是另一个条目）。
        let change = if self.proxy_password.secret() == Some(&secret) {
            self.proxy_password.change(cx)
        } else {
            self.proxy_password.change_ignoring_saved(cx)
        };
        change.map(|change| (secret, change))
    }

    fn choose(&mut self, choice: RouteChoice, errors: &mut FieldErrors) {
        self.choice = choice;
        self.picking = false;
        for field in [
            FIELD_JUMPS,
            FIELD_PROXY_HOST,
            FIELD_PROXY_PORT,
            FIELD_PROXY_USER,
        ] {
            errors.clear(field);
        }
    }

    /// 还能加的主机：不在跳板列表里的。
    fn available(&self) -> impl Iterator<Item = &JumpHost> {
        self.candidates
            .iter()
            .filter(|host| !self.hops.contains(&Some(host.id)))
    }

    fn add_hop(&mut self, id: HostId, errors: &mut FieldErrors) {
        if !self.hops.contains(&Some(id)) {
            self.hops.push(Some(id));
        }
        self.picking = false;
        errors.clear(FIELD_JUMPS);
    }

    fn remove_hop(&mut self, position: usize, errors: &mut FieldErrors) {
        if position < self.hops.len() {
            self.hops.remove(position);
            errors.clear(FIELD_JUMPS);
        }
    }

    /// 把第 `position` 个跳板和它前一个（`up`）或后一个对调。
    fn move_hop(&mut self, position: usize, up: bool) {
        let other = if up {
            position.checked_sub(1)
        } else {
            Some(position + 1)
        };
        if let Some(other) = other.filter(|other| *other < self.hops.len())
            && position < self.hops.len()
        {
            self.hops.swap(position, other);
        }
    }

    fn hop_host(&self, hop: &Option<HostId>) -> Option<&JumpHost> {
        hop.and_then(|id| self.candidates.iter().find(|host| host.id == id))
    }

    /// 「连接方式」组。`this_host` 是路线最后一站的名字（表单里现在的名称或地址）。
    pub(super) fn render(
        &self,
        errors: &FieldErrors,
        tokens: &Tokens,
        this_host: SharedString,
        window: &Window,
        cx: &mut Context<HostDialog>,
    ) -> AnyElement {
        let dialog = cx.entity().downgrade();
        let choice = self.choice;
        let choose: crate::settings::OnChoose = Rc::new(move |index, _, cx| {
            let Some(choice) = RouteChoice::ALL.get(index).copied() else {
                return;
            };
            dialog
                .update(cx, |dialog, cx| {
                    dialog.route.choose(choice, &mut dialog.errors);
                    cx.notify();
                })
                .ok();
        });
        let mut rows = vec![form_field(
            Some(t!("host.dialog.route")),
            segmented(
                ROUTE_CHOICE,
                RouteChoice::ALL
                    .iter()
                    .map(|choice| choice.label())
                    .collect(),
                RouteChoice::ALL
                    .iter()
                    .position(|candidate| *candidate == choice)
                    .unwrap_or(0),
                choose,
                tokens,
                crate::theme::TypeScale::current(cx),
            ),
            FieldNote::error_of(ROUTE_CHOICE, errors)
                .into_iter()
                .collect(),
            tokens,
        )];
        match choice {
            RouteChoice::Direct => {}
            RouteChoice::Jump => rows.extend(self.render_jumps(errors, tokens, this_host, cx)),
            RouteChoice::Proxy => rows.extend(self.render_proxy(errors, tokens, window, cx)),
        }
        crate::ui::form::form_group(rows)
    }

    /// 路线和跳板列表两行。
    fn render_jumps(
        &self,
        errors: &FieldErrors,
        tokens: &Tokens,
        this_host: SharedString,
        cx: &mut Context<HostDialog>,
    ) -> Vec<AnyElement> {
        // 本机 → jp-gateway → 已删除的主机 → db-master
        let mut stops = vec![(t!("host.dialog.this_computer"), false, false)];
        stops.extend(self.hops.iter().map(|hop| match self.hop_host(hop) {
            Some(host) => (host.name.clone(), true, false),
            None => (t!("host.dialog.deleted_host"), true, true),
        }));
        stops.push((this_host, true, false));
        let chain_label = stops
            .iter()
            .map(|(name, _, _)| name.as_ref())
            .collect::<Vec<_>>()
            .join(" \u{2192} ");
        let count = stops.len();
        let chain = div()
            .id(ROUTE_CHAIN)
            .test_support()
            .aria_label(SharedString::from(chain_label))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(space::SPACE_4)
            .min_h(px(30.))
            .text_size(ui_text())
            .text_color(tokens.text2)
            .children(
                stops
                    .into_iter()
                    .enumerate()
                    .flat_map(|(index, (name, pill, gone))| {
                        let stop = if pill {
                            div()
                                .px(space::SPACE_8)
                                .py(space::SPACE_2)
                                .rounded_full()
                                .bg(tokens.input)
                                .border_1()
                                .border_color(tokens.line_strong)
                                .font_weight(WEIGHT_NORMAL)
                                .text_color(if gone { tokens.err } else { tokens.text })
                                .child(name)
                        } else {
                            div().child(name)
                        };
                        let arrow = (index + 1 < count).then(|| div().child("\u{2192}"));
                        std::iter::once(stop.into_any_element())
                            .chain(arrow.map(IntoElement::into_any_element))
                    }),
            );

        let last = self.hops.len().saturating_sub(1);
        let hop_rows = self
            .hops
            .iter()
            .enumerate()
            .map(|(position, hop)| {
                let host = self.hop_host(hop);
                let name = host
                    .map(|host| host.name.clone())
                    .unwrap_or_else(|| t!("host.dialog.deleted_host"));
                div()
                    .id((JUMP_ROW, position))
                    .test_support()
                    .aria_label(name.clone())
                    .flex()
                    .items_center()
                    .gap(space::SPACE_8)
                    .mb(space::SPACE_4)
                    .py(space::SPACE_4)
                    .pl(space::SPACE_10)
                    .pr(space::SPACE_4)
                    .rounded(crate::theme::size::RADIUS_LG)
                    .bg(tokens.input)
                    .text_size(ui_text())
                    .child(
                        div()
                            .flex_none()
                            .w(px(14.))
                            .text_color(tokens.faint)
                            .child((position + 1).to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .gap(space::SPACE_6)
                            .child(
                                div()
                                    .truncate()
                                    .when(host.is_none(), |name| name.text_color(tokens.err))
                                    .child(name),
                            )
                            .children(host.map(|host| {
                                div()
                                    .truncate()
                                    .text_color(tokens.faint)
                                    .child(host.address.clone())
                            })),
                    )
                    .when(host.is_some(), |row| {
                        row.child(hop_button(
                            (JUMP_UP, position),
                            IconName::ArrowUp,
                            t!("host.dialog.jump_up"),
                            position > 0,
                            tokens,
                            cx.listener(move |dialog, _, _, cx| {
                                dialog.route.move_hop(position, true);
                                cx.notify();
                            }),
                        ))
                        .child(hop_button(
                            (JUMP_DOWN, position),
                            IconName::ArrowDown,
                            t!("host.dialog.jump_down"),
                            position < last,
                            tokens,
                            cx.listener(move |dialog, _, _, cx| {
                                dialog.route.move_hop(position, false);
                                cx.notify();
                            }),
                        ))
                    })
                    .child(hop_button(
                        (JUMP_REMOVE, position),
                        IconName::Close,
                        t!("host.dialog.jump_remove"),
                        true,
                        tokens,
                        cx.listener(move |dialog, _, _, cx| {
                            dialog.route.remove_hop(position, &mut dialog.errors);
                            cx.notify();
                        }),
                    ))
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        let picker = self.picking.then(|| self.render_picker(tokens, cx));
        let list = div()
            .id(FIELD_JUMPS)
            .flex()
            .flex_col()
            .children(hop_rows)
            .child(div().child(add_jump_button(tokens, cx)))
            .children(picker);
        vec![
            form_field(
                Some(t!("host.dialog.route_chain")),
                chain,
                Vec::new(),
                tokens,
            ),
            form_field(
                Some(t!("host.dialog.jump_hosts")),
                list,
                FieldNote::error_of(FIELD_JUMPS, errors)
                    .into_iter()
                    .collect(),
                tokens,
            ),
        ]
    }

    /// 「添加跳板主机」展开的列表：还能加的主机，点一下加到最后。
    fn render_picker(&self, tokens: &Tokens, cx: &mut Context<HostDialog>) -> AnyElement {
        let hover = tokens.hover;
        let available: Vec<_> = self.available().cloned().collect();
        let empty = available.is_empty().then(|| {
            let note = if self.candidates.is_empty() {
                t!("host.dialog.no_other_hosts")
            } else {
                t!("host.dialog.all_hosts_added")
            };
            EmptyState::new(
                "host-jump-picker-empty",
                note,
                EmptyPlacement::RoutePicker,
            )
            .render(tokens, crate::theme::TypeScale::current(cx))
        });
        div()
            .id("host-jump-picker")
            .test_support()
            .flex()
            .flex_col()
            .mt(space::SPACE_4)
            .p(space::SPACE_4)
            .rounded(crate::theme::size::RADIUS_LG)
            .border_1()
            .border_color(tokens.line)
            .children(available.into_iter().map(|host| {
                let id = host.id;
                div()
                    .id((JUMP_CANDIDATE, id.0 as usize))
                    .test_support()
                    .aria_label(host.name.clone())
                    .flex()
                    .gap(space::SPACE_6)
                    .px(space::SPACE_8)
                    .py(space::SPACE_4)
                    .rounded(crate::theme::size::RADIUS_MD)
                    .text_size(ui_text())
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .on_click(cx.listener(move |dialog, _, _, cx| {
                        dialog.route.add_hop(id, &mut dialog.errors);
                        cx.notify();
                    }))
                    .child(div().truncate().child(host.name))
                    .child(
                        div()
                            .truncate()
                            .text_color(tokens.faint)
                            .child(host.address),
                    )
            }))
            .children(empty)
            .into_any_element()
    }

    /// 代理：类型、地址和端口、用户名、密码四行。
    fn render_proxy(
        &self,
        errors: &FieldErrors,
        tokens: &Tokens,
        window: &Window,
        cx: &mut Context<HostDialog>,
    ) -> Vec<AnyElement> {
        let dialog = cx.entity().downgrade();
        let kind = self.proxy_kind;
        let choose_kind: crate::settings::OnChoose = Rc::new(move |index, _, cx| {
            let Some(kind) = ProxyKind::ALL.get(index).copied() else {
                return;
            };
            dialog
                .update(cx, |dialog, cx| {
                    dialog.route.proxy_kind = kind;
                    cx.notify();
                })
                .ok();
        });
        let invalid = |field| errors.get(field).is_some();
        vec![
            form_field(
                Some(t!("host.dialog.proxy_kind")),
                segmented(
                    PROXY_KIND,
                    ProxyKind::ALL
                        .iter()
                        .copied()
                        .map(|kind| SharedString::from(kind.label()))
                        .collect(),
                    ProxyKind::ALL
                        .iter()
                        .position(|candidate| *candidate == kind)
                        .unwrap_or(0),
                    choose_kind,
                    tokens,
                    crate::theme::TypeScale::current(cx),
                ),
                Vec::new(),
                tokens,
            ),
            form_field(
                Some(t!("host.dialog.proxy_address")),
                div()
                    .flex()
                    .gap(space::SPACE_6)
                    .child(div().flex_1().min_w_0().child(modal_input(
                        FIELD_PROXY_HOST,
                        t!("host.dialog.proxy_address"),
                        &self.proxy_host,
                        ModalInputKind::Text,
                        invalid(FIELD_PROXY_HOST),
                        tokens,
                        window,
                        cx,
                    )))
                    .child(
                        div()
                            .flex_none()
                            .w(super::general::PORT_WIDTH)
                            .child(modal_input(
                                FIELD_PROXY_PORT,
                                t!("host.dialog.proxy_port"),
                                &self.proxy_port,
                                ModalInputKind::Text,
                                invalid(FIELD_PROXY_PORT),
                                tokens,
                                window,
                                cx,
                            )),
                    ),
                [
                    FieldNote::error_of(FIELD_PROXY_HOST, errors),
                    FieldNote::error_of(FIELD_PROXY_PORT, errors),
                ]
                .into_iter()
                .flatten()
                .collect(),
                tokens,
            ),
            form_field(
                Some(t!("host.dialog.user")),
                modal_input(
                    FIELD_PROXY_USER,
                    t!("host.dialog.proxy_user"),
                    &self.proxy_user,
                    ModalInputKind::Text,
                    invalid(FIELD_PROXY_USER),
                    tokens,
                    window,
                    cx,
                ),
                FieldNote::error_of(FIELD_PROXY_USER, errors)
                    .into_iter()
                    .collect(),
                tokens,
            ),
            form_field(
                Some(t!("host.dialog.password")),
                if self.proxy_password.is_locked() {
                    locked_secret(
                        PROXY_PASSWORD_LOCKED,
                        PROXY_PASSWORD_UNLOCK,
                        tokens,
                        crate::theme::TypeScale::current(cx),
                    )
                        .into_any_element()
                } else {
                    modal_input(
                        FIELD_PROXY_PASSWORD,
                        t!("host.dialog.proxy_password"),
                        self.proxy_password.input(),
                        ModalInputKind::Secret,
                        false,
                        tokens,
                        window,
                        cx,
                    )
                    .into_any_element()
                },
                Vec::new(),
                tokens,
            ),
        ]
    }
}

/// 跳板行右边的小图标按钮（原型 `.ib.sm`）。不能用时变淡、点了没反应。
/// 「添加跳板主机」：整行宽、高同输入框（28），细边、圆角 6，悬停有底。
fn add_jump_button(tokens: &Tokens, cx: &mut Context<HostDialog>) -> impl IntoElement + use<> {
    let hover = tokens.hover;
    let text = tokens.text;
    let label = t!("host.dialog.add_jump_host");
    div()
        .id(ADD_JUMP)
        .test_support()
        .aria_label(label.clone())
        .role(Role::Button)
        .flex()
        .items_center()
        .justify_center()
        .gap(space::SPACE_6)
        .h(crate::theme::size::INPUT)
        .rounded(crate::theme::size::RADIUS_MD)
        .border_1()
        .border_color(tokens.line_strong)
        .text_color(tokens.text2)
        .text_size(crate::theme::TypeScale::current(cx).sm)
        .line_height(px(16.))
        .cursor_pointer()
        .hover(move |style| style.bg(hover).text_color(text))
        .on_click(cx.listener(|dialog, _, _, cx| {
            dialog.route.picking = !dialog.route.picking;
            cx.notify();
        }))
        .child(Icon::new(IconName::Plus).size(crate::theme::size::ICON_SM))
        .child(label)
}

fn hop_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: SharedString,
    enabled: bool,
    tokens: &Tokens,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (hover, text) = (tokens.hover, tokens.text);
    div()
        .id(id)
        .test_support()
        .aria_label(label.clone())
        .tooltip(move |_, cx| TextTooltip::view(label.clone(), cx))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(crate::theme::size::CONTROL_MD)
        .rounded(crate::theme::size::RADIUS_SM)
        .text_color(if enabled { tokens.icon } else { tokens.faint })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(hover).text_color(text))
                .on_click(on_click)
        })
        .child(Icon::new(icon).size(px(14.)))
}

#[cfg(test)]
mod tests {
    use super::RouteChoice;
    use micaterm_hosts::{HostId, ProxyKind, ProxySettings, Route};

    #[test]
    fn the_choice_follows_the_saved_route() {
        assert_eq!(RouteChoice::of(&Route::Direct), RouteChoice::Direct);
        assert_eq!(
            RouteChoice::of(&Route::Jump(vec![Some(HostId(1))])),
            RouteChoice::Jump
        );
        assert_eq!(
            RouteChoice::of(&Route::Proxy(ProxySettings::new(
                ProxyKind::Http,
                "p",
                8080
            ))),
            RouteChoice::Proxy
        );
    }
}
