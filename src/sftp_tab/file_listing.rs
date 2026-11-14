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

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::marquee::{Pressed, ends_a_drag};
use super::{
    CardSide, ClickMode, FileCard, FileEntry, FileIcon, SftpTabId, format_changed, format_rights,
    format_size,
};
use crate::app::{SftpAction, SftpDispatch as _, SftpTabCommand};
use crate::backend::sftp::TransferDirection;
use crate::i18n::t;
use crate::theme::{MONO_FONT_FAMILY, Tokens, TypeScale, UI_FONT_FAMILY, baseline_shift, ui_text};
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
    OnlyHidden,
    Empty,
    NotConnected,
    Unreadable,
}

impl ListPlaceholder {
    fn text(self) -> SharedString {
        match self {
            Self::Connecting => t!("sftp_tab.list.connecting"),
            Self::Reading => t!("sftp_tab.list.reading"),
            Self::OnlyHidden => t!("sftp_tab.list.only_hidden"),
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
    columns: Vec<TableColumn>,
    /// 目录里的全部，读到的样子。
    listed: Vec<FileEntry>,
    /// 显示的行，排好序：`listed` 去掉不显示的隐藏文件。
    rows: Vec<FileEntry>,
    show_hidden: bool,
    /// 按哪一列、哪个方向排；`None` 是按名称升序。
    sort: Option<(usize, SortDirection)>,
}

impl FileListing {
    pub fn new(side: CardSide) -> Self {
        Self {
            columns: columns(side),
            listed: Vec::new(),
            rows: Vec::new(),
            show_hidden: false,
            sort: None,
        }
    }

    pub(super) fn columns(&self) -> &[TableColumn] {
        &self.columns
    }

    /// 点列标题排序：降序、升序、回到按名称。
    pub(super) fn cycle_sort(&mut self, col_ix: usize) {
        self.sort = match self.sort {
            Some((ix, SortDirection::Descending)) if ix == col_ix => {
                Some((col_ix, SortDirection::Ascending))
            }
            Some((ix, SortDirection::Ascending)) if ix == col_ix => None,
            _ => Some((col_ix, SortDirection::Descending)),
        };
        self.apply_sort();
    }

    /// 这一列标题后面的箭头：排序的那一列才有，没选时是按名称升序（26 号 `.sf-hd .so`）。
    pub(super) fn sort_arrow(&self, col_ix: usize) -> Option<SortDirection> {
        match self.sort {
            Some((ix, direction)) if ix == col_ix => Some(direction),
            None if self.columns[col_ix].key == "name" => Some(SortDirection::Ascending),
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

    /// 没显示的隐藏文件有几个。
    pub fn hidden_count(&self) -> usize {
        self.listed.len() - self.rows.len()
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
        self.filter();
    }

    pub fn shows_hidden(&self) -> bool {
        self.show_hidden
    }

    pub fn set_show_hidden(&mut self, show: bool) {
        if self.show_hidden != show {
            self.show_hidden = show;
            self.filter();
        }
    }

    fn filter(&mut self) {
        let show_hidden = self.show_hidden;
        self.rows = self
            .listed
            .iter()
            .filter(|entry| show_hidden || !entry.is_hidden())
            .cloned()
            .collect();
        self.apply_sort();
    }

    /// 按选的列排序，没选就按名称。`..` 总在最前，目录（和指向目录的链接）在文件前面。
    /// 按大小排序看的是字节数。
    fn apply_sort(&mut self) {
        let (key, descending) = self
            .sort
            .and_then(|(col_ix, direction)| {
                self.columns
                    .get(col_ix)
                    .map(|column| (column.key, direction == SortDirection::Descending))
            })
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
    /// 表头：点标题排序（降序、升序、回到按名称）。
    pub(super) fn render_header(&self, tokens: &Tokens, cx: &mut Context<Self>) -> Div {
        let geometry = self.marquee.geometry.clone();
        let columns = self.listing.columns();
        header_row(tokens).children((0..columns.len()).map(|col_ix| {
            let geometry = geometry.clone();
            header_cell(columns, col_ix, self.listing.sort_arrow(col_ix), tokens)
                .on_any_mouse_down(move |_, _, _| geometry.pressed.set(Pressed::Title))
                .on_click(cx.listener(move |card, _, _, cx| {
                    cx.stop_propagation();
                    card.listing.cycle_sort(col_ix);
                    cx.notify();
                }))
                .test_support()
        }))
    }

    /// 列表里 `range` 那几行（`uniform_list` 只要看得见的）。
    pub(super) fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        // 权限是等宽小字（`.sf-r .pm`，界面 12 时 11.5），和界面字放在同一条基线上（26 号整行按基线对齐）。
        let rights_size = scale.mono_small;
        let rights_shift = baseline_shift(
            window,
            (UI_FONT_FAMILY, scale.ui),
            (MONO_FONT_FAMILY, scale.mono_small),
            table::ROW_HEIGHT,
        );
        range
            .filter_map(|ix| {
                let entry = self.listing.entry(ix)?.clone();
                Some(self.render_row(ix, &entry, (rights_shift, rights_size), &tokens, cx))
            })
            .collect()
    }

    fn render_row(
        &self,
        row_ix: usize,
        entry: &FileEntry,
        (rights_shift, rights_size): (Pixels, Pixels),
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let columns = self.listing.columns();
        let name = entry.name.to_string();
        let parent = entry.is_parent();
        let selected = self.selection.contains(&name);
        // 键盘光标落在没选中的行上（`..`，或者刚用 Ctrl 取消的）时描一圈细边，好找到它。
        let cursor = !selected && self.selection.cursor() == Some(name.as_str());
        let geometry = self.marquee.geometry.clone();
        let card = cx.entity().downgrade();
        let state = RowState {
            selected,
            focused: self.current,
        };
        let dim = |cell: Div| cell.text_color(tokens.text2);
        let cells = columns.iter().enumerate().map(|(col_ix, column)| {
            let blank = parent && column.key != "name";
            match column.key {
                "name" => self.render_name(entry, selected, card.clone(), tokens),
                _ if blank => cell(columns, col_ix).into_any_element(),
                "size" => {
                    let size = (!entry.is_dir()).then(|| format_size(entry.size));
                    dim(cell(columns, col_ix))
                        .id(ElementId::Name(format!("size:{name}").into()))
                        .test_support()
                        .aria_label(size.clone().unwrap_or_default())
                        .children(size)
                        .into_any_element()
                }
                "type" => dim(text_cell(columns, col_ix, entry.type_label())).into_any_element(),
                "modified" => dim(text_cell(
                    columns,
                    col_ix,
                    entry.modified.map(format_changed).unwrap_or_default(),
                ))
                .into_any_element(),
                "rights" => dim(cell(columns, col_ix))
                    .child(
                        div()
                            .relative()
                            .top(rights_shift)
                            .font_family(MONO_FONT_FAMILY)
                            .text_size(rights_size)
                            .children(entry.permissions.map(format_rights)),
                    )
                    .into_any_element(),
                "owner" => dim(text_cell(
                    columns,
                    col_ix,
                    entry.owner.clone().unwrap_or_default(),
                ))
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
        // 隐藏文件显示出来时整行更淡（`.sf-r.hid`）。
        .when(entry.is_hidden(), |row| row.text_color(tokens.st_dim))
        .children(cells)
        .when(cursor, |row| {
            row.child(
                div()
                    .absolute()
                    .inset_0()
                    .border_1()
                    .border_color(tokens.line_strong),
            )
        })
        // 这一行画在哪，就知道每一行在哪（框选用）。要写明偏移：不写的绝对定位子元素会落到格子下面。
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
        // 按在行上（名字以外）：点一下选中它，拖是框选。名字自己先说了就不改。
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
                } else if modifiers.secondary() {
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
        // 右键先选中这一行（没选中时只选它），菜单才对着高亮的项目；`..` 当空白处。
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
        // 放到目录这一行上，就是放进这个目录。
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

    /// 名称格：图标（14，单色）和名字。图标和名字能拖到另一侧，`..` 不能。
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
        let label = div()
            .id(ElementId::Name(format!("name:{name}").into()))
            .flex()
            .h_full()
            .min_w_0()
            .items_center()
            .gap(ICON_GAP)
            .on_any_mouse_down(move |_, _, _| geometry.pressed.set(Pressed::Name))
            // 图标在 GPUI 里比几何居中高（150% 实测比字面中心高 1 个设备像素），往下挪半个像素。
            .child(div().flex_none().relative().top(ICON_NUDGE).child(
                FileIcon::entry(entry).sized(
                    ICON_SIZE,
                    if entry.is_hidden() {
                        tokens.st_dim
                    } else {
                        tokens.icon
                    },
                ),
            ))
            .child(div().min_w_0().truncate().child(entry.name.clone()));
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
                if !event.modifiers.secondary() && !event.modifiers.shift {
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

    /// 没有行时列表写的字，在表头下面居中。
    pub(super) fn render_placeholder(
        &self,
        placeholder: ListPlaceholder,
        tokens: &Tokens,
    ) -> AnyElement {
        let text = placeholder.text();
        div()
            .id("list-placeholder")
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .text_color(tokens.faint)
            .child(text.clone())
            .aria_label(text)
            .test_support()
            .into_any_element()
    }
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

/// 显示另一侧的行能放到 `element` 上，指针在它上面时告诉那次拖动。
pub(super) fn offer_drop<P: 'static>(element: Stateful<Div>, tokens: &Tokens) -> Stateful<Div> {
    let highlight = tokens.sel;
    element
        .drag_over::<FilesDrag<P>>(move |style, _, _, _| style.bg(highlight))
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
        // 拖动的东西最后画，它的光标说了算。
        let pointer = canvas(
            |_, _, _| {},
            move |_, _, window, _| window.set_window_cursor_style(cursor),
        )
        .absolute()
        .top_0()
        .left_0();
        if !droppable {
            return div().child(pointer).into_any_element();
        }
        let text = match self.direction {
            TransferDirection::Upload => t!("sftp_tab.drag.upload", count = self.count),
            TransferDirection::Download => t!("sftp_tab.drag.download", count = self.count),
        };
        let tokens = Tokens::current(cx);
        div()
            .id("file-drag-preview")
            .px(px(9.))
            .py(px(5.))
            .rounded(px(7.))
            .bg(tokens.tooltip)
            .text_color(tokens.tooltip_text)
            .text_size(ui_text())
            .child(pointer)
            .child(text.clone())
            .test_support()
            .aria_label(text)
            .into_any_element()
    }
}
