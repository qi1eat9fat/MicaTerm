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

use gpui_kit::component::button::Button;
use gpui_kit::component::input::InputState;
use gpui_kit::component::{Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use zeroize::Zeroizing;

use qishell_hosts::{HostDraft, HostId, ProxyKind, ProxySettings, Route, SecretRef};

use super::HostDialog;
use super::form::{FieldErrors, FieldNote, form_field, text_input};
use super::general::parse_port;
use crate::host::HostStore;
use crate::i18n::t;
use crate::settings::segmented;
use crate::theme::Tokens;

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

/// 代理端口输入框的宽度（原型 `.in.narrow`）。
const PORT_WIDTH: Pixels = px(80.);
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

/// 代理类型的显示名，和 `ProxyKind::ALL` 一一对应。
fn proxy_kind_label(kind: ProxyKind) -> SharedString {
    match kind {
        ProxyKind::Http => "HTTP".into(),
        ProxyKind::Socks5 => "SOCKS5".into(),
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
    picking: bool,
    proxy_kind: ProxyKind,
    proxy_host: Entity<InputState>,
    proxy_port: Entity<InputState>,
    proxy_user: Entity<InputState>,
    proxy_password: Entity<InputState>,
    /// 编辑的主机原来的代理密码条目，和保险库里存的值：值是 `None` 时还没读回来
    /// （这时空的密码框不表示「删掉」），`Some(None)` 是没存过。
    saved_proxy: Option<(SecretRef, Option<Option<Zeroizing<String>>>)>,
    _load: Option<Task<()>>,
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
        let proxy_password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t!("host.dialog.optional"))
        });
        let saved_secret = proxy.as_ref().and_then(ProxySettings::password_secret);
        let load = saved_secret
            .clone()
            .map(|secret| load_saved_proxy_password(store, secret, window, cx));
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
            saved_proxy: saved_secret.map(|secret| (secret, None)),
            _load: load,
        }
    }

    /// 各输入框和它的字段名：改了就收起那个字段的错误。
    pub(super) fn inputs(&self) -> Vec<(&'static str, Entity<InputState>)> {
        vec![
            (FIELD_PROXY_HOST, self.proxy_host.clone()),
            (FIELD_PROXY_PORT, self.proxy_port.clone()),
            (FIELD_PROXY_USER, self.proxy_user.clone()),
            (FIELD_PROXY_PASSWORD, self.proxy_password.clone()),
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
                if user.is_empty() && !self.proxy_password.read(cx).value().is_empty() {
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
    pub(super) fn typed_proxy_password(&self, cx: &App) -> SharedString {
        match self.choice {
            RouteChoice::Proxy => self.proxy_password.read(cx).value(),
            _ => SharedString::default(),
        }
    }

    /// 保存时代理密码要怎么改：`None` 不动；`Some((条目, None))` 删掉；`Some((条目, Some(..)))` 写进去。
    /// `route` 是校验过、要保存的路线。
    pub(super) fn proxy_password_change(
        &self,
        route: &Route,
        cx: &App,
    ) -> Option<(SecretRef, Option<String>)> {
        let Route::Proxy(proxy) = route else {
            return None;
        };
        let secret = proxy.password_secret()?;
        let typed = self.proxy_password.read(cx).value();
        // 原来存的值，只在条目没变、也已经读回来时才算数。
        let saved = match &self.saved_proxy {
            Some((original, Some(saved))) if *original == secret => Some(saved.as_ref()),
            _ => None,
        };
        match (saved, typed.is_empty()) {
            (Some(Some(_)), true) => Some((secret, None)),
            (_, true) => None,
            (Some(Some(saved)), false) if saved.as_str() == typed.as_ref() => None,
            (_, false) => Some((secret, Some(typed.to_string()))),
        }
    }

    fn set_saved_proxy_password(
        &mut self,
        saved: Option<Zeroizing<String>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 读回来之前用户已经输了，就不盖掉。
        if self.proxy_password.read(cx).value().is_empty()
            && let Some(saved) = &saved
        {
            let value = SharedString::from(saved.to_string());
            self.proxy_password
                .update(cx, |field, cx| field.set_value(value, window, cx));
        }
        if let Some((_, loaded)) = &mut self.saved_proxy {
            *loaded = Some(saved);
        }
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
            None,
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
            ),
            FieldNote::error_of(ROUTE_CHOICE, errors)
                .into_iter()
                .collect(),
            tokens,
        )];
        match choice {
            RouteChoice::Direct => {}
            RouteChoice::Jump => rows.extend(self.render_jumps(errors, tokens, this_host, cx)),
            RouteChoice::Proxy => rows.extend(self.render_proxy(errors, tokens, cx)),
        }
        super::form::form_group(t!("host.dialog.route"), rows, tokens)
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
            .gap(px(4.))
            .min_h(px(30.))
            .text_size(px(12.))
            .text_color(tokens.muted)
            .children(
                stops
                    .into_iter()
                    .enumerate()
                    .flat_map(|(index, (name, pill, gone))| {
                        let stop = if pill {
                            div()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded_full()
                                .bg(tokens.field)
                                .border_1()
                                .border_color(tokens.field_border)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if gone { tokens.bad } else { tokens.text })
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
                    .gap(px(8.))
                    .mb(px(4.))
                    .py(px(4.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .rounded(px(8.))
                    .bg(tokens.field)
                    .text_size(px(12.5))
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
                            .gap(px(6.))
                            .child(
                                div()
                                    .truncate()
                                    .when(host.is_none(), |name| name.text_color(tokens.bad))
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
            .child(
                div().child(
                    Button::new(ADD_JUMP)
                        .small()
                        .icon(IconName::Plus)
                        .label(t!("host.dialog.add_jump_host"))
                        .on_click(cx.listener(|dialog, _, _, cx| {
                            dialog.route.picking = !dialog.route.picking;
                            cx.notify();
                        })),
                ),
            )
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
            div()
                .id("host-jump-picker-empty")
                .test_support()
                .aria_label(note.clone())
                .px(px(10.))
                .py(px(6.))
                .text_size(px(12.))
                .text_color(tokens.faint)
                .child(note)
        });
        div()
            .id("host-jump-picker")
            .test_support()
            .flex()
            .flex_col()
            .mt(px(4.))
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens.line)
            .children(available.into_iter().map(|host| {
                let id = host.id;
                div()
                    .id((JUMP_CANDIDATE, id.0 as usize))
                    .test_support()
                    .aria_label(host.name.clone())
                    .flex()
                    .gap(px(6.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .text_size(px(12.5))
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
                        .map(proxy_kind_label)
                        .collect(),
                    ProxyKind::ALL
                        .iter()
                        .position(|candidate| *candidate == kind)
                        .unwrap_or(0),
                    choose_kind,
                    tokens,
                ),
                Vec::new(),
                tokens,
            ),
            form_field(
                Some(t!("host.dialog.proxy_address")),
                div()
                    .flex()
                    .gap(px(6.))
                    .child(div().flex_1().min_w_0().child(text_input(
                        FIELD_PROXY_HOST,
                        t!("host.dialog.proxy_address"),
                        &self.proxy_host,
                        invalid(FIELD_PROXY_HOST),
                        tokens,
                    )))
                    .child(div().flex_none().w(PORT_WIDTH).child(text_input(
                        FIELD_PROXY_PORT,
                        t!("host.dialog.proxy_port"),
                        &self.proxy_port,
                        invalid(FIELD_PROXY_PORT),
                        tokens,
                    ))),
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
                text_input(
                    FIELD_PROXY_USER,
                    t!("host.dialog.proxy_user"),
                    &self.proxy_user,
                    invalid(FIELD_PROXY_USER),
                    tokens,
                ),
                FieldNote::error_of(FIELD_PROXY_USER, errors)
                    .into_iter()
                    .collect(),
                tokens,
            ),
            form_field(
                Some(t!("host.dialog.password")),
                text_input(
                    FIELD_PROXY_PASSWORD,
                    t!("host.dialog.proxy_password"),
                    &self.proxy_password,
                    false,
                    tokens,
                ),
                Vec::new(),
                tokens,
            ),
        ]
    }
}

/// 跳板行右边的小图标按钮（原型 `.ib.sm`）。不能用时变淡、点了没反应。
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
        .aria_label(label)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(22.))
        .rounded(px(6.))
        .text_color(if enabled { tokens.icon } else { tokens.faint })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(hover).text_color(text))
                .on_click(on_click)
        })
        .child(Icon::new(icon).size(px(14.)))
}

/// 在后台读编辑的主机原来存的代理密码，读回来填进密码框。读不出（锁着、坏了）当作没存。
fn load_saved_proxy_password(
    store: &Entity<HostStore>,
    secret: SecretRef,
    window: &mut Window,
    cx: &mut Context<HostDialog>,
) -> Task<()> {
    let read = store.update(cx, |store, cx| store.read_secret(secret, cx));
    cx.spawn_in(window, async move |this, cx| {
        let saved = read.await;
        this.update_in(cx, |dialog, window, cx| {
            dialog.route.set_saved_proxy_password(saved, window, cx);
            cx.notify();
        })
        .ok();
    })
}

#[cfg(test)]
mod tests {
    use super::RouteChoice;
    use qishell_hosts::{HostId, ProxyKind, ProxySettings, Route};

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
