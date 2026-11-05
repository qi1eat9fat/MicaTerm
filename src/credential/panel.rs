//! 凭据面板（原型 `credPanel`）：右侧栏「凭据」展开约 290 宽的卡片。标题栏是图标、「凭据」和「+」；
//! 下面先是保险库状态条（未设主密码 / 已锁定（琥珀色底）/ 已解锁），再是凭据列表：每行类型图标、
//! 名称，下一行写类型（密钥写算法）和用户名，右边「N 台主机」。保险库锁着时名称、类型、用户名照常
//! 显示（它们不是秘密）。
//!
//! 单击选中，双击或回车编辑，↑↓ 移动选中；右键菜单：编辑… / 复制公钥（只有密钥）/ 删除。
//! 按钮和菜单都经窗口壳的焦点派发 action。
// 参考 since2006/shell-rs @ b2a307a: src/credential/credential_panel.rs 重写（去掉搜索框和
// 「生成密钥…」入口，加保险库状态条；右键菜单照 `HostList` 自己持有菜单实体）。

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{Icon, IconName, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{kind_icon, summary};
use crate::app::{
    CatalogIcon, CopyCredentialPublicKey, DeleteCredential, EditCredential, LockVault,
    NewCredential, ResetVault, SetMasterPassword, UnlockVault,
};
use crate::host::{Credential, CredentialId, CredentialKind, HostStore};
use crate::i18n::t;
use crate::theme::{CARD_RADIUS, GAP, RIGHT_PANEL_WIDTH, Tokens};
use crate::vault::{VaultStatus, VaultStore};

/// 面板标题栏的高（`.panel .ph`）。
const HEADER_HEIGHT: Pixels = px(38.);
/// 标题栏「+」的边长（`.ib.sm`）。
const SMALL_BUTTON: Pixels = px(22.);

/// 凭据面板。窗口壳建一个，展开时画它。
pub struct CredentialPanel {
    store: Entity<HostStore>,
    vault: Entity<VaultStore>,
    /// 窗口壳的焦点：按钮、菜单的 action 经它派发。
    target: FocusHandle,
    focus_handle: FocusHandle,
    selected: Option<CredentialId>,
    menu: Option<OpenMenu>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

/// 开着的右键菜单。菜单实体由面板持有，关菜单时一起放掉。
struct OpenMenu {
    credential: CredentialId,
    position: Point<Pixels>,
    view: Entity<PopupMenu>,
    _dismiss: Subscription,
}

/// 保险库状态条的一种样子：文字、右边的链接和它派发的 action。
struct VaultBar {
    icon: CatalogIcon,
    text: SharedString,
    action_label: SharedString,
    action: Box<dyn Action>,
    /// 已锁定：琥珀色底。
    locked: bool,
    /// 已解锁：选中色底；未设主密码：分组底加细描边。
    unlocked: bool,
}

impl CredentialPanel {
    pub fn new(
        store: Entity<HostStore>,
        vault: Entity<VaultStore>,
        target: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe(&store, |this, store, cx| {
                // 选中的凭据删掉了，就不再选中它。
                if let Some(id) = this.selected
                    && store.read(cx).credential(id).is_none()
                {
                    this.selected = None;
                }
                cx.notify();
            }),
            cx.observe(&vault, |_, _, cx| cx.notify()),
        ];
        Self {
            store,
            vault,
            target,
            focus_handle: cx.focus_handle(),
            selected: None,
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

    /// 在 `position`（窗口坐标）为 `credential` 开右键菜单，顺带选中它；之前开着的那个放掉。
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
            |this, menu: &Entity<PopupMenu>, _: &DismissEvent, window, cx| {
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
        self.selected = Some(credential);
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
            .store
            .read(cx)
            .credentials()
            .iter()
            .map(|credential| credential.id)
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

    /// 保险库状态条：没有保险库（秘密在别处）时不画。
    fn vault_bar(&self, cx: &App) -> Option<VaultBar> {
        let status = self.vault.read(cx).status()?;
        let bar = |icon, text, action_label, action: Box<dyn Action>| VaultBar {
            icon,
            text,
            action_label,
            action,
            locked: status == VaultStatus::Locked,
            unlocked: matches!(status, VaultStatus::Unlocked { .. }),
        };
        Some(match status {
            VaultStatus::Absent | VaultStatus::Unprotected => bar(
                CatalogIcon::Shield,
                t!("credential.vault.unset"),
                t!("credential.vault.unset_action"),
                Box::new(SetMasterPassword),
            ),
            VaultStatus::KeyLost => bar(
                CatalogIcon::Shield,
                t!("credential.vault.key_lost"),
                t!("credential.vault.key_lost_action"),
                Box::new(ResetVault),
            ),
            VaultStatus::Locked => bar(
                CatalogIcon::Lock,
                t!("credential.vault.locked"),
                t!("credential.vault.locked_action"),
                Box::new(UnlockVault),
            ),
            VaultStatus::Unlocked { remembered } => bar(
                CatalogIcon::LockOpen,
                if remembered {
                    t!("credential.vault.unlocked_remembered")
                } else {
                    t!("credential.vault.unlocked")
                },
                t!("credential.vault.unlocked_action"),
                Box::new(LockVault),
            ),
        })
    }

    fn render_header(&self, tokens: &Tokens) -> impl IntoElement + use<> {
        let target = self.target.clone();
        let hover = tokens.hover;
        let text = tokens.text;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(HEADER_HEIGHT)
            .pl(px(12.))
            .pr(px(6.))
            .border_b_1()
            .border_color(tokens.line)
            .child(
                Icon::new(CatalogIcon::KeyRound)
                    .size(px(16.))
                    .text_color(tokens.icon),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("credential.panel.title")),
            )
            .child(
                div()
                    .id("credentials-new")
                    .test_support()
                    .aria_label(t!("credential.panel.new"))
                    .ml_auto()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(SMALL_BUTTON)
                    .rounded(px(7.))
                    .text_color(tokens.icon)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover).text_color(text))
                    .on_click(move |_, window, cx| {
                        target.dispatch_action(&NewCredential, window, cx)
                    })
                    .child(Icon::new(IconName::Plus).size(px(14.))),
            )
    }

    /// 状态条（原型 `.vault`）：整条可点，点了和右边的链接一样。
    fn render_vault_bar(&self, bar: VaultBar, tokens: &Tokens) -> impl IntoElement + use<> {
        let target = self.target.clone();
        let action = bar.action.boxed_clone();
        div()
            .id("credentials-vault")
            .test_support()
            .aria_label(bar.text.clone())
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .mx(px(2.))
            .mt(px(2.))
            .mb(px(8.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(10.))
            .text_size(px(12.))
            .cursor_pointer()
            .map(|row| {
                if bar.locked {
                    row.bg(tokens.card_focused.blend(tokens.attention.opacity(0.16)))
                        .text_color(tokens.text)
                } else if bar.unlocked {
                    row.bg(tokens.selection).text_color(tokens.text)
                } else {
                    row.bg(tokens.group)
                        .border_1()
                        .border_color(tokens.line)
                        .text_color(tokens.muted)
                }
            })
            .on_click(move |_, window, cx| target.dispatch_action(action.as_ref(), window, cx))
            .child(Icon::new(bar.icon).size(px(15.)).flex_none())
            .child(div().flex_1().min_w_0().child(bar.text))
            .child(
                div()
                    .id("credentials-vault-action")
                    .test_support()
                    .aria_label(bar.action_label.clone())
                    .flex_none()
                    .whitespace_nowrap()
                    .text_color(tokens.accent)
                    .child(bar.action_label),
            )
    }

    /// 一条凭据（`.it`）：类型图标、名称，下一行类型和用户名，右边「N 台主机」。
    fn render_row(
        &self,
        credential: &Credential,
        hosts: usize,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = credential.id;
        let selected = self.selected == Some(id);
        let pressed = self.menu.as_ref().is_some_and(|menu| menu.credential == id);
        let detail = summary(credential);
        let usage = if hosts == 0 {
            t!("credential.panel.unused")
        } else {
            t!("credential.panel.hosts", count = hosts)
        };
        let label = format!("{} · {detail} · {usage}", credential.name);
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
            .gap(px(8.))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(8.))
            .cursor_pointer()
            .map(|row| {
                if selected || pressed {
                    row.bg(tokens.selection)
                } else {
                    row.hover(move |style| style.bg(hover))
                }
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
                    cx.stop_propagation();
                    this.open_menu(id, event.position, window, cx);
                }),
            )
            .child(
                Icon::new(kind_icon(credential.kind))
                    .size(px(16.))
                    .flex_none()
                    .text_color(tokens.icon),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.5))
                            .child(credential.name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(tokens.faint)
                            .child(detail),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_size(px(11.))
                    .text_color(tokens.faint)
                    .child(usage),
            )
    }
}

/// 一条凭据的右键菜单：编辑… / 复制公钥（只有密钥）/ 删除。菜单派发的 action 送到 `target`。
fn build_menu(
    credential: CredentialId,
    kind: CredentialKind,
    target: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let target = target.clone();
    PopupMenu::build(window, cx, move |menu, _, _| {
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
            .menu(
                t!("credential.menu.delete"),
                Box::new(DeleteCredential(credential)),
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
        let rows: Vec<(Credential, usize)> = {
            let store = self.store.read(cx);
            store
                .credentials()
                .iter()
                .map(|credential| (credential.clone(), store.hosts_using(credential.id).count()))
                .collect()
        };
        let empty = rows.is_empty().then(|| render_empty(&tokens));
        let rows: Vec<AnyElement> = rows
            .iter()
            .map(|(credential, hosts)| {
                self.render_row(credential, *hosts, &tokens, cx)
                    .into_any_element()
            })
            .collect();
        let bar = self
            .vault_bar(cx)
            .map(|bar| self.render_vault_bar(bar, &tokens));
        div()
            .id("credentials")
            .test_support()
            .key_context("CredentialPanel")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key))
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(RIGHT_PANEL_WIDTH)
            .ml(GAP)
            .rounded(CARD_RADIUS)
            .overflow_hidden()
            .bg(tokens.card_focused)
            .border_1()
            .border_color(tokens.card_border)
            .shadow(vec![BoxShadow {
                color: tokens.shadow_focused,
                offset: point(px(0.), px(6.)),
                blur_radius: px(22.),
                spread_radius: px(-2.),
                inset: false,
            }])
            .child(self.render_header(&tokens))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .p(px(6.))
                    .children(bar)
                    .child(
                        div()
                            .id("credentials-list")
                            .test_support()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .children(rows)
                            .children(empty),
                    ),
            )
            .children(self.menu.as_ref().map(|menu| {
                deferred(
                    anchored()
                        .position(menu.position)
                        .snap_to_window_with_margin(px(8.))
                        .child(menu.view.clone()),
                )
                .with_priority(POPUP_PRIORITY)
            }))
    }
}

/// 还没有凭据时的那句话（`.empty`）。
fn render_empty(tokens: &Tokens) -> AnyElement {
    let text = t!("credential.panel.empty");
    div()
        .id("credentials-empty")
        .test_support()
        .aria_label(text.clone())
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
        .px(px(16.))
        .py(px(30.))
        .text_size(px(12.5))
        .text_color(tokens.faint)
        .child(text)
        .child(
            div()
                .text_size(px(11.5))
                .text_center()
                .child(t!("credential.panel.empty_hint")),
        )
        .into_any_element()
}
