// 参考 since2006/shell-rs @ b2a307a: src/host/quick_connect.rs 重写
// 裁剪：去掉多选（`MULTIPLE`）、分组、临时主机；匹配规则换成主机栏的 `host_matches`；
// 外观按全部界面原型的 `.qc`：「⌄」下面弹出的列表，不是居中的对话框。

//! 快速连接：标签轨道「+」旁边的「⌄」（Ctrl+Shift+O）弹出的主机搜索列表。
//!
//! - 最近连过的主机排在前面（最新的在前）并标「最近」，其余按名称排；
//! - 输入就筛选（和主机栏一样：名称、地址、用户名，几个词都要匹配），总是高亮第一个；
//! - ↑↓ 移动高亮，回车在新标签里连接高亮的那台，Esc 关闭；点一行也连接。
//!
//! 这里只管列表和按键，连接、关闭由窗口壳处理（[`QuickConnectEvent`]）。

use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use qishell_hosts::{Host, HostData, HostId, host_matches};

use super::list::list_address;
use super::mark::HostMark;
use super::store::HostStore;
use crate::i18n::t;
use crate::theme::Tokens;

/// 弹层宽度（原型 `.qc`）。
pub const QUICK_CONNECT_WIDTH: Pixels = px(330.);
/// 徽章直径（`.qi .badge`）。
const BADGE: Pixels = px(22.);
/// 列表最多这么高，再多就滚动（约 8 行）。
const LIST_MAX_HEIGHT: Pixels = px(316.);

/// 列表里的一台主机：打开时从主机数据里取的样子，和它是不是最近连过的。
#[derive(Clone, Debug)]
pub struct QuickHost {
    pub host: Host,
    pub recent: bool,
}

/// 快速连接列出的主机：最近连过的在前（最新的在前），其余按名称（不分大小写）。
pub fn quick_hosts(data: &HostData) -> Vec<QuickHost> {
    let mut hosts: Vec<QuickHost> = data
        .recent_hosts()
        .map(|host| QuickHost {
            host: host.clone(),
            recent: true,
        })
        .collect();
    let mut rest: Vec<QuickHost> = data
        .hosts()
        .iter()
        .filter(|host| !hosts.iter().any(|recent| recent.host.id == host.id))
        .map(|host| QuickHost {
            host: host.clone(),
            recent: false,
        })
        .collect();
    rest.sort_by_key(|quick| quick.host.name.to_lowercase());
    hosts.extend(rest);
    hosts
}

/// 列表显示什么、高亮哪一行：快速连接的纯状态。
#[derive(Clone, Debug, Default)]
pub struct QuickChoice {
    hosts: Vec<QuickHost>,
    query: String,
    /// `hosts` 里匹配搜索词的下标，按顺序。
    shown: Vec<usize>,
    /// 高亮的是 `shown` 的第几行。
    highlighted: usize,
}

impl QuickChoice {
    pub fn new(hosts: Vec<QuickHost>) -> Self {
        let mut choice = Self {
            hosts,
            ..Self::default()
        };
        choice.search("");
        choice
    }

    /// 主机数据变了：换上新的主机，搜索词不变；高亮回到第一个。
    pub fn set_hosts(&mut self, hosts: Vec<QuickHost>) {
        self.hosts = hosts;
        let query = std::mem::take(&mut self.query);
        self.search(&query);
    }

    /// 只显示 `query` 匹配的主机，高亮第一个。
    pub fn search(&mut self, query: &str) {
        self.query = query.to_string();
        self.shown = (0..self.hosts.len())
            .filter(|ix| host_matches(&self.hosts[*ix].host, query))
            .collect();
        self.highlighted = 0;
    }

    /// 高亮往下（`step` 为正）或往上移，停在两头。
    pub fn move_highlight(&mut self, step: isize) {
        if self.shown.is_empty() {
            return;
        }
        let last = self.shown.len() as isize - 1;
        self.highlighted = (self.highlighted as isize + step).clamp(0, last) as usize;
    }

    /// 高亮第 `row` 行（鼠标移上去）。
    pub fn highlight(&mut self, row: usize) {
        if row < self.shown.len() {
            self.highlighted = row;
        }
    }

    /// 显示的主机，从上到下。
    pub fn shown(&self) -> impl Iterator<Item = &QuickHost> {
        self.shown.iter().map(|ix| &self.hosts[*ix])
    }

    /// 高亮的是第几行；没有匹配时是 `None`。
    pub fn highlighted_row(&self) -> Option<usize> {
        (!self.shown.is_empty()).then_some(self.highlighted)
    }

    /// 回车连接的那台：高亮的主机。
    pub fn highlighted(&self) -> Option<HostId> {
        self.shown
            .get(self.highlighted)
            .map(|ix| self.hosts[*ix].host.id)
    }

    /// 一台主机都没有（不是没有匹配）。
    pub fn has_hosts(&self) -> bool {
        !self.hosts.is_empty()
    }

    pub fn query(&self) -> &str {
        &self.query
    }
}

/// 快速连接要窗口壳做的事。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuickConnectEvent {
    /// 回车或点了一行：关掉弹层，在新标签里连接这台主机。
    Connect(HostId),
    /// Esc：关掉弹层，焦点回到打开前的地方。
    Close,
    /// 在弹层外面按下了鼠标（窗口坐标）：窗口壳决定关不关（按在「⌄」上由它自己开关）。
    ClickedOutside(Point<Pixels>),
}

/// 快速连接弹层：搜索框、主机列表和按键提示。窗口壳打开时建一个，关掉时放掉。
pub struct QuickConnectList {
    search: Entity<InputState>,
    choice: QuickChoice,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<QuickConnectEvent> for QuickConnectList {}

impl QuickConnectList {
    pub fn new(store: Entity<HostStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("quick_connect.search")));
        let subscriptions = vec![
            cx.subscribe_in(
                &search,
                window,
                |this, state, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => {
                        let query = state.read(cx).value().to_string();
                        this.choice.search(&query);
                        this.scroll.scroll_to_item(0);
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => {
                        if let Some(id) = this.choice.highlighted() {
                            cx.emit(QuickConnectEvent::Connect(id));
                        }
                    }
                    _ => {}
                },
            ),
            // 开着时主机数据变了（比如探测出了系统）：列表跟着变，搜索词不变。
            cx.observe(&store, |this, store, cx| {
                this.choice.set_hosts(quick_hosts(store.read(cx)));
                cx.notify();
            }),
        ];
        let choice = QuickChoice::new(quick_hosts(store.read(cx)));
        Self {
            search,
            choice,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// 列表的状态（显示哪些主机、高亮哪个）。
    pub fn choice(&self) -> &QuickChoice {
        &self.choice
    }

    /// 焦点给搜索框：打开就能打字。
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
    }

    fn move_highlight(&mut self, step: isize, cx: &mut Context<Self>) {
        self.choice.move_highlight(step);
        if let Some(row) = self.choice.highlighted_row() {
            self.scroll.scroll_to_item(row);
        }
        cx.notify();
    }

    fn render_search(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("quick-connect-search-box")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .mb(px(6.))
            .h(px(30.))
            .px(px(8.))
            .rounded(px(8.))
            .text_size(px(12.5))
            .text_color(tokens.faint)
            .bg(tokens.field_focused)
            .shadow(vec![BoxShadow {
                color: tokens.accent,
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.5),
                inset: true,
            }])
            // 搜索框里的 ↑↓、Esc 先到这里：移动高亮、关闭，不交给输入框。
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.move_highlight(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.move_highlight(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                cx.emit(QuickConnectEvent::Close);
                cx.stop_propagation();
            }))
            .child(Icon::new(IconName::Search).size(px(14.)))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.search)
                        .id("quick-connect-search")
                        .aria_label(t!("quick_connect.search"))
                        .appearance(false)
                        .small(),
                ),
            )
            .child(render_kbd(shortcut(), tokens))
    }

    fn render_row(
        &self,
        quick: &QuickHost,
        highlighted: bool,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let host = &quick.host;
        let id = host.id;
        let hover = tokens.hover;
        let address = list_address(host);
        div()
            .id(("quick-connect-host", id.0 as usize))
            .test_support()
            .aria_label(host.name.clone())
            .aria_selected(highlighted)
            .flex()
            .flex_none()
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
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(QuickConnectEvent::Connect(id));
            }))
            .child(HostMark::of(host).render(("quick-connect-badge", id.0 as usize).into(), BADGE))
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
                            .child(address),
                    ),
            )
            .when(quick.recent, |row| {
                let text = t!("quick_connect.recent");
                row.child(
                    div()
                        .id(("quick-connect-recent", id.0 as usize))
                        .test_support()
                        .aria_label(text.clone())
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(tokens.faint)
                        .child(text),
                )
            })
    }
}

impl Focusable for QuickConnectList {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl Render for QuickConnectList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let highlighted = self.choice.highlighted_row();
        let rows: Vec<AnyElement> = self
            .choice
            .shown()
            .cloned()
            .collect::<Vec<_>>()
            .iter()
            .enumerate()
            .map(|(row, quick)| {
                self.render_row(quick, highlighted == Some(row), &tokens, cx)
                    .into_any_element()
            })
            .collect();
        let list = if rows.is_empty() {
            let text = if self.choice.has_hosts() {
                t!("quick_connect.no_match", query = self.choice.query().trim())
            } else {
                t!("quick_connect.no_hosts")
            };
            div()
                .id("quick-connect-empty")
                .test_support()
                .aria_label(text.clone())
                .py(px(14.))
                .text_center()
                .text_size(px(12.5))
                .text_color(tokens.faint)
                .child(text)
                .into_any_element()
        } else {
            div()
                .id("quick-connect-rows")
                .flex()
                .flex_col()
                .max_h(LIST_MAX_HEIGHT)
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .children(rows)
                .into_any_element()
        };
        div()
            .id("quick-connect-popup")
            .test_support()
            .aria_label(t!("workspace.title_bar.quick_connect"))
            .occlude()
            .flex()
            .flex_col()
            .w(QUICK_CONNECT_WIDTH)
            .p(px(6.))
            .rounded(px(12.))
            .bg(tokens.panel)
            .text_color(tokens.text)
            .shadow(vec![
                BoxShadow {
                    color: tokens.panel_border,
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.22),
                    offset: point(px(0.), px(14.)),
                    blur_radius: px(40.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
            .on_mouse_down_out(cx.listener(|_, event: &MouseDownEvent, _, cx| {
                cx.emit(QuickConnectEvent::ClickedOutside(event.position));
            }))
            .child(self.render_search(&tokens, cx))
            .child(list)
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .mt(px(4.))
                    .px(px(8.))
                    .pt(px(6.))
                    .pb(px(2.))
                    .border_t_1()
                    .border_color(tokens.line)
                    .text_size(px(11.))
                    .text_color(tokens.faint)
                    .child(t!("quick_connect.keys.move"))
                    .child(t!("quick_connect.keys.connect"))
                    .child(t!("quick_connect.keys.close")),
            )
    }
}

/// 按键提示（原型 `.kbd`）。
fn render_kbd(keys: &'static str, tokens: &Tokens) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(5.))
        .bg(tokens.field)
        .border_1()
        .border_color(tokens.field_border)
        .text_size(px(11.))
        .text_color(tokens.muted)
        .whitespace_nowrap()
        .child(keys)
}

/// 搜索框里提示的快捷键：快速连接（spec 第 8 节）。
fn shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "\u{21e7}\u{2318}O"
    } else {
        "Ctrl+Shift+O"
    }
}

#[cfg(test)]
mod tests {
    use qishell_hosts::{AuthKind, HostData, HostDraft, HostId};

    use super::{QuickChoice, quick_hosts};

    fn ids(choice: &QuickChoice) -> Vec<u64> {
        choice.shown().map(|quick| quick.host.id.0).collect()
    }

    #[test]
    fn hosts_connected_lately_come_first_newest_first_then_the_rest_by_name() {
        // seed：web-01 1、web-02 2、db-01 3、staging-api 4、qa-runner 5、dev-box 6；
        // 先连 web-01，后连 staging-api。
        let mut data = HostData::seed();
        let alpha = data.insert_unnotified(HostDraft::new(
            "Alpha",
            "10.9.9.9",
            22,
            "root",
            AuthKind::Password,
        ));
        let hosts = quick_hosts(&data);
        let order: Vec<(u64, bool)> = hosts
            .iter()
            .map(|quick| (quick.host.id.0, quick.recent))
            .collect();
        assert_eq!(
            order,
            [
                (4, true),
                (1, true),
                (alpha.0, false),
                (3, false),
                (6, false),
                (5, false),
                (2, false),
            ]
        );
    }

    #[test]
    fn the_first_host_is_highlighted_and_typing_filters_like_the_host_sidebar() {
        let mut choice = QuickChoice::new(quick_hosts(&HostData::seed()));
        assert_eq!(choice.highlighted(), Some(HostId(4)));

        // 地址、用户名、名称都查，几个词都要匹配；备注不查。
        choice.search("10.0.1");
        assert_eq!(ids(&choice), [1, 2]);
        assert_eq!(choice.highlighted(), Some(HostId(1)));
        choice.search("ROOT web 02");
        assert_eq!(ids(&choice), [2]);
        choice.search("postgres");
        assert_eq!(ids(&choice), [3]);
    }

    #[test]
    fn arrows_move_the_highlight_and_stop_at_either_end() {
        let mut choice = QuickChoice::new(quick_hosts(&HostData::seed()));
        choice.search("web");
        choice.move_highlight(-1);
        assert_eq!(choice.highlighted(), Some(HostId(1)));
        choice.move_highlight(1);
        assert_eq!(choice.highlighted(), Some(HostId(2)));
        choice.move_highlight(1);
        assert_eq!(choice.highlighted(), Some(HostId(2)));
        // 换个搜索词，高亮回到第一个。
        choice.search("we");
        assert_eq!(choice.highlighted_row(), Some(0));
    }

    #[test]
    fn nothing_matching_highlights_nothing() {
        let mut choice = QuickChoice::new(quick_hosts(&HostData::seed()));
        choice.search("nope");
        assert_eq!(choice.highlighted(), None);
        assert_eq!(choice.highlighted_row(), None);
        choice.move_highlight(1);
        assert_eq!(choice.highlighted(), None);
        assert!(choice.has_hosts());
        assert!(!QuickChoice::new(Vec::new()).has_hosts());
    }
}
