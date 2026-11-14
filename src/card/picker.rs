//! 刚分出来的新卡片（2026-11-14 分屏重做，26 号「分屏 › 选择」）：一个小号的新标签页，选了才开。
//!
//! - 卡片上按「向右分屏」「向下分屏」（顶行按钮、Ctrl+Shift+D / Ctrl+Shift+E），这张卡片让出一半，
//!   新卡片立刻出现在要去的位置，焦点在它的搜索框里；
//! - 顶上搜索框，下面第一项「再开一个 web-01」默认高亮（直接回车 = 复制当前卡片：远程复用同一条连接开
//!   新 shell 通道，本地在同一个目录开新终端），第二项本地终端；细线下面是全部主机，右边淡字 `用户@地址`；
//! - 打字只按主机过滤（名称、地址、用户名，和主机栏搜索同一套），↑↓ 移动，回车或点一行打开；
//! - Esc、顶行的 ×、或者焦点去了别处（点了别的卡片、切了标签）= 撤销，新卡片收回；
//! - 和卡片一样有顶行（「新卡片」，正中淡字「选择要打开的终端」，右边 ×）和状态栏（右边「Esc 取消」）。
//!
//! 它只是个占位：选好以后由窗口壳撤掉它，再把真卡片分到同一个位置。

use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use micaterm_hosts::{Host, HostId, host_matches};
use std::cell::Cell;
use std::rc::Rc;

use super::CardId;
use super::fit::{centered_fits, measure_width, text_width};
use super::title_row::{ICON_GAP, ROW_ICON, ROW_LEFT, TITLE_ROW, row_icon};
use crate::app::CatalogIcon;
use crate::host::{HostMark, HostStore};
use crate::i18n::t;
use crate::split::Axis;
use crate::theme::{MONO_FONT_FAMILY, Tokens, TypeScale, size};

/// 一列最宽多少（卡片窄时跟着变窄）。
const COLUMN: Pixels = px(360.);
/// 一行的高（行和行之间再空 2）。用户：字号小一档、间距收紧（2026-11-14）。
const ROW: Pixels = px(30.);
/// 行前面那一块（徽章、终端图标）的边长。
const LEAD: Pixels = px(14.);

/// 占位新卡片的 id 从这里往上数，和真卡片（从 1 起）错开：真卡片的 id 不因为开过新卡片而跳号。
const PICKER_IDS: u64 = 1 << 40;

/// 下一张占位新卡片的 id。
pub fn next_picker_id() -> CardId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(PICKER_IDS);
    CardId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// 新卡片里选了什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerChoice {
    /// 再开一个：复制分屏的那张卡片。
    Duplicate,
    /// 本地终端（设置里的 shell，主目录）。
    Local,
    /// 连这台主机。
    Host(HostId),
}

/// 新卡片告诉窗口壳的事。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerEvent {
    Chosen(PickerChoice),
    /// 撤销了。`refocus`：按了 Esc，焦点该回到分屏的那张卡片；焦点自己去了别处时是 `false`，不抢回来。
    Dismissed {
        refocus: bool,
    },
}

/// 分屏的是哪张卡片：「再开一个」那一行写它。
#[derive(Clone, Debug)]
pub struct PickerSource {
    /// 卡片的名称：主机名，或者 shell 名。
    pub name: SharedString,
    /// 远程卡片的徽章；本地卡片是 `None`（画终端图标）。
    pub mark: Option<HostMark>,
}

pub struct CardPicker {
    id: CardId,
    axis: Axis,
    source: PickerSource,
    hosts: Entity<HostStore>,
    search: Entity<InputState>,
    query: String,
    highlighted: usize,
    scroll: ScrollHandle,
    focus_handle: FocusHandle,
    /// 上一帧量到的宽度：顶行正中的提示放不放得下看它。
    width: Rc<Cell<f32>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PickerEvent> for CardPicker {}

impl CardPicker {
    pub fn new(
        id: CardId,
        axis: Axis,
        source: PickerSource,
        hosts: Entity<HostStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(t!("card.picker.search")));
        let focus_handle = cx.focus_handle();
        let subscriptions = vec![
            cx.subscribe_in(&search, window, |this, state, event, _, cx| match event {
                InputEvent::Change => {
                    this.query = state.read(cx).value().to_string();
                    this.highlighted = 0;
                    this.scroll.scroll_to_item(0);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.choose(this.highlighted, cx),
                _ => {}
            }),
            cx.observe(&hosts, |_, _, cx| cx.notify()),
            // 焦点去了别处（点了别的卡片、切了标签）：撤销，不抢焦点。
            cx.on_focus_out(&focus_handle, window, |_, _, _, cx| {
                cx.emit(PickerEvent::Dismissed { refocus: false })
            }),
        ];
        Self {
            id,
            axis,
            source,
            hosts,
            search,
            query: String::new(),
            highlighted: 0,
            scroll: ScrollHandle::new(),
            focus_handle,
            width: Rc::new(Cell::new(0.)),
            _subscriptions: subscriptions,
        }
    }

    pub fn id(&self) -> CardId {
        self.id
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }

    /// 焦点给搜索框（刚分出来、或者又按了一次分屏）。
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
    }

    /// 列出来的每一项，从上到下：没在搜索时先是「再开一个」「本地终端」，后面是全部主机；
    /// 搜索时只有匹配的主机。
    pub fn items(&self, cx: &App) -> Vec<PickerChoice> {
        let searching = !self.query.trim().is_empty();
        let fixed = (!searching).then_some([PickerChoice::Duplicate, PickerChoice::Local]);
        fixed
            .into_iter()
            .flatten()
            .chain(
                self.hosts(cx)
                    .iter()
                    .map(|host| PickerChoice::Host(host.id)),
            )
            .collect()
    }

    fn hosts(&self, cx: &App) -> Vec<Host> {
        self.hosts
            .read(cx)
            .hosts()
            .iter()
            .filter(|host| host_matches(host, &self.query))
            .cloned()
            .collect()
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(choice) = self.items(cx).get(index).copied() {
            cx.emit(PickerEvent::Chosen(choice));
        }
    }

    fn move_highlight(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self.items(cx).len();
        if count == 0 {
            return;
        }
        let next = (self.highlighted as isize + step).clamp(0, count as isize - 1) as usize;
        if next != self.highlighted {
            self.highlighted = next;
            self.scroll.scroll_to_item(next);
            cx.notify();
        }
    }

    /// 搜索框（和主机栏的同一个样子，边深一档：焦点总在它里面）。
    fn render_search(&self, tokens: &Tokens, scale: TypeScale) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(size::INPUT)
            .mb(px(8.))
            .px(px(10.))
            .rounded(size::RADIUS)
            .bg(tokens.input)
            .border_1()
            .border_color(tokens.line_strong)
            .text_size(scale.ui)
            .text_color(tokens.st_dim)
            .child(Icon::new(IconName::Search).size(px(14.)))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.search)
                        .id("card-picker-search")
                        .aria_label(t!("card.picker.search"))
                        .appearance(false)
                        .small()
                        .px_0()
                        .text_size(scale.ui),
                ),
            )
    }

    fn render_row(
        &self,
        index: usize,
        choice: PickerChoice,
        tokens: &Tokens,
        scale: TypeScale,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let highlighted = index == self.highlighted;
        let hover = tokens.hover;
        let (lead, name, sub): (AnyElement, SharedString, Option<SharedString>) = match choice {
            PickerChoice::Duplicate => (
                match &self.source.mark {
                    Some(mark) => mark
                        .render(("card-picker-badge", index).into(), LEAD, dark)
                        .into_any_element(),
                    None => terminal_icon(tokens),
                },
                t!("card.picker.duplicate", name = self.source.name),
                Some(if self.source.mark.is_some() {
                    t!("card.picker.reuse_connection")
                } else {
                    t!("card.picker.same_directory")
                }),
            ),
            PickerChoice::Local => (terminal_icon(tokens), t!("card.local.title"), None),
            PickerChoice::Host(id) => {
                let Some(host) = self.hosts.read(cx).host(id).cloned() else {
                    return div().into_any_element();
                };
                (
                    HostMark::of(&host)
                        .render(("card-picker-badge", index).into(), LEAD, dark)
                        .into_any_element(),
                    host.name.clone(),
                    Some(format!("{}@{}", host.user, host.address).into()),
                )
            }
        };
        div()
            .id(("card-picker-item", index))
            .test_support()
            .aria_label(name.clone())
            .aria_selected(highlighted)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .h(ROW)
            .px(px(10.))
            .rounded(size::RADIUS)
            .text_size(scale.ui)
            .text_color(tokens.label)
            .whitespace_nowrap()
            .cursor_pointer()
            .map(|row| {
                if highlighted {
                    row.bg(tokens.sel).text_color(tokens.text)
                } else {
                    row.hover(move |style| style.bg(hover))
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(LEAD)
                    .child(lead),
            )
            .child(div().flex_none().child(name))
            .when_some(sub, |row, sub| {
                row.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(scale.small)
                        .text_color(tokens.st_dim)
                        .child(sub),
                )
            })
            .when(highlighted, |row| {
                row.child(
                    div()
                        .ml_auto()
                        .pl(px(12.))
                        .flex_none()
                        .font_family(MONO_FONT_FAMILY)
                        .text_size(scale.mono_small)
                        .text_color(tokens.st_dim)
                        .child("Enter"),
                )
            })
            .into_any_element()
    }

    /// 和卡片同样的顶行：左边「新卡片」，正中淡字「选择要打开的终端」（放不下就藏），右边 ×。
    fn render_title(
        &self,
        window: &Window,
        tokens: &Tokens,
        scale: TypeScale,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let icon = match self.axis {
            Axis::Horizontal => CatalogIcon::Columns2,
            Axis::Vertical => CatalogIcon::Rows2,
        };
        let title = t!("card.picker.title");
        let hint = t!("card.picker.hint");
        let left = ROW_LEFT + ROW_ICON + ICON_GAP + text_width(window, &title, scale.ui);
        let right = 6. + 22.;
        let hint_fits = centered_fits(
            self.width.get(),
            left,
            right,
            text_width(window, &hint, scale.ui),
        );
        let hover = tokens.hover;
        let text = tokens.text;
        div()
            .id(self.id.element("card-picker-title"))
            .test_support()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .h(TITLE_ROW)
            .pl(px(ROW_LEFT))
            .pr(px(6.))
            .text_size(scale.ui)
            .text_color(tokens.text2)
            .whitespace_nowrap()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(px(ICON_GAP))
                    .child(row_icon(icon, tokens.icon))
                    .child(div().min_w_0().truncate().child(title)),
            )
            .when(hint_fits, |row| {
                row.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .justify_center()
                        .items_center()
                        .text_color(tokens.st_dim)
                        .child(hint),
                )
            })
            .child(
                div()
                    .id("card-picker-close")
                    .test_support()
                    .aria_label(t!("card.picker.cancel"))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(22.))
                    .rounded(px(5.))
                    .text_color(tokens.icon)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover).text_color(text))
                    .on_click(
                        cx.listener(|_, _, _, cx| {
                            cx.emit(PickerEvent::Dismissed { refocus: true })
                        }),
                    )
                    .child(Icon::new(CatalogIcon::Close).size(px(14.))),
            )
    }

    /// 和卡片同样的状态栏：右边「Esc 取消」。
    fn render_status(&self, tokens: &Tokens, scale: TypeScale) -> impl IntoElement {
        div()
            .id(self.id.element("card-picker-status"))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(size::STATUS_BAR)
            .pl(px(ROW_LEFT))
            .pr(px(10.))
            .border_t(size::LINE)
            .border_color(tokens.line)
            .bg(tokens.tabbar)
            .text_size(scale.ui)
            .text_color(tokens.text2)
            .whitespace_nowrap()
            .overflow_hidden()
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .font_family(MONO_FONT_FAMILY)
                            .text_size(scale.mono_small)
                            .child("Esc"),
                    )
                    .child(t!("card.picker.cancel")),
            )
    }
}

fn terminal_icon(tokens: &Tokens) -> AnyElement {
    div()
        .text_color(tokens.icon)
        .child(Icon::new(CatalogIcon::SquareTerminal).size(px(13.)))
        .into_any_element()
}

impl Focusable for CardPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CardPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let dark = cx.theme().is_dark();
        let items = self.items(cx);
        let searching = !self.query.trim().is_empty();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (index, choice) in items.iter().copied().enumerate() {
            let row = self.render_row(index, choice, &tokens, scale, dark, cx);
            // 「本地终端」下面一条细线，再是主机。细线和它包在一起：一项一个子元素，滚动按下标找得准。
            rows.push(
                if choice == PickerChoice::Local && items.len() > index + 1 {
                    div()
                        .flex()
                        .flex_col()
                        .flex_none()
                        .child(row)
                        .child(
                            div()
                                .flex_none()
                                .h(size::LINE)
                                .mx(px(10.))
                                .my(px(4.))
                                .bg(tokens.line),
                        )
                        .into_any_element()
                } else {
                    row
                },
            );
        }
        if searching && items.is_empty() {
            rows.push(
                div()
                    .id("card-picker-empty")
                    .test_support()
                    .pt(px(8.))
                    .px(px(8.))
                    .text_size(scale.ui)
                    .text_color(tokens.st_dim)
                    .child(t!("host.list.no_match", query = self.query.trim()))
                    .into_any_element(),
            );
        }
        div()
            .id(self.id.element("card-picker"))
            .test_support()
            .aria_label(t!("card.picker.title"))
            .track_focus(&self.focus_handle)
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(tokens.content)
            // ↑↓、Esc 先到这里：移动高亮、撤销，不交给输入框。
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.move_highlight(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.move_highlight(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                cx.emit(PickerEvent::Dismissed { refocus: true });
                cx.stop_propagation();
            }))
            .child(self.render_title(window, &tokens, scale, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .justify_center()
                    .pt(px(16.))
                    .px(px(20.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w_full()
                            .max_w(COLUMN)
                            .min_h_0()
                            .child(self.render_search(&tokens, scale))
                            .child(
                                div()
                                    .id("card-picker-list")
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.scroll)
                                    .children(rows),
                            ),
                    ),
            )
            .child(self.render_status(&tokens, scale))
            .child(measure_width(self.width.clone(), cx.entity_id()))
    }
}
