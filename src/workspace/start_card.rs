//! 开始卡片：没有任何标签时（启动时，或关掉最后一个标签后）标签区显示的卡片。
//!
//! - 上面是最近连接列表（工单 18，沿用 shell-rs 开始页 `recent_hosts.rs` 和它的连接记录）：
//!   第一行默认高亮，单击高亮，双击或回车在新标签里连接，↑↓ 移动高亮；
//! - 下面是「新建本地终端」「新建主机」两个按钮，再下面一行灰字写快速连接的快捷键；
//! - 一台主机都没有时只显示欢迎语（一句引导）、两个按钮和快捷键。

use gpui_kit::component::{Icon, kbd::Kbd};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::{CatalogIcon, NewHost, NewLocalTerminalTab, QuickConnect};
use crate::host::{Host, HostId, HostMark, list_address};
use crate::i18n::t;
use crate::theme::{CARD_RADIUS, Tokens};
use crate::workspace::Workspace;

/// 最近连接列表最多列几台（更多的从快速连接里找）。
const RECENT_ROWS: usize = 6;
/// 徽章直径（原型 `.qi .badge`）。
const BADGE: Pixels = px(22.);

/// 标志的渐变：原型 `.logo-i` 的 #7c5cff → #2e9bd0。
pub(super) fn logo(size: Pixels, radius: Pixels) -> Div {
    div()
        .flex_none()
        .size(size)
        .rounded(radius)
        .bg(linear_gradient(
            135.,
            linear_color_stop(rgb(0x7c5cff), 0.),
            linear_color_stop(rgb(0x2e9bd0), 1.),
        ))
}

impl Workspace {
    /// 开始卡片列出的最近连接，最新的在前。
    pub fn start_card_recent(&self, cx: &App) -> Vec<HostId> {
        self.recent_rows(cx).iter().map(|host| host.id).collect()
    }

    /// 开始卡片的最近连接列表高亮的那台。
    pub fn start_card_highlighted(&self, cx: &App) -> Option<HostId> {
        let rows = self.start_card_recent(cx);
        rows.get(self.start_highlight.min(rows.len().saturating_sub(1)))
            .copied()
    }

    fn recent_rows(&self, cx: &App) -> Vec<Host> {
        self.hosts
            .read(cx)
            .recent_hosts()
            .take(RECENT_ROWS)
            .cloned()
            .collect()
    }

    /// 开始卡片上的按键：↑↓ 移动最近连接的高亮，回车连接高亮的那台。
    fn on_start_card_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let count = self.start_card_recent(cx).len();
        if count == 0 {
            return;
        }
        let current = self.start_highlight.min(count - 1);
        match event.keystroke.key.as_str() {
            "up" => self.start_highlight = current.saturating_sub(1),
            "down" => self.start_highlight = (current + 1).min(count - 1),
            "enter" => {
                if let Some(host) = self.start_card_highlighted(cx) {
                    self.connect_from_start_card(host, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// 从开始卡片连接：下次回到开始卡片时高亮又从第一行（刚连的那台）开始。
    fn connect_from_start_card(
        &mut self,
        host: HostId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_highlight = 0;
        self.connect_host(host, window, cx);
    }

    /// 开始卡片。没有标签时它持有焦点（`start_card_focus`），按钮经 `target` 派发 action。
    pub(super) fn render_start_card(
        &self,
        target: &FocusHandle,
        tokens: &Tokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let quick_connect = Kbd::binding_for_action(&QuickConnect, None, window);
        let has_hosts = !self.hosts.read(cx).hosts().is_empty();
        let recent = self.recent_rows(cx);
        let highlighted = self.start_highlight.min(recent.len().saturating_sub(1));
        let top = if has_hosts {
            self.render_recent(recent, highlighted, tokens, cx)
                .into_any_element()
        } else {
            render_hello(tokens).into_any_element()
        };
        div()
            .id("start-card")
            .test_support()
            .track_focus(&self.start_card_focus)
            .aria_label(t!("workspace.start.title"))
            .on_key_down(cx.listener(Self::on_start_card_key))
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .rounded(CARD_RADIUS)
            .bg(tokens.card)
            .border_1()
            .border_color(tokens.card_border)
            .shadow(vec![BoxShadow {
                color: tokens.shadow,
                offset: point(px(0.), px(6.)),
                blur_radius: px(22.),
                spread_radius: px(-2.),
                inset: false,
            }])
            .child(
                div()
                    .w(px(440.))
                    .flex()
                    .flex_col()
                    .child(top)
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .mb(px(14.))
                            .child(big_button(
                                "start-new-local-terminal",
                                CatalogIcon::Terminal,
                                (rgb(0x334155).into(), rgb(0x86efac).into()),
                                t!("workspace.start.new_local_terminal"),
                                t!("workspace.start.new_local_terminal_hint"),
                                Box::new(NewLocalTerminalTab),
                                target,
                                tokens,
                            ))
                            .child(big_button(
                                "start-new-host",
                                CatalogIcon::Plus,
                                (tokens.accent, tokens.on_accent),
                                t!("workspace.start.new_host"),
                                t!("workspace.start.new_host_hint"),
                                Box::new(NewHost),
                                target,
                                tokens,
                            )),
                    )
                    .child(
                        div()
                            .id("start-quick-connect-keys")
                            .test_support()
                            .aria_label(t!("workspace.start.quick_connect"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(6.))
                            .text_size(px(11.5))
                            .text_color(tokens.faint)
                            .child(t!("workspace.start.quick_connect"))
                            .when_some(quick_connect, |keys, kbd| keys.child(kbd)),
                    ),
            )
    }

    /// 「最近连接」和它的列表（原型 `.start h3`、`.start .list`）；还没连过时是一句说明。
    fn render_recent(
        &self,
        recent: Vec<Host>,
        highlighted: usize,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let list = div()
            .id("start-recent")
            .test_support()
            .flex()
            .flex_col()
            .mb(px(18.))
            .p(px(4.))
            .rounded(CARD_RADIUS)
            .bg(tokens.group)
            .shadow(vec![BoxShadow {
                color: tokens.line,
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: true,
            }]);
        let list = if recent.is_empty() {
            let text = t!("workspace.start.recent_empty");
            list.child(
                div()
                    .id("start-recent-empty")
                    .test_support()
                    .aria_label(text.clone())
                    .py(px(12.))
                    .text_center()
                    .text_size(px(12.))
                    .text_color(tokens.faint)
                    .child(text),
            )
        } else {
            let rows: Vec<AnyElement> = recent
                .iter()
                .enumerate()
                .map(|(row, host)| {
                    self.render_recent_row(host, row == highlighted, tokens, cx)
                        .into_any_element()
                })
                .collect();
            list.children(rows)
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(4.))
                    .pl(px(10.))
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.muted)
                    .child(t!("workspace.start.recent")),
            )
            .child(list)
    }

    /// 最近连接的一行（原型 `.qi`）：徽章、名称和 `user@地址`。
    fn render_recent_row(
        &self,
        host: &Host,
        highlighted: bool,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = host.id;
        let hover = tokens.hover;
        div()
            .id(("start-recent-host", id.0 as usize))
            .test_support()
            .aria_label(host.name.clone())
            .aria_selected(highlighted)
            .flex()
            .items_center()
            .gap(px(9.))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(8.))
            .cursor_pointer()
            .map(|row| {
                if highlighted {
                    row.bg(tokens.selection)
                } else {
                    row.hover(move |style| style.bg(hover))
                }
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                let rows = this.start_card_recent(cx);
                if let Some(row) = rows.iter().position(|host| *host == id) {
                    this.start_highlight = row;
                }
                this.start_card_focus.focus(window, cx);
                cx.notify();
                if event.click_count() == 2 {
                    this.connect_from_start_card(id, window, cx);
                }
            }))
            .child(HostMark::of(host).render(("start-recent-badge", id.0 as usize).into(), BADGE))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .line_height(relative(1.3))
                    .child(
                        div()
                            .text_size(px(13.))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(host.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(tokens.faint)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(list_address(host)),
                    ),
            )
    }
}

/// 一台主机都没有时的欢迎语和一句引导（原型 `.start .hello`）。
fn render_hello(tokens: &Tokens) -> impl IntoElement {
    div()
        .id("start-hello")
        .test_support()
        .aria_label(t!("workspace.start.hint"))
        .flex()
        .flex_col()
        .items_center()
        .mb(px(22.))
        .child(logo(px(48.), px(14.)).mb(px(12.)))
        .child(
            div()
                .text_size(px(18.))
                .font_weight(FontWeight::SEMIBOLD)
                .mb(px(6.))
                .child(t!("workspace.start.title")),
        )
        .child(
            div()
                .text_size(px(12.5))
                .text_color(tokens.muted)
                .child(t!("workspace.start.hint")),
        )
}

/// 开始卡片上的大按钮：彩色图标块、名称和一行说明。
#[allow(clippy::too_many_arguments)]
fn big_button(
    id: &'static str,
    icon: CatalogIcon,
    (icon_bg, icon_fg): (Hsla, Hsla),
    title: SharedString,
    hint: SharedString,
    action: Box<dyn Action>,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
    let target = target.clone();
    let hover = tokens.card_focused;
    div()
        .id(id)
        .test_support()
        .aria_label(title.clone())
        .flex()
        .flex_1()
        .items_center()
        .gap(px(12.))
        .h(px(64.))
        .px(px(16.))
        .rounded(CARD_RADIUS)
        .bg(tokens.card_focused)
        .border_1()
        .border_color(tokens.card_focused_border)
        .shadow(vec![BoxShadow {
            color: tokens.shadow_focused,
            offset: point(px(0.), px(1.)),
            blur_radius: px(2.),
            spread_radius: px(0.),
            inset: false,
        }])
        .cursor_pointer()
        .hover(move |style| style.bg(hover.opacity(0.92)))
        .on_click(move |_, window, cx| target.dispatch_action(action.as_ref(), window, cx))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(32.))
                .rounded(px(9.))
                .bg(icon_bg)
                .text_color(icon_fg)
                .child(Icon::new(icon).size(px(18.))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .child(title)
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(tokens.faint)
                        .child(hint),
                ),
        )
}
