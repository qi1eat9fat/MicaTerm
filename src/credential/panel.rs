//! 凭据面板（26 号「凭据」，停靠面板 `ui::dock`）：右侧图标栏「凭据」展开。标题行是图标 +「凭据」+「+」「收起」；
//! 下面按类型分组（密钥 / 密码 / SSH Agent），分组标题点一下收起；每条一行（`.ci`）：类型图标、名称、
//! 淡色「用户名 · 算法」，右边「N 台」（没有主机在用时更淡的「未使用」）。行高 24，名称 12、其余 11，按基线对齐。
//! 保险库的状态只在主机栏左下角显示，这里不重复；锁着时名称、用户名照常显示（它们不是秘密）。
//!
//! 单击选中，双击或回车编辑，↑↓ 按显示的顺序移动选中；右键菜单：编辑… / 复制公钥（只有密钥）/ 删除，
//! 被右键的那一行描一圈细边，选中项不变。按钮和菜单都经窗口壳的焦点派发 action。
// 参考 since2006/shell-rs @ b2a307a: src/credential/credential_panel.rs 重写（去掉搜索框和
// 「生成密钥…」入口；右键菜单照 `HostList` 自己持有菜单实体）。

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::{Icon, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use micaterm_ui::{Menu, WINDOW_MARGIN};

use super::{kind_icon, kind_label, summary};
use crate::app::{
    CatalogIcon, CopyCredentialPublicKey, DeleteCredential, EditCredential, NewCredential,
    ShowCredentials,
};
use crate::host::{Credential, CredentialId, CredentialKind, HostStore};
use crate::i18n::t;
use crate::theme::space;
use crate::theme::{Tokens, TypeScale, WEIGHT_NORMAL, size};
use crate::ui::dock::{DockClose, DockNew, dock_empty, dock_group, dock_header, dock_panel};

/// 分组的顺序（26 号）：密钥、密码、SSH Agent。
const GROUPS: [CredentialKind; 3] = [
    CredentialKind::Key,
    CredentialKind::Password,
    CredentialKind::Agent,
];

/// 凭据面板。窗口壳建一个，展开时画它。
pub struct CredentialPanel {
    store: Entity<HostStore>,
    /// 窗口壳的焦点：按钮、菜单的 action 经它派发。
    target: FocusHandle,
    focus_handle: FocusHandle,
    selected: Option<CredentialId>,
    /// 收起的分组。
    collapsed: Vec<CredentialKind>,
    menu: Option<OpenMenu>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

/// 开着的右键菜单。菜单实体由面板持有，关菜单时一起放掉。
struct OpenMenu {
    credential: CredentialId,
    position: Point<Pixels>,
    view: Entity<Menu>,
    _dismiss: Subscription,
}

impl CredentialPanel {
    pub fn new(store: Entity<HostStore>, target: FocusHandle, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&store, |this, store, cx| {
            // 选中的凭据删掉了，就不再选中它。
            if let Some(id) = this.selected
                && store.read(cx).credential(id).is_none()
            {
                this.selected = None;
            }
            cx.notify();
        })];
        Self {
            store,
            target,
            focus_handle: cx.focus_handle(),
            selected: None,
            collapsed: Vec::new(),
            menu: None,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// 选中的凭据。
    pub fn selected(&self) -> Option<CredentialId> {
        self.selected
    }

    /// 面板的焦点：展开时交给它，回车、↑↓ 才有去处。
    pub fn panel_focus(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    /// 在 `position`（窗口坐标）为 `credential` 开右键菜单（选中项不变）；之前开着的那个放掉。
    fn open_menu(
        &mut self,
        credential: CredentialId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kind) = self
            .store
            .read(cx)
            .credential(credential)
            .map(|credential| credential.kind)
        else {
            return;
        };
        let view = build_menu(credential, kind, &self.target, window, cx);
        let dismiss = cx.subscribe_in(
            &view,
            window,
            |this, menu: &Entity<Menu>, _: &DismissEvent, window, cx| {
                if this.menu.as_ref().is_some_and(|open| &open.view == menu) {
                    this.menu = None;
                    // 选的是「编辑…」「删除」时对话框已经开了，焦点留在对话框里。
                    if !window.has_active_dialog(cx) {
                        window.focus(&this.focus_handle, cx);
                    }
                }
                cx.notify();
            },
        );
        window.focus(&view.focus_handle(cx), cx);
        self.menu = Some(OpenMenu {
            credential,
            position,
            view,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    /// 回车编辑选中的凭据，↑↓ 移动选中。
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        match event.keystroke.key.as_str() {
            "enter" => {
                if let Some(id) = self.selected {
                    self.target.dispatch_action(&EditCredential(id), window, cx);
                    cx.stop_propagation();
                }
            }
            "up" => {
                self.move_selection(false, cx);
                cx.stop_propagation();
            }
            "down" => {
                self.move_selection(true, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    /// 选中上一行或下一行，到头就停；没选中时向下落在第一行、向上落在最后一行。
    fn move_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        let ids: Vec<CredentialId> = self
            .shown(cx)
            .iter()
            .map(|(credential, _)| credential.id)
            .collect();
        let Some(last) = ids.len().checked_sub(1) else {
            return;
        };
        let current = self
            .selected
            .and_then(|id| ids.iter().position(|each| *each == id));
        let next = match (current, down) {
            (Some(index), true) => (index + 1).min(last),
            (Some(index), false) => index.saturating_sub(1),
            (None, true) => 0,
            (None, false) => last,
        };
        self.selected = Some(ids[next]);
        self.scroll.scroll_to_item(next);
        cx.notify();
    }

    /// 展开着的分组里的凭据和在用的主机数，按显示的顺序（分组顺序，组内照存的顺序）。
    fn shown(&self, cx: &App) -> Vec<(Credential, usize)> {
        let store = self.store.read(cx);
        GROUPS
            .iter()
            .filter(|kind| !self.collapsed.contains(kind))
            .flat_map(|kind| {
                store
                    .credentials()
                    .iter()
                    .filter(move |credential| credential.kind == *kind)
            })
            .map(|credential| (credential.clone(), store.hosts_using(credential.id).count()))
            .collect()
    }

    fn toggle_group(&mut self, kind: CredentialKind, cx: &mut Context<Self>) {
        if let Some(index) = self.collapsed.iter().position(|each| *each == kind) {
            self.collapsed.remove(index);
        } else {
            self.collapsed.push(kind);
            // 选中的凭据收起来了就不再选中，免得回车编辑一个看不见的。
            if let Some(id) = self.selected
                && self
                    .store
                    .read(cx)
                    .credential(id)
                    .is_some_and(|credential| credential.kind == kind)
            {
                self.selected = None;
            }
        }
        cx.notify();
    }

    /// 一条凭据：左右内缩 4px 微圆角行，左边类型图标 + 正文色名称 + 淡色用户名 + 算法微徽章；
    /// 右边仅在有主机引用时显示绿字主机计数（未使用时留白减噪，无障碍标签仍完整保留）。
    fn render_row(
        &self,
        credential: &Credential,
        hosts: usize,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let scale = TypeScale::current(cx);
        let id = credential.id;
        let selected = self.selected == Some(id);
        let pressed = self.menu.as_ref().is_some_and(|menu| menu.credential == id);
        let usage_long = if hosts == 0 {
            t!("credential.panel.unused")
        } else if hosts == 1 {
            t!("credential.panel.hosts_one", count = hosts)
        } else {
            t!("credential.panel.hosts", count = hosts)
        };
        let label = format!(
            "{} · {} · {usage_long}",
            credential.name,
            summary(credential)
        );
        let algorithm = key_algorithm_badge(credential);
        let has_user = !credential.user.is_empty();
        let hover = tokens.hover;
        let target = self.target.clone();
        div()
            .id(("credential-row", id.0 as usize))
            .test_support()
            .aria_label(SharedString::from(label))
            .aria_selected(selected)
            .flex()
            .flex_none()
            .items_center()
            .gap(space::SPACE_5)
            .h(size::DOCK_ROW)
            .mx(space::SPACE_4)
            .mt(px(1.))
            .pl(px(18.))
            .pr(space::SPACE_8)
            .rounded(size::RADIUS_SM)
            .text_size(scale.sm)
            .line_height(size::DOCK_ROW)
            .text_color(tokens.text)
            .whitespace_nowrap()
            .cursor_pointer()
            .map(|row| {
                if selected {
                    row.bg(tokens.sel)
                } else {
                    row.hover(move |style| style.bg(hover))
                }
            })
            // 被右键的那一行描一圈细边，选中项不变。
            .when(pressed, |row| {
                row.border_1().border_color(tokens.line_strong)
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                window.focus(&this.focus_handle, cx);
                this.selected = Some(id);
                cx.notify();
                if event.click_count() == 2 {
                    target.dispatch_action(&EditCredential(id), window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_menu(id, event.position, window, cx);
                    // 焦点已经给了菜单：别让面板的 track_focus 在冒泡时又抢回去，否则 Esc 到不了菜单。
                    window.prevent_default();
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(size::ICON_MD)
                    .relative()
                    .top(match credential.kind {
                        CredentialKind::Password => px(0.),
                        CredentialKind::Key | CredentialKind::Agent => px(0.5),
                    })
                    .child(
                        Icon::new(kind_icon(credential.kind))
                            .size(size::ICON_SM)
                            .text_color(tokens.icon),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .max_w(relative(0.56))
                    .truncate()
                    .font_weight(WEIGHT_NORMAL)
                    .child(credential.name.clone()),
            )
            .when(has_user, |row| {
                row.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(tokens.st_dim)
                        .child(credential.user.clone()),
                )
            })
            .when_some(algorithm, |row, algorithm| {
                row.child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .px(space::SPACE_4)
                        .h(px(14.))
                        .rounded(size::RADIUS_SM)
                        .bg(tokens.sel)
                        .text_size(scale.xs)
                        .line_height(px(14.))
                        .font_weight(WEIGHT_NORMAL)
                        .text_color(tokens.text2)
                        .child(algorithm),
                )
            })
            .when(hosts > 0, |row| {
                row.child(
                    div()
                        .ml_auto()
                        .pl(space::SPACE_6)
                        .flex_none()
                        .text_size(scale.xs)
                        .font_weight(WEIGHT_NORMAL)
                        .text_color(tokens.ok_text)
                        .child(usage_long),
                )
            })
    }
}

/// 密钥凭据的算法微徽章（如 `ED25519`）；非密钥或读不出公钥时为 `None`。
fn key_algorithm_badge(credential: &Credential) -> Option<SharedString> {
    (credential.kind == CredentialKind::Key)
        .then(|| {
            credential
                .public_key
                .as_deref()
                .and_then(crate::host::key_algorithm)
        })
        .flatten()
        .map(SharedString::from)
}

/// 一条凭据的右键菜单：编辑… / 复制公钥（只有密钥）/ 删除。菜单派发的 action 送到 `target`。
fn build_menu(
    credential: CredentialId,
    kind: CredentialKind,
    target: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Menu> {
    let target = target.clone();
    Menu::build(window, cx, move |menu, _, _| {
        menu.action_context(target)
            .menu(
                t!("credential.menu.edit"),
                Box::new(EditCredential(credential)),
            )
            .when(kind == CredentialKind::Key, |menu| {
                menu.menu(
                    t!("credential.menu.copy_public_key"),
                    Box::new(CopyCredentialPublicKey(credential)),
                )
            })
            .separator()
            .danger(
                t!("credential.menu.delete"),
                Box::new(DeleteCredential(credential)),
                false,
            )
    })
}

impl Focusable for CredentialPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CredentialPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let credentials: Vec<(Credential, usize)> = {
            let store = self.store.read(cx);
            store
                .credentials()
                .iter()
                .map(|credential| (credential.clone(), store.hosts_using(credential.id).count()))
                .collect()
        };
        let empty = credentials.is_empty().then(|| {
            dock_empty(
                "credentials-empty",
                t!("credential.panel.empty"),
                Some(t!("credential.panel.empty_hint")),
                &tokens,
                cx,
            )
        });
        // 每个有凭据的类型一组：分组目录行，展开着时下面是缩进的凭据节点。
        let mut list: Vec<AnyElement> = Vec::new();
        for kind in GROUPS {
            let rows: Vec<&(Credential, usize)> = credentials
                .iter()
                .filter(|(credential, _)| credential.kind == kind)
                .collect();
            if rows.is_empty() {
                continue;
            }
            let collapsed = self.collapsed.contains(&kind);
            let panel = cx.entity().downgrade();
            list.push(dock_group(
                ("credential-group", kind as usize),
                kind_label(kind),
                rows.len(),
                collapsed,
                list.is_empty(),
                move |_, _, cx| {
                    panel
                        .update(cx, |panel, cx| panel.toggle_group(kind, cx))
                        .ok();
                },
                &tokens,
                cx,
            ));
            if !collapsed {
                for (credential, hosts) in rows {
                    list.push(
                        self.render_row(credential, *hosts, &tokens, cx)
                            .into_any_element(),
                    );
                }
            }
        }
        dock_panel("credentials", &tokens)
            .key_context("CredentialPanel")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key))
            .child(dock_header(
                CatalogIcon::KeyRound,
                t!("credential.panel.title"),
                None,
                DockNew {
                    id: "credentials-new",
                    label: t!("credential.panel.new"),
                    action: Box::new(NewCredential),
                },
                DockClose {
                    id: "credentials-close",
                    label: t!("credential.panel.collapse"),
                    action: Box::new(ShowCredentials),
                },
                &self.target,
                &tokens,
                cx,
            ))
            .child(
                div()
                    .id("credentials-list")
                    .test_support()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .pb(space::SPACE_8)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(list)
                    .children(empty),
            )
            .children(self.menu.as_ref().map(|menu| {
                deferred(
                    anchored()
                        .position(menu.position)
                        .snap_to_window_with_margin(WINDOW_MARGIN)
                        .child(menu.view.clone()),
                )
                .with_priority(POPUP_PRIORITY)
            }))
            .test_support()
    }
}
