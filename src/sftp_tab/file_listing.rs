// 来自 since2006/shell-rs @ b2a307a: src/explorer/file_listing.rs
// 裁剪：去掉单栏「文件」的窄列表、切换语言和 `FileSizeFormat`（大小固定简短格式）；
// 颜色取原型的 token：选中是 `--sel-solid`，悬停只在名称格上，行本身透明（表格的颜色见 `theme::component`）。
//! 一张文件卡片的行和列，交给 `DataTable`。选择归卡片（按名字记），表格自己的选行关着。

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::marquee::{ListGeometry, Pressed, ends_a_drag};
use super::{
    CardSide, ClickMode, FileCard, FileEntry, FileIcon, Selection, SftpTabId, child_path,
    format_changed, format_rights, format_size,
};
use crate::app::{SftpAction, SftpDispatch as _, SftpTabCommand};
use crate::backend::sftp::TransferDirection;
use crate::i18n::t;
use crate::theme::Tokens;

/// 行的回调要从卡片知道的东西，由 `FileCard` 推进来：渲染时不读别的实体。
#[derive(Clone, Default)]
pub(super) struct ListingContext {
    pub tab: SftpTabId,
    pub path: String,
    pub selection: Rc<Selection>,
    pub transfer_enabled: bool,
    pub dispatch: Option<FocusHandle>,
    pub card: Option<WeakEntity<FileCard>>,
    /// 上一次右键点到了什么，列表的右键菜单读它。
    pub menu_hit: Rc<RefCell<Option<MenuHit>>>,
    /// 行在哪里，给框选用。
    pub geometry: Rc<ListGeometry>,
    /// 没有行时列表写什么。
    pub placeholder: ListPlaceholder,
}

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

/// 列表里的右键点到了什么；空白处是 `None`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MenuHit {
    /// 文件或文件夹，按名字。
    Item(String),
    /// 列标题，按列的键。
    Column(SharedString),
}

/// 一张文件卡片的行和列。
pub struct FileListing {
    columns: Vec<Column>,
    /// 目录里的全部，读到的样子。
    listed: Vec<FileEntry>,
    /// 显示的行，排好序：`listed` 去掉不显示的隐藏文件。
    rows: Vec<FileEntry>,
    show_hidden: bool,
    sort: Option<(usize, ColumnSort)>,
    side: CardSide,
    /// 用户拖过列宽：之后不再按百分比摆。
    dragged: bool,
    context: ListingContext,
}

/// 列表放不下时的最窄的列。
const MIN_COLUMN_WIDTH: Pixels = px(48.);
/// 列表右边留给竖滚动条的地方。
const SCROLLBAR_ROOM: Pixels = px(10.);

/// 两侧每列占列表宽度的百分比（原型 `fcard` 的 `colgroup`）。
fn column_shares(side: CardSide) -> &'static [f32] {
    match side {
        CardSide::Local => &[44., 13., 19., 24.],
        CardSide::Remote => &[34., 13., 22., 16., 15.],
    }
}

/// 两侧的列（原型）。量到列表的宽度之前先用这些宽度。箭头是 `render_th` 自己画的：
/// 原型只在排序的那一列上有箭头，不用表格那种每列都有的。
fn columns(side: CardSide) -> Vec<Column> {
    let column =
        |key: &'static str, width: f32| Column::new(key, column_title(key)).width(px(width));
    match side {
        CardSide::Local => vec![
            name_column(px(240.)),
            column("size", 80.),
            column("type", 100.),
            column("modified", 130.),
        ],
        CardSide::Remote => vec![
            name_column(px(200.)),
            column("size", 80.),
            column("modified", 130.),
            column("rights", 90.),
            column("owner", 80.),
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

impl FileListing {
    pub fn new(side: CardSide) -> Self {
        Self {
            columns: columns(side),
            listed: Vec::new(),
            rows: Vec::new(),
            show_hidden: false,
            sort: None,
            side,
            dragged: false,
            context: ListingContext::default(),
        }
    }

    pub(super) fn configure(&mut self, context: ListingContext) {
        self.context = context;
    }

    /// 记住拖到的列宽：每次 `refresh`（每次列目录都有）表格都重新取一遍列。
    pub(super) fn set_widths(&mut self, widths: &[Pixels]) {
        for (column, width) in self.columns.iter_mut().zip(widths) {
            if column.width != *width {
                self.dragged = true;
                column.width = *width;
            }
        }
    }

    /// 按原型的百分比摆列，列表宽 `width`。用户拖过就不动了。返回变了没有。
    pub(super) fn fit_columns(&mut self, width: Pixels) -> bool {
        if self.dragged {
            return false;
        }
        let room = f32::from((width - SCROLLBAR_ROOM).max(px(0.)));
        let mut changed = false;
        for (column, share) in self.columns.iter_mut().zip(column_shares(self.side)) {
            let fitted = px((room * share / 100.).floor()).max(MIN_COLUMN_WIDTH);
            if column.width != fitted {
                column.width = fitted;
                changed = true;
            }
        }
        changed
    }

    /// 点列标题排序，和点箭头一样：降序、升序、回到按名称。
    fn cycle_sort(&mut self, col_ix: usize) {
        let sort = match self.sort {
            Some((ix, ColumnSort::Descending)) if ix == col_ix => ColumnSort::Ascending,
            Some((ix, ColumnSort::Ascending)) if ix == col_ix => ColumnSort::Default,
            _ => ColumnSort::Descending,
        };
        self.set_sort(col_ix, sort);
    }

    /// 按某一列排序。
    fn set_sort(&mut self, col_ix: usize, sort: ColumnSort) {
        self.sort = match sort {
            ColumnSort::Default => None,
            sort => Some((col_ix, sort)),
        };
        self.apply_sort();
    }

    /// 这一列标题后面的箭头：排序的那一列才有，没选时是按名称升序（原型 `th.srt`）。
    fn sort_arrow(&self, col_ix: usize) -> Option<&'static str> {
        match self.sort {
            Some((ix, ColumnSort::Descending)) if ix == col_ix => Some("\u{2193}"),
            Some((ix, _)) if ix == col_ix => Some("\u{2191}"),
            None if self.columns[col_ix].key.as_ref() == "name" => Some("\u{2191}"),
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

    /// 一行的项目在它那一侧的路径。
    fn child_path(&self, name: &str) -> String {
        child_path(&self.context.path, name, self.side == CardSide::Remote)
    }

    /// 让 `cell` 能把这一行拖到另一侧；这一行在选中项里时连选中的一起拖。
    /// 按下没选中的项目就只选它，这样高亮的就是要拖的。
    fn drag_from(&self, cell: Stateful<Div>, name: &str) -> Stateful<Div> {
        let names = if self.context.selection.contains(name) {
            self.context.selection.targets(&self.order())
        } else {
            vec![name.to_string()]
        };
        // 按着 Ctrl、Shift 的留给名称格的点击。
        let cell = match self.context.card.clone() {
            Some(card) => cell.on_mouse_down(MouseButton::Left, {
                let name = name.to_string();
                move |event, _, cx| {
                    if !event.modifiers.secondary() && !event.modifiers.shift {
                        let _ = card.update(cx, |card, cx| {
                            card.clear_menu_row(cx);
                            card.select_pressed(&name, cx);
                        });
                    }
                }
            }),
            None => cell,
        };
        let paths = names.iter().map(|name| self.child_path(name));
        match self.side {
            CardSide::Local => drag_files(
                cell,
                paths.map(PathBuf::from).collect(),
                TransferDirection::Upload,
            ),
            CardSide::Remote => drag_files(
                cell,
                paths.collect::<Vec<String>>(),
                TransferDirection::Download,
            ),
        }
    }

    /// 名称格的点击：选中（Ctrl 加减，Shift 扩展），双击打开，右键先选中，菜单才对着高亮的项目。
    fn name_events(&self, cell: Stateful<Div>, name: &str) -> Stateful<Div> {
        let geometry = self.context.geometry.clone();
        let cell = cell.on_any_mouse_down(move |_, _, _| geometry.pressed.set(Pressed::Name));
        let (Some(dispatch), Some(card)) =
            (self.context.dispatch.clone(), self.context.card.clone())
        else {
            return cell;
        };
        let tab = self.context.tab;
        let remote = self.side == CardSide::Remote;
        cell.on_click({
            let name = name.to_string();
            let card = card.clone();
            move |event, window, cx| {
                if ends_a_drag(event) {
                    return;
                }
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
                        &SftpAction::new(tab, SftpTabCommand::Open { remote }),
                        window,
                        cx,
                    );
                }
            }
        })
        .on_mouse_down(MouseButton::Right, {
            let hit = (name != "..").then(|| name.to_string());
            let menu_hit = self.context.menu_hit.clone();
            move |_, _, cx| {
                if let Some(hit) = &hit {
                    let _ = card.update(cx, |card, cx| card.select_pressed(hit, cx));
                }
                *menu_hit.borrow_mut() = hit.clone().map(MenuHit::Item);
            }
        })
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
            .and_then(|(col_ix, sort)| {
                self.columns
                    .get(col_ix)
                    .map(|column| (column.key.clone(), sort == ColumnSort::Descending))
            })
            .unwrap_or_else(|| ("name".into(), false));
        let start = usize::from(self.rows.first().is_some_and(FileEntry::is_parent));
        self.rows[start..].sort_by(|a, b| {
            let by_name = || a.name.to_lowercase().cmp(&b.name.to_lowercase());
            let by_key = match key.as_ref() {
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

impl TableDelegate for FileListing {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    /// `Column::text_right` 在 `DataTable` 里不生效，大小列自己靠右。点标题排序，不只是点箭头。
    /// 右键记下来给列表的菜单。
    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let column = &self.columns[col_ix];
        let key = column.key.clone();
        let arrow = self.sort_arrow(col_ix);
        let accent = Tokens::current(cx).accent;
        let menu_hit = self.context.menu_hit.clone();
        let geometry = self.context.geometry.clone();
        h_flex()
            .id(ElementId::Name(format!("column:{key}").into()))
            .test_support()
            .size_full()
            .text_size(px(11.5))
            .font_weight(FontWeight::MEDIUM)
            .when(key.as_ref() == "size", |this| this.justify_end())
            .when(key.as_ref() == "name", |this| this.px_1p5())
            .on_mouse_down(MouseButton::Right, move |_, _, _| {
                *menu_hit.borrow_mut() = Some(MenuHit::Column(key.clone()));
            })
            .on_any_mouse_down(move |_, _, _| geometry.pressed.set(Pressed::Title))
            // 箭头记在表格的列上，只有 `refresh` 才重新取。
            .on_click(cx.listener(move |table, _, _, cx| {
                cx.stop_propagation();
                table.delegate_mut().cycle_sort(col_ix);
                table.refresh(cx);
                cx.notify();
            }))
            .gap(px(3.))
            .child(column.name.clone())
            .children(arrow.map(|arrow| div().text_color(accent).child(arrow)))
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(entry) = self.rows.get(row_ix) else {
            return div().into_any_element();
        };
        let tokens = Tokens::current(cx);
        let parent = entry.is_parent();
        let faint = |cell: Div| cell.text_size(px(11.5)).text_color(tokens.faint);
        match self.columns[col_ix].key.as_ref() {
            // 名称格就是项目：它显示悬停、选中和光标，收点击；行的其余地方算空白。
            // 只有图标和文字能拖文件，从格子的其余地方拖是框选。
            "name" => {
                let selection = &self.context.selection;
                let selected = selection.contains(&entry.name);
                // 选中的名字不管光标在不在都显示选中；边框只用来在没选中的名字上找到光标：
                // `..`，或者刚用 Ctrl 取消的。
                let cursor = !selected && selection.cursor() == Some(entry.name.as_ref());
                let label = h_flex()
                    .id(ElementId::Name(format!("name:{}", entry.name).into()))
                    .h_full()
                    .min_w_0()
                    .gap(px(7.))
                    .child(FileIcon::entry(entry).dimmed(entry.is_hidden()))
                    .child(div().min_w_0().truncate().child(entry.name.clone()));
                let label = if parent {
                    label
                } else {
                    self.drag_from(label, &entry.name)
                };
                let cell = h_flex()
                    .id(ElementId::Name(format!("name-cell:{}", entry.name).into()))
                    .size_full()
                    .px_1p5()
                    .border_1()
                    .rounded(px(6.))
                    .border_color(if cursor {
                        tokens.accent
                    } else {
                        gpui_kit::transparent_black()
                    })
                    .map(|this| {
                        if selected {
                            this.bg(tokens.selection_solid)
                        } else {
                            this.hover(|this| this.bg(tokens.hover))
                        }
                    })
                    .child(label.test_support().aria_selected(selected));
                self.name_events(cell, &entry.name)
                    .test_support()
                    .into_any_element()
            }
            "size" => {
                let size = (!entry.is_dir()).then(|| format_size(entry.size));
                h_flex()
                    .id(ElementId::Name(format!("size:{}", entry.name).into()))
                    .test_support()
                    .aria_label(size.clone().unwrap_or_default())
                    .w_full()
                    .justify_end()
                    .text_size(px(11.5))
                    .text_color(tokens.faint)
                    .children(size)
                    .into_any_element()
            }
            "type" => faint(div())
                .child(if parent {
                    String::new()
                } else {
                    entry.type_label()
                })
                .into_any_element(),
            "modified" => faint(div())
                .child(if parent {
                    String::new()
                } else {
                    entry.modified.map(format_changed).unwrap_or_default()
                })
                .into_any_element(),
            "rights" => faint(div())
                .when(!parent, |this| {
                    this.children(entry.permissions.map(format_rights))
                })
                .into_any_element(),
            "owner" => faint(div())
                .when(!parent, |this| this.children(entry.owner.clone()))
                .into_any_element(),
            _ => div().into_any_element(),
        }
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let Some(entry) = self.rows.get(row_ix) else {
            return div().id(("empty-file-row", row_ix));
        };
        let name = entry.name.to_string();
        let context = &self.context;
        let tokens = Tokens::current(cx);
        // 悬停、选中和光标显示在名称格上，见 `render_td`。隐藏文件显示出来时比别的淡，整行都是。
        let row = div()
            .id(ElementId::Name(format!("file:{name}").into()))
            .when(entry.is_hidden(), |row| row.text_color(tokens.faint))
            // 这一行画在哪，就知道每一行在哪。要写明偏移：不写的绝对定位子元素会落到格子下面。
            .child({
                let geometry = context.geometry.clone();
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
            });
        let Some(dispatch) = context.dispatch.clone() else {
            return row;
        };
        if entry.is_parent() || !entry.is_dir() {
            return row;
        }
        // 放到目录这一行上，就是放进这个目录。
        accept_drops(
            row,
            self.side,
            context.transfer_enabled,
            dispatch,
            context.tab,
            self.child_path(&name),
        )
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.set_sort(col_ix, sort);
        cx.notify();
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let text = self.context.placeholder.text();
        let tokens = Tokens::current(cx);
        div()
            .id("list-placeholder")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.5))
            .text_color(tokens.faint)
            .child(text.clone())
            .test_support()
            .aria_label(text)
    }
}

/// 名称列自己画满整格的选中，所以没有表格的内边距：格子和标题自己留（`px_1p5`）。
fn name_column(width: Pixels) -> Column {
    Column::new("name", column_title("name"))
        .width(width)
        .paddings(Edges::all(px(0.)))
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
    let highlight = tokens.selection_solid;
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
            .text_size(px(11.5))
            .child(pointer)
            .child(text.clone())
            .test_support()
            .aria_label(text)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Pixels, px};

    use super::{CardSide, FileListing};
    use crate::sftp_tab::FileEntry;

    fn widths(listing: &FileListing) -> Vec<Pixels> {
        listing.columns.iter().map(|column| column.width).collect()
    }

    #[test]
    fn the_columns_share_the_list_as_the_prototype_does() {
        let mut local = FileListing::new(CardSide::Local);
        assert!(local.fit_columns(px(1010.)));
        assert_eq!(widths(&local), [px(440.), px(130.), px(190.), px(240.)]);
        // 同样的宽度：什么都没变，不用刷新表格。
        assert!(!local.fit_columns(px(1010.)));

        let mut remote = FileListing::new(CardSide::Remote);
        remote.fit_columns(px(1010.));
        assert_eq!(
            widths(&remote),
            [px(340.), px(130.), px(220.), px(160.), px(150.)]
        );
    }

    #[test]
    fn a_narrow_list_keeps_a_minimum_and_dragged_columns_stay() {
        let mut listing = FileListing::new(CardSide::Remote);
        listing.fit_columns(px(100.));
        assert!(widths(&listing).iter().all(|width| *width == px(48.)));

        let dragged = [px(300.), px(60.), px(60.), px(60.), px(60.)];
        listing.set_widths(&dragged);
        assert!(!listing.fit_columns(px(1010.)));
        assert_eq!(widths(&listing), dragged);
    }

    #[test]
    fn folders_stay_first_and_the_parent_on_top_whatever_the_sort() {
        let mut listing = FileListing::new(CardSide::Local);
        listing.set_rows(vec![
            FileEntry::parent(),
            FileEntry::file("b.txt", 1),
            FileEntry::file("a.txt", 9),
            FileEntry::dir("目录"),
        ]);
        assert_eq!(listing.order(), ["..", "目录", "a.txt", "b.txt"]);
        // 第 1 列是大小：第一次点降序，按字节数。
        listing.cycle_sort(1);
        assert_eq!(listing.order(), ["..", "目录", "a.txt", "b.txt"]);
        assert_eq!(listing.sort_arrow(1), Some("\u{2193}"));
        assert_eq!(listing.sort_arrow(0), None);
        listing.cycle_sort(1);
        assert_eq!(listing.order(), ["..", "目录", "b.txt", "a.txt"]);
        // 第三次回到按名称。
        listing.cycle_sort(1);
        assert_eq!(listing.order(), ["..", "目录", "a.txt", "b.txt"]);
        assert_eq!(listing.sort_arrow(0), Some("\u{2191}"));
    }
}
