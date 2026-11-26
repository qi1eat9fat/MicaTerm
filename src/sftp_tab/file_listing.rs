// 来自 since2006/shell-rs @ b2a307a: src/explorer/file_listing.rs
// 裁剪：去掉单栏「文件」的窄列表、切换语言和 `FileSizeFormat`（大小固定简短格式）、拖列宽。
// 改造：外观照 26 号「SFTP」（`ui::table`，Zed data_table）：整行是一个项目，悬停、选中都是整行。
//! 一侧文件列表的行和列：显示哪些行、怎么排、每一行怎么画。选择归卡片（按名字记）。
//!
//! 点一行任何地方都选中它（Ctrl 加减、Shift 扩展），双击打开，右键先选中再开菜单；
//! 从图标和名字上拖是把文件拖到另一侧，从行的别处或空白处拖是框选（同资源管理器的详细信息视图）。

use std::cell::Cell;
use std::cmp::Ordering;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::marquee::{Pressed, ends_a_drag};
use super::{
    CardSide, ClickMode, FileCard, FileEntry, FileIcon, SftpTabId, format_changed, format_rights,
    format_size,
};
use crate::app::{CatalogIcon, SftpAction, SftpDispatch as _, SftpTabCommand};
use crate::backend::sftp::TransferDirection;
use crate::i18n::t;
use crate::theme::space;
use crate::theme::{
    Elevation, MONO_FONT_FAMILY, Tokens, TypeScale, UI_FONT_FAMILY, WEIGHT_NORMAL, baseline_shift,
    size,
};
use crate::ui::empty::{EmptyPlacement, EmptyState};
use crate::ui::table::{
    self, ColumnWidth, RowState, SortDirection, TableColumn, cell, header_cell, header_row,
    text_cell,
};

/// 没有行时列表写什么，画的时候再翻成文字。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ListPlaceholder {
    Connecting,
    #[default]
    Reading,
    Empty,
    NotConnected,
    Unreadable,
}

impl ListPlaceholder {
    fn text(self) -> SharedString {
        match self {
            Self::Connecting => t!("sftp_tab.list.connecting"),
            Self::Reading => t!("sftp_tab.list.reading"),
            Self::Empty => t!("sftp_tab.list.empty"),
            Self::NotConnected => t!("sftp_tab.list.not_connected"),
            Self::Unreadable => t!("sftp_tab.list.unreadable"),
        }
    }
}

/// 名称格里图标的边长、图标和名字之间（`.sf-r .nm`：图标在 12，名字在 34）。
const ICON_SIZE: Pixels = px(14.);
const ICON_GAP: Pixels = px(8.);
/// 图标往下挪多少才和名字的字面居中（真程序 150% 实测，ui-lessons 第二节）。
const ICON_NUDGE: Pixels = px(0.5);

/// 两侧的列（26 号 `grid-template-columns`）：本地 名称 / 大小 76 / 类型 96 / 修改时间 124，
/// 远程 名称 / 大小 64 / 修改时间 124 / 权限 84 / 所有者 84。
fn columns(side: CardSide) -> Vec<TableColumn> {
    let fixed = |key: &'static str, width: f32| {
        TableColumn::new(key, column_title(key), ColumnWidth::Fixed(px(width)))
    };
    let name = TableColumn::new("name", column_title("name"), ColumnWidth::Fill);
    match side {
        CardSide::Local => vec![
            name,
            fixed("size", 76.).right(),
            fixed("type", 96.),
            fixed("modified", 124.),
        ],
        CardSide::Remote => vec![
            name,
            fixed("size", 64.).right(),
            fixed("modified", 124.),
            fixed("rights", 84.),
            fixed("owner", 84.),
        ],
    }
}

/// 卡片窄得放不下所有列时，名称列至少留这么宽（含图标和左右边距）。
const NAME_MIN_WIDTH: Pixels = px(140.);

/// 放不下时先收起哪几列，越前面越先收；名称列从不收。
fn fold_order(side: CardSide) -> &'static [&'static str] {
    match side {
        CardSide::Local => &["type", "size", "modified"],
        CardSide::Remote => &["owner", "rights", "size", "modified"],
    }
}

/// `width` 宽的列表里要收起哪几列：名称列留够 `NAME_MIN_WIDTH`，不够就按 `order` 一列列收。
fn folded_columns(
    columns: &[TableColumn],
    order: &[&'static str],
    width: Pixels,
) -> Vec<&'static str> {
    let fixed = |key: &str| {
        columns
            .iter()
            .find(|column| column.key == key)
            .and_then(|column| match column.width {
                ColumnWidth::Fixed(width) => Some(width),
                ColumnWidth::Fill => None,
            })
            .unwrap_or_default()
    };
    let mut needed = NAME_MIN_WIDTH
        + columns
            .iter()
            .map(|column| fixed(column.key))
            .fold(Pixels::ZERO, |sum, width| sum + width);
    let mut folded = Vec::new();
    for &key in order {
        if needed <= width {
            break;
        }
        needed -= fixed(key);
        folded.push(key);
    }
    folded
}

/// 列标题，按列的键。
fn column_title(key: &str) -> SharedString {
    match key {
        "name" => t!("sftp_tab.column.name"),
        "size" => t!("sftp_tab.column.size"),
        "type" => t!("sftp_tab.column.type"),
        "modified" => t!("sftp_tab.column.modified"),
        "rights" => t!("sftp_tab.column.rights"),
        "owner" => t!("sftp_tab.column.owner"),
        _ => SharedString::default(),
    }
}

/// 一侧文件列表的行和列。
pub struct FileListing {
    /// 这一侧所有的列。
    all_columns: Vec<TableColumn>,
    /// 放得下、显示出来的列（`fit` 按宽度从 `all_columns` 里挑）。
    columns: Vec<TableColumn>,
    fold_order: &'static [&'static str],
    /// 目录里的全部，读到的样子（点开头的隐藏文件也在）。
    listed: Vec<FileEntry>,
    /// 显示的行：`listed` 的副本，排好序。
    rows: Vec<FileEntry>,
    /// 按哪一列（列的键）、哪个方向排；`None` 是按名称升序。
    sort: Option<(&'static str, SortDirection)>,
}

impl FileListing {
    pub fn new(side: CardSide) -> Self {
        let all_columns = columns(side);
        Self {
            columns: all_columns.clone(),
            all_columns,
            fold_order: fold_order(side),
            listed: Vec::new(),
            rows: Vec::new(),
            sort: None,
        }
    }

    pub(super) fn columns(&self) -> &[TableColumn] {
        &self.columns
    }

    /// 按列表的宽度挑显示哪几列（上一帧量的，还没量过就全显示）：名称列放不下就收起次要的列。
    pub(super) fn fit(&mut self, width: Option<Pixels>) {
        let folded = width
            .map(|width| folded_columns(&self.all_columns, self.fold_order, width))
            .unwrap_or_default();
        self.columns = self
            .all_columns
            .iter()
            .filter(|column| !folded.contains(&column.key))
            .cloned()
            .collect();
    }

    /// 点列标题排序：降序、升序、回到按名称。
    pub(super) fn cycle_sort(&mut self, key: &'static str) {
        self.sort = match self.sort {
            Some((sorted, SortDirection::Descending)) if sorted == key => {
                Some((key, SortDirection::Ascending))
            }
            Some((sorted, SortDirection::Ascending)) if sorted == key => None,
            _ => Some((key, SortDirection::Descending)),
        };
        self.apply_sort();
    }

    /// 这一列标题后面的箭头：排序的那一列才有，没选时是按名称升序（26 号 `.sf-hd .so`）。
    pub(super) fn sort_arrow(&self, key: &str) -> Option<SortDirection> {
        match self.sort {
            Some((sorted, direction)) if sorted == key => Some(direction),
            None if key == "name" => Some(SortDirection::Ascending),
            _ => None,
        }
    }

    /// 显示的行，按显示的顺序。
    pub fn rows(&self) -> &[FileEntry] {
        &self.rows
    }

    /// 目录里的全部，隐藏文件也在。
    pub fn listed(&self) -> &[FileEntry] {
        &self.listed
    }

    /// 行的名字，按显示的顺序，`..` 也在。
    pub fn order(&self) -> Vec<&str> {
        self.rows.iter().map(|row| row.name.as_ref()).collect()
    }

    pub fn entry(&self, ix: usize) -> Option<&FileEntry> {
        self.rows.get(ix)
    }

    pub fn position(&self, name: &str) -> Option<usize> {
        self.rows.iter().position(|row| row.name.as_ref() == name)
    }

    pub fn set_rows(&mut self, rows: Vec<FileEntry>) {
        self.listed = rows;
        self.rows = self.listed.clone();
        self.apply_sort();
    }

    /// 按选的列排序，没选就按名称。`..` 总在最前，目录（和指向目录的链接）在文件前面。
    /// 按大小排序看的是字节数。
    fn apply_sort(&mut self) {
        let (key, descending) = self
            .sort
            .map(|(key, direction)| (key, direction == SortDirection::Descending))
            .unwrap_or(("name", false));
        let start = usize::from(self.rows.first().is_some_and(FileEntry::is_parent));
        self.rows[start..].sort_by(|a, b| {
            let by_name = || a.name.to_lowercase().cmp(&b.name.to_lowercase());
            let by_key = match key {
                "size" => a.size.cmp(&b.size),
                "modified" => a.modified.cmp(&b.modified),
                "type" => a.type_label().cmp(&b.type_label()),
                "rights" => a
                    .permissions
                    .map(format_rights)
                    .cmp(&b.permissions.map(format_rights)),
                "owner" => a.owner.cmp(&b.owner),
                _ => Ordering::Equal,
            }
            .then_with(by_name);
            let by_key = if descending { by_key.reverse() } else { by_key };
            b.is_dir().cmp(&a.is_dir()).then(by_key)
        });
    }
}

impl FileCard {
    /// Column header row (`11px Regular`, click to cycle sort: Desc -> Asc -> Name).
    pub(super) fn render_header(&self, tokens: &Tokens, cx: &mut Context<Self>) -> Div {
        let scale = TypeScale::current(cx);
        let geometry = self.marquee.geometry.clone();
        let columns = self.listing.columns();
        header_row(tokens, scale).children((0..columns.len()).map(|col_ix| {
            let geometry = geometry.clone();
            let key = columns[col_ix].key;
            header_cell(columns, col_ix, self.listing.sort_arrow(key), tokens)
                .on_any_mouse_down(move |_, _, _| geometry.pressed.set(Pressed::Title))
                .on_click(cx.listener(move |card, _, _, cx| {
                    cx.stop_propagation();
                    card.listing.cycle_sort(key);
                    cx.notify();
                }))
                .test_support()
        }))
    }

    /// Visible slice `range` of virtualized rows.
    pub(super) fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let meta_size = scale.sm;
        let ui_meta_shift = baseline_shift(
            window,
            (UI_FONT_FAMILY, scale.body),
            (UI_FONT_FAMILY, scale.sm),
            table::ROW_HEIGHT,
        );
        let rights_shift = baseline_shift(
            window,
            (UI_FONT_FAMILY, scale.body),
            (MONO_FONT_FAMILY, scale.sm),
            table::ROW_HEIGHT,
        );
        range
            .filter_map(|ix| {
                let entry = self.listing.entry(ix)?.clone();
                Some(self.render_row(
                    ix,
                    &entry,
                    (meta_size, ui_meta_shift, rights_shift),
                    &tokens,
                    cx,
                ))
            })
            .collect()
    }

    fn render_row(
        &self,
        row_ix: usize,
        entry: &FileEntry,
        (meta_size, ui_meta_shift, rights_shift): (Pixels, Pixels, Pixels),
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let columns = self.listing.columns();
        let name = entry.name.to_string();
        let parent = entry.is_parent();
        let selected = self.selection.contains(&name);
        let cursor = !selected && self.selection.cursor() == Some(name.as_str());
        let geometry = self.marquee.geometry.clone();
        let card = cx.entity().downgrade();
        let state = RowState {
            selected,
            focused: self.current,
        };
        let meta_cell = |cell: Div, color: Hsla| {
            cell.text_size(meta_size)
                .text_color(color)
                .relative()
                .top(ui_meta_shift)
        };
        let cells = columns.iter().enumerate().map(|(col_ix, column)| {
            let blank = parent && column.key != "name";
            match column.key {
                "name" => self.render_name(entry, selected, card.clone(), tokens),
                _ if blank => cell(columns, col_ix).into_any_element(),
                "size" => {
                    let size_str = (!entry.is_dir()).then(|| format_size(entry.size));
                    meta_cell(cell(columns, col_ix), tokens.text2)
                        .id(ElementId::Name(format!("size:{name}").into()))
                        .test_support()
                        .aria_label(size_str.clone().unwrap_or_default())
                        .children(size_str)
                        .into_any_element()
                }
                "type" => {
                    meta_cell(text_cell(columns, col_ix, entry.type_label()), tokens.st_dim)
                        .into_any_element()
                }
                "modified" => meta_cell(
                    text_cell(
                        columns,
                        col_ix,
                        entry.modified.map(format_changed).unwrap_or_default(),
                    ),
                    tokens.st_dim,
                )
                .into_any_element(),
                "rights" => cell(columns, col_ix)
                    .child(
                        div()
                            .relative()
                            .top(rights_shift)
                            .font_family(MONO_FONT_FAMILY)
                            .text_size(meta_size)
                            .children(
                                entry
                                    .permissions
                                    .map(|mode| styled_rights(&format_rights(mode), tokens)),
                            ),
                    )
                    .into_any_element(),
                "owner" => meta_cell(
                    text_cell(columns, col_ix, entry.owner.clone().unwrap_or_default()),
                    tokens.st_dim,
                )
                .into_any_element(),
                _ => cell(columns, col_ix).into_any_element(),
            }
        });
        let row = table::row(
            ElementId::Name(format!("file:{name}").into()),
            state,
            tokens,
        )
        .relative()
        .when(entry.is_hidden() || parent, |row| row.text_color(tokens.st_dim))
        .children(cells)
        .when(cursor, |row| {
            row.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(size::RADIUS_MD)
                    .border_1()
                    .border_color(tokens.line_strong),
            )
        })
        .child({
            let geometry = geometry.clone();
            canvas(
                move |bounds, _, _| {
                    let first = bounds.origin.y - bounds.size.height * row_ix as f32;
                    geometry.rows.set(Some((first, bounds.size.height)));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        })
        .on_any_mouse_down({
            let geometry = geometry.clone();
            move |_, _, _| {
                if geometry.pressed.get() == Pressed::Empty {
                    geometry.pressed.set(Pressed::Row);
                }
            }
        })
        .on_click({
            let (name, card) = (name.clone(), card.clone());
            let (dispatch, tab, side) = (self.dispatch.clone(), self.tab, self.side);
            move |event, window, cx| {
                if ends_a_drag(event) {
                    return;
                }
                cx.stop_propagation();
                let modifiers = event.modifiers();
                let mode = if modifiers.shift {
                    ClickMode::Extend
                } else if modifiers.secondary(cx) {
                    ClickMode::Toggle
                } else {
                    ClickMode::Replace
                };
                let _ = card.update(cx, |card, cx| card.click_row(&name, mode, cx));
                if mode == ClickMode::Replace && event.click_count() == 2 {
                    dispatch.dispatch_sftp_action(
                        &SftpAction::new(tab, SftpTabCommand::DoubleClick { side }),
                        window,
                        cx,
                    );
                }
            }
        })
        .on_mouse_down(MouseButton::Right, {
            let hit = (!parent).then(|| name.clone());
            let (menu_hit, card) = (self.menu_hit.clone(), card.clone());
            move |_, _, cx| {
                if let Some(hit) = &hit {
                    let _ = card.update(cx, |card, cx| card.select_pressed(hit, cx));
                }
                menu_hit.set(hit.is_some());
            }
        });
        if parent || !entry.is_dir() {
            return row.test_support().into_any_element();
        }
        accept_drops(
            row,
            self.side,
            self.transfer_enabled,
            self.dispatch.clone(),
            self.tab,
            self.child_path_of(&name),
        )
        .test_support()
        .into_any_element()
    }

    /// Name cell: categorized Lucide icon + filename (primary color for directories, dimmed for hidden/parent).
    fn render_name(
        &self,
        entry: &FileEntry,
        selected: bool,
        card: WeakEntity<FileCard>,
        tokens: &Tokens,
    ) -> AnyElement {
        let columns = self.listing.columns();
        let name = entry.name.to_string();
        let geometry = self.marquee.geometry.clone();
        let file_icon = FileIcon::entry(entry);
        let is_folder = file_icon.kind().is_folder();
        let icon_color = if entry.is_hidden() || entry.is_parent() {
            tokens.st_dim
        } else if is_folder {
            tokens.text_soft
        } else {
            tokens.icon
        };
        let label = div()
            .id(ElementId::Name(format!("name:{name}").into()))
            .flex()
            .h_full()
            .min_w_0()
            .items_center()
            .gap(ICON_GAP)
            .on_any_mouse_down(move |_, _, _| geometry.pressed.set(Pressed::Name))
            .child(
                div()
                    .flex_none()
                    .relative()
                    .top(ICON_NUDGE)
                    .child(file_icon.sized(ICON_SIZE, icon_color)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .when(entry.is_dir() && !entry.is_parent(), |text| {
                        text.font_weight(WEIGHT_NORMAL)
                    })
                    .child(entry.name.clone()),
            );
        let label = if entry.is_parent() {
            label
        } else {
            self.drag_from(label, &name, card)
        };
        cell(columns, 0)
            .id(ElementId::Name(format!("name-cell:{name}").into()))
            .child(label.aria_selected(selected).test_support())
            .test_support()
            .into_any_element()
    }

    /// 让 `label` 能把这一行拖到另一侧；这一行在选中项里时连选中的一起拖。
    /// 按下没选中的项目就只选它，这样高亮的就是要拖的（按着 Ctrl、Shift 的留给行的点击）。
    fn drag_from(
        &self,
        label: Stateful<Div>,
        name: &str,
        card: WeakEntity<FileCard>,
    ) -> Stateful<Div> {
        let order = self.listing.order();
        let names = if self.selection.contains(name) {
            self.selection.targets(&order)
        } else {
            vec![name.to_string()]
        };
        let label = label.on_mouse_down(MouseButton::Left, {
            let name = name.to_string();
            move |event, _, cx| {
                if !event.modifiers.secondary(cx) && !event.modifiers.shift {
                    let _ = card.update(cx, |card, cx| card.select_pressed(&name, cx));
                }
            }
        });
        let paths = names.iter().map(|name| self.child_path_of(name));
        match self.side {
            CardSide::Local => drag_files(
                label,
                paths.map(PathBuf::from).collect(),
                TransferDirection::Upload,
            ),
            CardSide::Remote => drag_files(
                label,
                paths.collect::<Vec<String>>(),
                TransferDirection::Download,
            ),
        }
    }

    /// Centered Muxy-style empty / loading / disconnected placeholder state.
    pub(super) fn render_placeholder(
        &self,
        placeholder: ListPlaceholder,
        tokens: &Tokens,
        scale: TypeScale,
    ) -> AnyElement {
        let icon = match placeholder {
            ListPlaceholder::Empty => Icon::new(IconName::Folder),
            ListPlaceholder::Connecting | ListPlaceholder::Reading => {
                Icon::new(IconName::LoaderCircle)
            }
            ListPlaceholder::NotConnected | ListPlaceholder::Unreadable => {
                Icon::new(CatalogIcon::ShieldX)
            }
        };
        EmptyState::new(
            self.side.placeholder_id(),
            placeholder.text(),
            EmptyPlacement::FileList,
        )
        .icon(icon)
        .render(tokens, scale)
    }
}

/// Highlights active permission bits (`r`, `w`, `x`, `s`, `t`) in `tokens.text2` while keeping `-`
/// dashes subtly dimmed in `tokens.faint`.
fn styled_rights(rights: &str, tokens: &Tokens) -> StyledText {
    let mut highlights = Vec::new();
    for (idx, ch) in rights.char_indices() {
        let color = if ch == '-' {
            tokens.faint
        } else {
            tokens.text2
        };
        highlights.push((
            idx..idx + ch.len_utf8(),
            HighlightStyle {
                color: Some(color),
                ..Default::default()
            },
        ));
    }
    StyledText::new(SharedString::from(rights.to_owned())).with_highlights(highlights)
}

/// 往另一侧拖的行，路径是它自己那一侧的。
#[derive(Clone)]
pub(super) struct FilesDrag<P> {
    pub paths: Vec<P>,
    pub spot: DropSpot,
}
pub(super) type LocalFilesDrag = FilesDrag<PathBuf>;
pub(super) type RemoteFilesDrag = FilesDrag<String>;

/// 让 `cell` 能拖到另一侧，带着 `paths`，放下去就往 `direction` 传。
fn drag_files<P: 'static>(
    cell: Stateful<Div>,
    paths: Vec<P>,
    direction: TransferDirection,
) -> Stateful<Div> {
    let drag = FilesDrag {
        paths,
        spot: DropSpot::default(),
    };
    cell.on_drag(drag, move |drag, _, _, cx| {
        cx.new(|_| FileDragPreview {
            direction,
            count: drag.paths.len(),
            spot: drag.spot.clone(),
        })
    })
}

/// Highlights `element` with a brand-soft outline when an incoming file drag hovers over it.
pub(super) fn offer_drop<P: 'static>(element: Stateful<Div>, tokens: &Tokens) -> Stateful<Div> {
    let highlight = tokens.brand_soft;
    let ring = tokens.brand_ring;
    element
        .drag_over::<FilesDrag<P>>(move |style, _, _, _| style.bg(highlight).border_color(ring))
        .on_drag_move(|event: &DragMoveEvent<FilesDrag<P>>, _, cx| {
            event
                .drag(cx)
                .spot
                .offer(event.event.position, event.bounds)
        })
}

/// 让 `element` 收下放给 `side` 那一侧目录 `target` 的东西：应用外面的文件和本地卡片的行
/// 上传到远程目录，远程卡片的行下载到本地目录。传输不能用（`enabled` 为假）时什么都不做。
pub(super) fn accept_drops(
    element: Stateful<Div>,
    side: CardSide,
    enabled: bool,
    dispatch: FocusHandle,
    tab: SftpTabId,
    target: String,
) -> Stateful<Div> {
    let send = move |command: SftpTabCommand, window: &mut Window, cx: &mut App| {
        if enabled {
            dispatch.dispatch_sftp_action(&SftpAction::new(tab, command), window, cx);
            cx.stop_propagation();
        }
    };
    match side {
        CardSide::Remote => element
            .on_drop({
                let (send, target) = (send.clone(), target.clone());
                move |paths: &ExternalPaths, window, cx| {
                    let (paths, target) = (paths.paths().to_vec(), target.clone());
                    send(SftpTabCommand::UploadPaths { paths, target }, window, cx)
                }
            })
            .on_drop(move |drag: &LocalFilesDrag, window, cx| {
                let (paths, target) = (drag.paths.clone(), target.clone());
                send(SftpTabCommand::UploadPaths { paths, target }, window, cx)
            }),
        CardSide::Local => element.on_drop(move |drag: &RemoteFilesDrag, window, cx| {
            let (paths, target) = (drag.paths.clone(), target.clone());
            send(SftpTabCommand::DownloadPaths { paths, target }, window, cx)
        }),
    }
}

/// 指针上一次在能收这次拖动的卡片上的位置，由那张卡片在指针移动时记下。只有在那里拖动才显示要做什么；
/// 别处（包括自己的列表）指针显示不行。记位置而不是标记，就不用谁去清：
/// GPUI 把每次移动告诉每张听这种拖动的卡片，指针不在上面的卡片不动它，指针一走它就过期了。
#[derive(Clone, Default)]
pub(super) struct DropSpot(Rc<Cell<Option<Point<Pixels>>>>);

impl DropSpot {
    /// 指针移到 `position`，在一张能收这次拖动、列表范围是 `bounds` 的卡片上。
    pub fn offer(&self, position: Point<Pixels>, bounds: Bounds<Pixels>) {
        if bounds.contains(&position) {
            self.0.set(Some(position));
        }
    }

    fn is_under_pointer(&self, window: &Window) -> bool {
        self.0.get() == Some(window.mouse_position())
    }
}

/// 拖动胶囊的入场时长：短到不拖慢手感，长到看得出是「拎起来了」。
const DRAG_PREVIEW_IN: Duration = Duration::from_millis(120);

struct FileDragPreview {
    direction: TransferDirection,
    count: usize,
    spot: DropSpot,
}

impl Render for FileDragPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let droppable = self.spot.is_under_pointer(window);
        let cursor = if droppable {
            CursorStyle::DragCopy
        } else {
            CursorStyle::OperationNotAllowed
        };
        let count = self.count;
        let direction = self.direction;
        let text = match direction {
            TransferDirection::Upload => {
                if count == 1 {
                    t!("sftp_tab.drag.upload_one", count = count)
                } else {
                    t!("sftp_tab.drag.upload", count = count)
                }
            }
            TransferDirection::Download => {
                if count == 1 {
                    t!("sftp_tab.drag.download_one", count = count)
                } else {
                    t!("sftp_tab.drag.download", count = count)
                }
            }
        };
        let label = text.clone();
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let icon = match direction {
            TransferDirection::Upload => CatalogIcon::Upload,
            TransferDirection::Download => CatalogIcon::Download,
        };
        // 每次拖动都是新视图，带自己的 id，入场动画才会从头播。
        let view = cx.entity_id();
        div()
            .id("file-drag-preview")
            .test_support()
            .aria_label(label)
            .child(div().with_animation(
                ("file-drag-preview-in", view),
                Animation::new(DRAG_PREVIEW_IN).with_easing(|t| 1. - (1. - t).powi(3)),
                move |_, t| {
                    // 胶囊一直跟着指针；从 86% 弹到 100%，像被拎起来一样。
                    let grow = 0.86 + 0.14 * t;
                    // 拖动的东西最后画，它的光标说了算。
                    let pointer = canvas(
                        |_, _, _| {},
                        move |_, _, window, _| window.set_window_cursor_style(cursor),
                    )
                    .absolute()
                    .top_0()
                    .left_0();
                    div()
                        .flex()
                        .items_center()
                        .gap(space::SPACE_6 * grow)
                        .px(space::SPACE_10 * grow)
                        .py(space::SPACE_4 * grow)
                        .rounded(size::RADIUS_MD)
                        .bg(tokens.popup)
                        .border_1()
                        .border_color(tokens.line)
                        .shadow(Elevation::Elevated.shadow(tokens.popup))
                        .text_color(tokens.text)
                        .text_size(scale.xs * grow)
                        .font_weight(WEIGHT_NORMAL)
                        .opacity(t)
                        .child(pointer)
                        .child(
                            Icon::new(icon)
                                .size(size::ICON_SM * grow)
                                .text_color(tokens.brand),
                        )
                        .child(text.clone())
                },
            ))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{CardSide, columns, fold_order, folded_columns};
    use gpui_kit::px;

    fn folded(side: CardSide, width: f32) -> Vec<&'static str> {
        folded_columns(&columns(side), fold_order(side), px(width))
    }

    #[test]
    fn keeps_every_column_when_they_fit() {
        assert!(folded(CardSide::Remote, 600.).is_empty());
        assert!(folded(CardSide::Local, 500.).is_empty());
    }

    #[test]
    fn folds_minor_columns_in_order_when_narrow() {
        // 远程定宽列共 356，名称至少 140。
        assert_eq!(folded(CardSide::Remote, 420.), ["owner"]);
        assert_eq!(folded(CardSide::Remote, 400.), ["owner", "rights"]);
        // 本地定宽列共 296。
        assert_eq!(folded(CardSide::Local, 353.), ["type"]);
        assert_eq!(folded(CardSide::Local, 280.), ["type", "size"]);
    }

    #[test]
    fn leaves_only_the_name_when_very_narrow() {
        assert_eq!(
            folded(CardSide::Remote, 100.),
            ["owner", "rights", "size", "modified"]
        );
    }
}
