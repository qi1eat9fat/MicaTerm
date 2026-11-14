//! 表格（26 号 SFTP `.sf-hd` / `.sf-r`，照 Zed `ui/components/data_table.rs`）：SFTP 文件列表和
//! 传输队列共用这一种，没有别的表格实现。
//!
//! - 表头一行高 28、下边一条 `--line`、`--st-dim` 字；排序的那一列是 `--text2`，后面一个 10 的小箭头。
//! - 行高 26、没有行线；悬停一层 `--hover`，选中 `--sel`（列表没有焦点时选中退成 `--hover`）。
//! - 单元格左右留 8，每行第一格左边 12；列宽是一列占剩下的（`Fill`）加几列定宽，表头和行用同一套，
//!   所以上下对齐。放不下的文字截断成省略号。
//! - 行在 `uniform_list` 里（只画看得见的），右边一条滚动条，从表头下面开始。

use std::ops::Range;

use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{Tokens, size};

/// 表头的高（`.sf-hd`）。
pub const HEADER_HEIGHT: Pixels = px(28.);
/// 一行的高（`.sf-r`）。
pub const ROW_HEIGHT: Pixels = px(26.);
/// 单元格左右的边距；每行第一格左边是 12。
const CELL_PADDING: Pixels = px(8.);
const FIRST_CELL_PADDING: Pixels = px(12.);
/// 行列表上面留 2（`.sf-list` 的 padding-top）。
const LIST_TOP: Pixels = px(2.);
/// 表头排序箭头的边长和它离标题的距离。
const SORT_ICON: Pixels = px(10.);

/// 一列多宽。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnWidth {
    /// 占剩下的宽度（一张表只有一列这样）。
    Fill,
    Fixed(Pixels),
}

/// 格子里的东西靠哪边。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// 一列：键（元素 id `column:键`）、标题、宽度、对齐。
#[derive(Clone, Debug)]
pub struct TableColumn {
    pub key: &'static str,
    pub title: SharedString,
    pub width: ColumnWidth,
    /// 行里格子的对齐。
    pub align: Align,
    /// 表头的对齐，默认和格子一样。
    pub header_align: Align,
    /// 格子不留左右边距：里面的东西自己摆（队列的折叠箭头）。
    pub bare: bool,
}

impl TableColumn {
    pub fn new(key: &'static str, title: impl Into<SharedString>, width: ColumnWidth) -> Self {
        Self {
            key,
            title: title.into(),
            width,
            align: Align::Left,
            header_align: Align::Left,
            bare: false,
        }
    }

    /// 格子和表头都靠右（大小、数字）。
    pub fn right(mut self) -> Self {
        self.align = Align::Right;
        self.header_align = Align::Right;
        self
    }

    /// 表头另外的对齐。
    pub fn header(mut self, align: Align) -> Self {
        self.header_align = align;
        self
    }

    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }
}

/// 第 `index` 列的格子：宽、边距、对齐，里面一行放不下就截断。
fn sized(column: &TableColumn, index: usize, align: Align) -> Div {
    let cell = div()
        .flex()
        .h_full()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap();
    let cell = match column.width {
        ColumnWidth::Fill => cell.flex_1(),
        ColumnWidth::Fixed(width) => cell.flex_none().w(width),
    };
    let cell = if column.bare {
        cell
    } else {
        cell.pl(if index == 0 {
            FIRST_CELL_PADDING
        } else {
            CELL_PADDING
        })
        .pr(CELL_PADDING)
    };
    match align {
        Align::Left => cell,
        Align::Center => cell.justify_center(),
        Align::Right => cell.justify_end(),
    }
}

/// 行里第 `index` 列的格子。
pub fn cell(columns: &[TableColumn], index: usize) -> Div {
    let column = &columns[index];
    sized(column, index, column.align)
}

/// 行里第 `index` 列的格子，放一段会截断的文字。
pub fn text_cell(columns: &[TableColumn], index: usize, text: impl Into<SharedString>) -> Div {
    cell(columns, index).child(div().min_w_0().truncate().child(text.into()))
}

/// 排序的方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// 表头第 `index` 列（元素 id `column:键`）；`sorted` 是这一列按什么排（不是排序的那一列就是 `None`）。
/// 点击由调用方加，调用方最后加 `.test_support()`。
pub fn header_cell(
    columns: &[TableColumn],
    index: usize,
    sorted: Option<SortDirection>,
    tokens: &Tokens,
) -> Stateful<Div> {
    let column = &columns[index];
    sized(column, index, column.header_align)
        .id(ElementId::Name(format!("column:{}", column.key).into()))
        .aria_label(column.title.clone())
        .gap(px(3.))
        .text_color(if sorted.is_some() {
            tokens.text2
        } else {
            tokens.st_dim
        })
        .child(div().min_w_0().truncate().child(column.title.clone()))
        .when_some(sorted, |cell, sorted| {
            cell.child(
                Icon::new(match sorted {
                    SortDirection::Ascending => IconName::ChevronUp,
                    SortDirection::Descending => IconName::ChevronDown,
                })
                .size(SORT_ICON)
                .flex_none(),
            )
        })
}

/// 表头那一行，放 `header_cell`。
pub fn header_row(tokens: &Tokens) -> Div {
    div()
        .flex()
        .flex_none()
        .w_full()
        .h(HEADER_HEIGHT)
        .border_b(size::LINE)
        .border_color(tokens.line)
}

/// 一行怎么显示。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowState {
    pub selected: bool,
    /// 表格有焦点：选中才是 `--sel`，不然退成 `--hover`。
    pub focused: bool,
}

/// 一行（26 高、悬停有底色、选中 `--sel`），放 `cell` / `text_cell`。调用方最后加 `.test_support()`。
pub fn row(id: impl Into<ElementId>, state: RowState, tokens: &Tokens) -> Stateful<Div> {
    let hover = tokens.hover;
    div()
        .id(id)
        .flex()
        .flex_none()
        .w_full()
        .h(ROW_HEIGHT)
        .text_color(tokens.label)
        .map(|row| match state {
            RowState {
                selected: true,
                focused: true,
            } => row.bg(tokens.sel).text_color(tokens.text),
            RowState {
                selected: true,
                focused: false,
            } => row.bg(hover),
            _ => row.hover(move |style| style.bg(hover)),
        })
}

/// 表格的行列表：只画看得见的行，`render` 给出 `range` 那几行。
pub fn body<V: 'static>(
    id: impl Into<ElementId>,
    count: usize,
    scroll: &UniformListScrollHandle,
    cx: &mut Context<V>,
    render: impl Fn(&mut V, Range<usize>, &mut Window, &mut Context<V>) -> Vec<AnyElement> + 'static,
) -> impl IntoElement {
    uniform_list(id, count, cx.processor(render))
        .track_scroll(scroll)
        .flex_1()
        .min_h_0()
        .pt(LIST_TOP)
}

/// 行列表的滚动条：贴着右边，从表头下面开始。放在表格容器（`relative`）里最后。
pub fn scrollbar(scroll: &UniformListScrollHandle) -> impl IntoElement {
    div()
        .absolute()
        .top(HEADER_HEIGHT)
        .right_0()
        .bottom_0()
        .w(Scrollbar::width())
        .child(
            Scrollbar::vertical(scroll)
                .viewport_from_layout()
                .max_fps(60),
        )
}
