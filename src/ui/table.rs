//! Shared virtualized table & list rows (aligned with Muxy's inset rounded list language and
//! `docs/ui-standard.md` §4.9): shared by SFTP file lists and the transfer queue.
//!
//! - Header row: `24px` tall (`size::TABLE_HEADER`), `11px Regular` (`TypeScale::sm`), bottom
//!   `1px` hairline (`tokens.line`), `4px` horizontal inset matching body rows; active sorted
//!   column uses `tokens.text` with a `10px` brand chevron.
//! - Body rows: `26px` tall (`size::TABLE_ROW`), `4px` horizontal list inset, `6px` (`RADIUS_MD`)
//!   rounded hover/selection fill (`tokens.hover` / `tokens.sel`, falling back to `tokens.tab_active`
//!   when the pane is unfocused), zero row divider lines.
//! - Virtualized via `uniform_list` with an overlay vertical `Scrollbar`.

use std::ops::Range;

use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{Tokens, TypeScale, WEIGHT_NORMAL, size};

/// Table header height (`24px`, same as the tab bar and pane header).
pub const HEADER_HEIGHT: Pixels = size::TABLE_HEADER;
/// Uniform row height (`26px`).
pub const ROW_HEIGHT: Pixels = size::TABLE_ROW;
/// Horizontal inset around rounded rows and matching header alignment (`4px`, Muxy `mx-1`).
pub const ROW_INSET: Pixels = px(4.);
/// Horizontal cell padding; first cell gets `8px` inside the `4px` inset (`12px` from pane edge).
const CELL_PADDING: Pixels = px(8.);
const FIRST_CELL_PADDING: Pixels = px(8.);
/// Top breathing room above the first row (`3px`) and bottom padding (`4px`).
const LIST_TOP: Pixels = px(3.);
const LIST_BOTTOM: Pixels = px(4.);
/// Sort indicator chevron size.
const SORT_ICON: Pixels = px(10.);

/// Column width policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnWidth {
    /// Expands to take remaining width (one `Fill` column per table).
    Fill,
    Fixed(Pixels),
}

/// Horizontal cell alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Column definition: key (`column:<key>`), title, width, and alignment.
#[derive(Clone, Debug)]
pub struct TableColumn {
    pub key: &'static str,
    pub title: SharedString,
    pub width: ColumnWidth,
    /// Cell alignment inside data rows.
    pub align: Align,
    /// Header cell alignment (defaults to `align`).
    pub header_align: Align,
    /// Omit default cell horizontal padding when the caller positions its own control.
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

    /// Right-align both data cells and header (for numeric columns like Size).
    pub fn right(mut self) -> Self {
        self.align = Align::Right;
        self.header_align = Align::Right;
        self
    }

    /// Override header alignment independently.
    pub fn header(mut self, align: Align) -> Self {
        self.header_align = align;
        self
    }

    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }
}

/// Sized cell box for column `index`.
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

/// Data cell for column `index`.
pub fn cell(columns: &[TableColumn], index: usize) -> Div {
    let column = &columns[index];
    sized(column, index, column.align)
}

/// Data cell containing a single truncated text string.
pub fn text_cell(columns: &[TableColumn], index: usize, text: impl Into<SharedString>) -> Div {
    cell(columns, index).child(div().min_w_0().truncate().child(text.into()))
}

/// Sort direction for a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// Header cell for column `index` (`id = "column:<key>"`).
/// Caller attaches click handler if sortable and finishes with `.test_support()`.
pub fn header_cell(
    columns: &[TableColumn],
    index: usize,
    sorted: Option<SortDirection>,
    tokens: &Tokens,
) -> Stateful<Div> {
    let column = &columns[index];
    let hover_color = tokens.text2;
    sized(column, index, column.header_align)
        .id(ElementId::Name(format!("column:{}", column.key).into()))
        .aria_label(column.title.clone())
        .gap(px(4.))
        .text_color(if sorted.is_some() {
            tokens.text
        } else {
            tokens.st_dim
        })
        .when(!column.title.is_empty(), |cell| {
            cell.cursor_pointer()
                .hover(move |style| style.text_color(hover_color))
        })
        .child(div().min_w_0().truncate().child(column.title.clone()))
        .when_some(sorted, |cell, sorted| {
            cell.child(
                Icon::new(match sorted {
                    SortDirection::Ascending => IconName::ChevronUp,
                    SortDirection::Descending => IconName::ChevronDown,
                })
                .size(SORT_ICON)
                .flex_none()
                .text_color(tokens.brand),
            )
        })
}

/// Table header row (`28px` tall, `11px Regular`, `4px` horizontal inset to align with body rows).
pub fn header_row(tokens: &Tokens, scale: TypeScale) -> Div {
    div()
        .flex()
        .flex_none()
        .w_full()
        .h(HEADER_HEIGHT)
        .px(ROW_INSET)
        .border_b(size::LINE)
        .border_color(tokens.line)
        .text_size(scale.sm)
        .font_weight(WEIGHT_NORMAL)
}

/// Visual selection and focus state for a row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowState {
    pub selected: bool,
    /// Whether the owning pane/list currently holds focus (`tokens.sel` vs `tokens.tab_active`).
    pub focused: bool,
}

/// Inset rounded row (`26px` tall, `RADIUS_MD = 6px`). Caller finishes with `.test_support()`.
pub fn row(id: impl Into<ElementId>, state: RowState, tokens: &Tokens) -> Stateful<Div> {
    let hover = tokens.hover;
    div()
        .id(id)
        .flex()
        .flex_none()
        .w_full()
        .h(ROW_HEIGHT)
        .rounded(size::RADIUS_MD)
        .text_color(tokens.label)
        .map(|row| match state {
            RowState {
                selected: true,
                focused: true,
            } => row.bg(tokens.sel).text_color(tokens.text),
            RowState {
                selected: true,
                focused: false,
            } => row
                .bg(tokens.tab_active)
                .hover(move |style| style.bg(hover)),
            _ => row.hover(move |style| style.bg(hover)),
        })
}

/// Virtualized row list with `4px` horizontal inset matching `header_row`.
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
        .px(ROW_INSET)
        .pt(LIST_TOP)
        .pb(LIST_BOTTOM)
}

/// Overlay vertical scrollbar starting below `HEADER_HEIGHT`.
pub fn scrollbar(scroll: &UniformListScrollHandle) -> impl IntoElement {
    scrollbar_from(HEADER_HEIGHT, scroll)
}

/// Overlay vertical scrollbar starting at `top` (use `px(0.)` for headerless lists).
pub fn scrollbar_from(top: Pixels, scroll: &UniformListScrollHandle) -> impl IntoElement {
    div()
        .absolute()
        .top(top)
        .right_0()
        .bottom_0()
        .w(Scrollbar::width())
        .child(
            Scrollbar::vertical(scroll)
                .viewport_from_layout()
                .max_fps(60),
        )
}
