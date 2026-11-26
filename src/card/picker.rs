//! 刚分出来的新卡片（分屏 › 选择）：嵌入分屏槽位里的扁平紧凑终端选择器。
//!
//! - 卡片上按「向右分屏」「向下分屏」（顶行按钮、Ctrl+Shift+D / Ctrl+Shift+E），原卡片让出一半，
//!   新卡片出现在目标槽位，焦点直接落在面板顶部的搜索框里；
//! - 居中一块扁平紧凑的选择器面板（宽 320、`RADIUS_LG` 圆角、`tokens.popup` 底 + `1px` 细边、无阴影）：
//!   顶部 28px 搜索栏，未输入时前两项是快捷操作（「再开一个 …」+ 淡字说明、「本地终端」实心图块），
//!   细线下面是全部主机（右侧淡字 `用户@地址`）；
//! - 打字只按主机过滤（名称、地址、用户名，与主机栏搜索同一套），右侧带一键清空按钮；
//!   ↑↓ 移动高亮，回车或单击打开；
//! - Esc、顶行 ×、状态栏「Esc 取消」或焦点移去别处 = 撤销，新卡片收回；
//! - 外框与同级终端分屏保持一致的 22px 顶行与 22px 分段式状态栏。

use gpui_kit::component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use micaterm_hosts::{Host, HostId, host_matches};
use std::cell::Cell;
use std::rc::Rc;

use super::CardId;
use super::fit::{measure_width, text_width};
use super::render_tab_tile;
use super::title_row::{ROW_LEFT, ROW_RIGHT, TITLE_ROW};
use crate::app::CatalogIcon;
use crate::host::{HostMark, HostStore};
use crate::i18n::t;
use crate::programs::shell_program;
use crate::split::Axis;
use crate::theme::{Tokens, TypeScale, WEIGHT_NORMAL, size, space};
use crate::ui::empty::{EmptyPlacement, EmptyState};
use crate::ui::search::{SearchBar, SearchChrome};
use crate::ui::icon_button::TextTooltip;

/// 选择器面板最大宽度（窄分屏下随容器自适应收缩）。
const PANEL_WIDTH: Pixels = px(320.);
/// 列表行高（紧凑 26px，对齐 `TABLE_ROW`）。
const ROW: Pixels = size::TABLE_ROW;
/// 行首彩色图标/系统徽章图块的统一边长（与标签、卡片标题行的图块同尺寸）。
const LEAD: Pixels = size::TAB_BADGE;
/// 状态栏分段左右内边距（对齐 `card::status_bar`）。
const SEGMENT_PAD: f32 = 7.;
/// 顶行右边距（和终端卡片顶行一致，关闭按钮对齐顶栏的窗口关闭圆点）。
const TITLE_RIGHT: f32 = ROW_RIGHT;

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
    /// 撤销了。`refocus`：按了 Esc 或点了取消，焦点该回到分屏的那张卡片；焦点自己去了别处时是 `false`，不抢回来。
    Dismissed {
        refocus: bool,
    },
}

/// 分屏的是哪张卡片：「再开一个」那一行写它。
#[derive(Clone, Debug)]
pub struct PickerSource {
    /// 卡片的名称：主机名，或者 shell 名。
    pub name: SharedString,
    /// 远程卡片的徽章；本地卡片是 `None`（按 shell 名画统一的终端图块）。
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
    /// 上一帧量到的卡片宽度：顶行副标题放不放得下看它。
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

    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| {
            search.set_value("", window, cx);
            search.focus(window, cx);
        });
        self.query.clear();
        self.highlighted = 0;
        self.scroll.scroll_to_item(0);
        cx.notify();
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

    fn axis_icon(&self) -> CatalogIcon {
        match self.axis {
            Axis::Horizontal => CatalogIcon::Columns2,
            Axis::Vertical => CatalogIcon::Rows2,
        }
    }

    /// 面板顶部紧凑一体化搜索栏（28px 高、无内框、底部分隔细线）。
    fn render_search(
        &self,
        tokens: &Tokens,
        scale: TypeScale,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        SearchBar::new(
            "card-picker-search-box",
            &self.search,
            "card-picker-search",
            t!("card.picker.search"),
            "card-picker-clear",
            t!("host.list.clear_search"),
            SearchChrome::Underline,
            tokens,
            scale,
            cx.listener(|this, _, window, cx| this.clear_search(window, cx)),
        )
        .query(!self.query.is_empty())
        .render()
    }

    fn render_row(
        &self,
        index: usize,
        choice: PickerChoice,
        tokens: &Tokens,
        scale: TypeScale,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let highlighted = index == self.highlighted;
        let hover = tokens.hover;
        let text = tokens.text;
        let badge_id: ElementId = ("card-picker-badge", index).into();
        let (lead, name, sub): (AnyElement, SharedString, Option<SharedString>) = match choice {
            PickerChoice::Duplicate => (
                match &self.source.mark {
                    Some(mark) => mark
                        .render(badge_id, LEAD, size::TAB_BADGE_GLYPH)
                        .into_any_element(),
                    None => shell_badge(badge_id, &self.source.name, tokens),
                },
                t!("card.picker.duplicate", name = self.source.name),
                Some(if self.source.mark.is_some() {
                    t!("card.picker.reuse_connection")
                } else {
                    t!("card.picker.same_directory")
                }),
            ),
            PickerChoice::Local => (
                shell_badge(badge_id, t!("card.local.title").as_ref(), tokens),
                t!("card.local.title"),
                None,
            ),
            PickerChoice::Host(id) => {
                let Some(host) = self.hosts.read(cx).host(id).cloned() else {
                    return div().into_any_element();
                };
                (
                    HostMark::of(&host)
                        .render(badge_id, LEAD, size::TAB_BADGE_GLYPH)
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
            .gap(space::SPACE_7)
            .h(ROW)
            .px(space::SPACE_6)
            .rounded(size::RADIUS_SM)
            .text_size(scale.sm)
            .text_color(tokens.label)
            .whitespace_nowrap()
            .cursor_pointer()
            .map(|row| {
                if highlighted {
                    row.bg(tokens.sel).text_color(tokens.text)
                } else {
                    row.hover(move |style| style.bg(hover).text_color(text))
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
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(space::SPACE_6)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_weight(WEIGHT_NORMAL)
                            .child(name),
                    )
                    .when_some(sub, |group, sub| {
                        group.child(
                            div()
                                .flex_shrink(4.)
                                .min_w_0()
                                .truncate()
                                .text_size(scale.xs)
                                .font_weight(WEIGHT_NORMAL)
                                .text_color(tokens.st_dim)
                                .child(sub),
                        )
                    }),
            )
            .when(highlighted, |row| {
                row.child(
                    div()
                        .ml_auto()
                        .pl(space::SPACE_6)
                        .flex_none()
                        .text_size(scale.sm)
                        .font_weight(WEIGHT_NORMAL)
                        .text_color(tokens.st_dim)
                        .child("Enter"),
                )
            })
            .into_any_element()
    }

    /// 与终端卡片一致的扁平 22px 顶行：左侧分屏图标 + 「新卡片」+ 紧随其后的淡色说明（窄时收起），右侧 20×20 关闭按钮。
    fn render_title(
        &self,
        window: &Window,
        tokens: &Tokens,
        scale: TypeScale,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let icon = self.axis_icon();
        let title = t!("card.picker.title");
        let hint = t!("card.picker.hint");
        let width = self.width.get();
        let required = ROW_LEFT
            + f32::from(size::ICON_SM)
            + 6.
            + text_width(window, &title, scale.sm)
            + 12.
            + text_width(window, &hint, scale.sm)
            + TITLE_RIGHT
            + f32::from(size::CONTROL_SM)
            + 12.;
        let show_hint = width <= 0. || width >= required;
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
            .pr(px(TITLE_RIGHT))
            .text_size(scale.sm)
            .font_weight(WEIGHT_NORMAL)
            .text_color(tokens.text)
            .whitespace_nowrap()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(space::SPACE_6)
                    .child(
                        div()
                            .flex_none()
                            .text_color(tokens.icon)
                            .child(Icon::new(icon).size(size::ICON_SM)),
                    )
                    .child(div().min_w_0().truncate().child(title))
                    .when(show_hint, |left| {
                        left.child(div().flex_none().text_color(tokens.faint).child("·"))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(tokens.st_dim)
                                    .child(hint),
                            )
                    }),
            )
            .child(
                div()
                    .id("card-picker-close")
                    .test_support()
                    .aria_label(t!("card.picker.cancel"))
                    .tooltip(|_, cx| TextTooltip::view(t!("card.picker.cancel"), cx))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(size::CONTROL_SM)
                    .rounded(size::RADIUS_SM)
                    .text_color(tokens.icon)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover).text_color(text))
                    .on_click(
                        cx.listener(|_, _, _, cx| {
                            cx.emit(PickerEvent::Dismissed { refocus: true })
                        }),
                    )
                    .child(Icon::new(CatalogIcon::Close).size(size::ICON_SM)),
            )
    }

    /// 与终端卡片一致的 22px 分段式状态栏：左段标示分屏方向，右段「Esc 取消」带贯通竖线且可点击取消。
    fn render_status(
        &self,
        tokens: &Tokens,
        scale: TypeScale,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let split_label = match self.axis {
            Axis::Horizontal => t!("card.split_right"),
            Axis::Vertical => t!("card.split_down"),
        };
        let hover = tokens.hover;
        let text = tokens.text;
        div()
            .id(self.id.element("card-picker-status"))
            .flex()
            .flex_none()
            .items_center()
            .h(size::STATUS_BAR)
            .border_t(size::LINE)
            .border_color(tokens.line)
            .bg(tokens.tabbar)
            .text_size(scale.sm)
            .text_color(tokens.text2)
            .whitespace_nowrap()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_shrink(1.)
                    .min_w_0()
                    .items_center()
                    .h_full()
                    .gap(space::SPACE_5)
                    .px(px(SEGMENT_PAD))
                    .child(
                        div()
                            .flex_none()
                            .text_color(tokens.st_dim)
                            .child(Icon::new(self.axis_icon()).size(px(10.5))),
                    )
                    .child(div().min_w_0().truncate().child(split_label)),
            )
            .child(div().flex_1().min_w_0())
            .child(
                div()
                    .id("card-picker-cancel")
                    .test_support()
                    .aria_label(t!("card.picker.cancel"))
                    .flex()
                    .flex_none()
                    .items_center()
                    .h_full()
                    .gap(space::SPACE_5)
                    .px(px(SEGMENT_PAD))
                    .border_l(size::LINE)
                    .border_color(tokens.line)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover).text_color(text))
                    .on_click(
                        cx.listener(|_, _, _, cx| {
                            cx.emit(PickerEvent::Dismissed { refocus: true })
                        }),
                    )
                    .child(div().text_color(tokens.st_dim).child("Esc"))
                    .child(t!("card.picker.cancel")),
            )
    }
}

/// 本地终端 / 本地复制项的实心圆角图块：和标签、卡片标题行上的同一个方块——终端图标 +
/// 统一的中性底色，不随本机 shell 的品牌变色（`render_tab_tile`，边长 `LEAD`）。
fn shell_badge(id: ElementId, shell_name: &str, tokens: &Tokens) -> AnyElement {
    render_tab_tile(id, &shell_program(shell_name), tokens).into_any_element()
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
        let items = self.items(cx);
        let searching = !self.query.trim().is_empty();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (index, choice) in items.iter().copied().enumerate() {
            let row = self.render_row(index, choice, &tokens, scale, cx);
            // 「本地终端」下面一条紧凑 1px 细线，再接全部主机。细线和它包在一起：保证下标与 `items` 一一对应。
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
                                .mx(space::SPACE_4)
                                .my(px(3.))
                                .bg(tokens.line),
                        )
                        .into_any_element()
                } else {
                    row
                },
            );
        }
        if searching && items.is_empty() {
            let empty_text = t!("host.list.no_match", query = self.query.trim());
            rows.push(
                EmptyState::new("card-picker-empty", empty_text, EmptyPlacement::Picker)
                    .render(&tokens, scale),
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
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .items_center()
                    .pt(px(14.))
                    .pb(space::SPACE_12)
                    .px(space::SPACE_12)
                    .child(
                        div()
                            .id("card-picker-surface")
                            .test_support()
                            .flex()
                            .flex_col()
                            .w_full()
                            .max_w(PANEL_WIDTH)
                            .min_h_0()
                            .rounded(size::RADIUS_LG)
                            .bg(tokens.popup)
                            .border_1()
                            .border_color(tokens.line)
                            .overflow_hidden()
                            .child(self.render_search(&tokens, scale, cx))
                            .child(
                                div()
                                    .id("card-picker-list")
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.))
                                    .p(space::SPACE_4)
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.scroll)
                                    .children(rows),
                            ),
                    ),
            )
            .child(self.render_status(&tokens, scale, cx))
            .child(measure_width(self.width.clone(), cx.entity_id()))
    }
}
