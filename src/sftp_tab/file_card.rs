// 参考 since2006/shell-rs @ b2a307a: src/explorer/file_pane.rs 改写
// 裁剪：去掉两行工具栏、目录下拉、书签、历史（后退前进）、单栏「文件」的窄列表和目录跟随。
// 外观按全部界面原型的 `fcard`：图标、「本机」或主机名、当前路径的头部，路径栏、文件列表、状态栏。
//! 文件卡片：SFTP 标签里本地或远程的一侧。

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{Sizable as _, Size};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::file_listing::{ListPlaceholder, ListingContext, MenuHit, accept_drops, offer_drop};
use super::file_menu::{CardMenuState, directory_menu, item_menu};
use super::marquee::{self, ListMarquee, MarqueeList};
use super::path_bar::PathEdit;
use super::{
    ClickMode, CursorMotion, FileEntry, FileListing, Selection, SftpTabId, ShowHiddenFiles,
    child_path,
};
use crate::app::SftpTabCommand;
use crate::app::{
    LOCAL_FILE_LIST_CONTEXT, REMOTE_FILE_LIST_CONTEXT, SftpAction, SftpDispatch as _,
};
use crate::backend::sftp::{DirectoryListing, RemotePath, SharedLocalDirectoryProvider};
use crate::backend::{Latency, latency_label};
use crate::card::render_program_tile;
use crate::i18n::t;
use crate::programs::Program;
use crate::theme::{CARD_HEADER_HEIGHT, CARD_RADIUS, STATUS_BAR_HEIGHT, Tokens};

/// 目录读多久还没回来，状态栏才说「正在读取」：大多数目录一下就到，说了只会闪。
const SLOW_LOAD: Duration = Duration::from_millis(300);

/// 表格的行高（原型 `.ftab td`）。
const ROW_HEIGHT: Pixels = px(25.);

/// 文件卡片是本地还是远程的一侧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardSide {
    Local,
    Remote,
}

impl CardSide {
    pub fn from_remote(remote: bool) -> Self {
        if remote { Self::Remote } else { Self::Local }
    }

    pub fn is_remote(self) -> bool {
        self == Self::Remote
    }

    /// 卡片的元素 id。一次只画一个 SFTP 标签，不用带标签的 id。
    pub(super) fn card_id(self) -> &'static str {
        match self {
            Self::Local => "local-card",
            Self::Remote => "remote-card",
        }
    }

    pub(super) fn header_id(self) -> &'static str {
        match self {
            Self::Local => "local-header",
            Self::Remote => "remote-header",
        }
    }

    pub(super) fn path_id(self) -> &'static str {
        match self {
            Self::Local => "local-path",
            Self::Remote => "remote-path",
        }
    }

    pub(super) fn path_input_id(self) -> &'static str {
        match self {
            Self::Local => "local-path-input",
            Self::Remote => "remote-path-input",
        }
    }

    pub(super) fn path_hint_id(self) -> &'static str {
        match self {
            Self::Local => "local-path-hint",
            Self::Remote => "remote-path-hint",
        }
    }

    fn status_id(self) -> &'static str {
        match self {
            Self::Local => "local-status",
            Self::Remote => "remote-status",
        }
    }

    fn list_id(self) -> &'static str {
        match self {
            Self::Local => "local-file-list",
            Self::Remote => "remote-file-list",
        }
    }

    fn key_context(self) -> &'static str {
        match self {
            Self::Local => LOCAL_FILE_LIST_CONTEXT,
            Self::Remote => REMOTE_FILE_LIST_CONTEXT,
        }
    }
}

/// 远程一侧的连接状态（本地一侧总是 `Connected`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
}

/// 这次读目录是为什么，决定读不出来时怎么办。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadIntent {
    /// 去别的目录、重读：读不出来写在状态栏，目录不变。
    Visit,
    /// 打开标签时的初始目录：读不出来就静默改读下一个（焦点卡片的目录 → 主目录）。
    Start { fallback: Vec<String> },
    /// 路径栏输入的：读不出来就描红写原因，留在输入框，不切换。
    PathBar,
}

impl LoadIntent {
    /// 读不出来时改读哪里，和那次的打算。
    fn next_start(&self) -> Option<(String, LoadIntent)> {
        let Self::Start { fallback } = self else {
            return None;
        };
        let (first, rest) = fallback.split_first()?;
        Some((
            first.clone(),
            Self::Start {
                fallback: rest.to_vec(),
            },
        ))
    }
}

/// 文件卡片。
pub struct FileCard {
    pub(super) side: CardSide,
    /// 所在的 SFTP 标签，命令发给它。
    tab: SftpTabId,
    /// 头部的名称：「本机」或主机名。
    name: SharedString,
    /// 头部的图块：本机是显示器，远程是主机的系统徽章。
    tile: Program,
    pub(super) path: String,
    home: String,
    table: Entity<TableState<FileListing>>,
    /// 指针下的那一级路径。
    pub(super) hovered_part: Option<String>,
    /// 路径栏上一帧的宽度，决定折叠哪几级。
    pub(super) label_width: Rc<Cell<Option<Pixels>>>,
    /// 用过的最后一侧：标签再到前面来时焦点回到这里，边框也亮着。
    pub(super) current: bool,
    selection: Selection,
    menu_hit: Rc<RefCell<Option<MenuHit>>>,
    marquee: ListMarquee,
    /// 正在读的目录是为什么读的。
    pending: Option<LoadIntent>,
    /// 下一次列目录后选中这一行（新建、重命名之后）。
    select_after_load: Option<String>,
    request_id: u64,
    pub(super) loading: bool,
    slow_load: bool,
    slow_load_timer: Option<Task<()>>,
    /// 为编辑器读的文件，读得久了状态栏才说。
    opening: Option<String>,
    opening_timer: Option<Task<()>>,
    /// 读目录出了什么错，或者连接为什么断了。
    error: Option<String>,
    /// 读到过目录：这时候的空列表才是空目录。
    listed: bool,
    connection: ConnectionState,
    /// `user@地址:端口`（远程一侧）。
    endpoint: Option<SharedString>,
    latency: Option<Latency>,
    transfer_enabled: bool,
    /// 这一侧有文件操作在跑（工单 21）。
    busy: bool,
    /// 路径栏的输入态。
    pub(super) path_edit: Option<PathEdit>,
    pub(super) dispatch: FocusHandle,
    _subscriptions: Vec<Subscription>,
    load_task: Option<Task<()>>,
}

impl FileCard {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        side: CardSide,
        tab: SftpTabId,
        name: SharedString,
        tile: Program,
        home: String,
        dispatch: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut listing = FileListing::new(side);
        listing.set_show_hidden(ShowHiddenFiles::get(side, cx));
        // 选择归卡片（按名字多选）；表格自己的选行关着，免得画出另一种高亮。
        let table = cx.new(|cx| {
            TableState::new(listing, window, cx)
                .row_selectable(false)
                .col_selectable(false)
                .cell_selectable(false)
        });
        let subscriptions = vec![
            cx.subscribe(&table, |_, table, event: &TableEvent, cx| {
                if let TableEvent::ColumnWidthsChanged(widths) = event {
                    table.update(cx, |table, _| table.delegate_mut().set_widths(widths));
                }
            }),
            cx.observe_global::<ShowHiddenFiles>(|card, cx| card.sync_hidden_files(cx)),
        ];
        let mut card = Self {
            side,
            tab,
            name,
            tile,
            path: home.clone(),
            home,
            table,
            hovered_part: None,
            label_width: Rc::default(),
            current: side == CardSide::Remote,
            selection: Selection::default(),
            menu_hit: Rc::default(),
            marquee: ListMarquee::default(),
            pending: None,
            select_after_load: None,
            request_id: 0,
            loading: false,
            slow_load: false,
            slow_load_timer: None,
            opening: None,
            opening_timer: None,
            error: None,
            listed: false,
            connection: if side == CardSide::Local {
                ConnectionState::Connected
            } else {
                ConnectionState::Connecting
            },
            endpoint: None,
            latency: None,
            transfer_enabled: false,
            busy: false,
            path_edit: None,
            dispatch,
            _subscriptions: subscriptions,
            load_task: None,
        };
        card.sync_listing(cx);
        card
    }

    pub(super) fn is_remote(&self) -> bool {
        self.side.is_remote()
    }

    /// 现在显示的目录。
    pub fn path(&self) -> String {
        self.path.clone()
    }

    pub fn home(&self) -> String {
        self.home.clone()
    }

    pub fn is_current(&self) -> bool {
        self.current
    }

    /// 菜单、路径栏接不接命令。断开时接：标签回答「要不要重新连接」；只有正在连接时只能等。
    pub fn takes_commands(&self) -> bool {
        self.connection != ConnectionState::Connecting
    }

    pub(super) fn dispatch_command(
        &self,
        command: SftpTabCommand,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.dispatch
            .dispatch_sftp_action(&SftpAction::new(self.tab, command), window, cx);
    }

    /// 选中的名字，按显示的顺序：文件命令的对象。
    pub fn selected_names(&self, cx: &App) -> Vec<String> {
        let table = self.table.read(cx);
        self.selection.targets(&table.delegate().order())
    }

    pub fn selected_entries(&self, cx: &App) -> Vec<FileEntry> {
        let table = self.table.read(cx);
        let listing = table.delegate();
        self.selection
            .targets(&listing.order())
            .iter()
            .filter_map(|name| listing.entry(listing.position(name)?).cloned())
            .collect()
    }

    /// 键盘光标所在的行。
    pub fn cursor_entry(&self, cx: &App) -> Option<FileEntry> {
        let cursor = self.selection.cursor()?;
        let table = self.table.read(cx);
        let listing = table.delegate();
        listing.entry(listing.position(cursor)?).cloned()
    }

    /// 显示的行，按显示的顺序。
    pub fn entries(&self, cx: &App) -> Vec<FileEntry> {
        self.table.read(cx).delegate().rows().to_vec()
    }

    /// 目录里的全部，隐藏文件也在：新名字不能和它们撞。
    pub fn listed_entries(&self, cx: &App) -> Vec<FileEntry> {
        self.table.read(cx).delegate().listed().to_vec()
    }

    /// 这一侧的隐藏文件显示或不显示了：重新列行。隐藏起来的文件也从选中项里去掉。
    fn sync_hidden_files(&mut self, cx: &mut Context<Self>) {
        let show = ShowHiddenFiles::get(self.side, cx);
        if self.table.read(cx).delegate().shows_hidden() == show {
            return;
        }
        self.table.update(cx, |table, cx| {
            table.delegate_mut().set_show_hidden(show);
            table.refresh(cx);
        });
        self.update_selection(cx, |selection, order| selection.retain(order));
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.table.read(cx).focus_handle(cx)
    }

    pub fn child_path_of(&self, name: &str) -> String {
        child_path(&self.path, name, self.is_remote())
    }

    pub fn parent_path(&self) -> String {
        if self.is_remote() {
            RemotePath::new(&self.path)
                .map(|path| path.parent().to_string())
                .unwrap_or_else(|_| "/".into())
        } else {
            PathBuf::from(&self.path)
                .parent()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.path.clone())
        }
    }

    pub fn expanded_path(&self, path: &str) -> String {
        super::expand_path(path, &self.path, &self.home, self.is_remote())
    }

    /// 连上时知道了主目录。还没有目录时就停在那里。
    pub fn set_home(&mut self, home: String) {
        if self.home.is_empty() {
            self.path = home.clone();
        }
        self.home = home;
    }

    pub fn set_available(
        &mut self,
        connection: ConnectionState,
        transfer: bool,
        cx: &mut Context<Self>,
    ) {
        self.connection = connection;
        self.transfer_enabled = transfer;
        self.sync_listing(cx);
        cx.notify();
    }

    /// 状态栏上的 `user@地址:端口` 和延迟。
    pub(super) fn set_link(
        &mut self,
        endpoint: Option<SharedString>,
        latency: Option<Latency>,
        cx: &mut Context<Self>,
    ) {
        self.endpoint = endpoint;
        self.latency = latency;
        cx.notify();
    }

    pub fn set_current(&mut self, current: bool, cx: &mut Context<Self>) {
        if self.current != current {
            self.current = current;
            cx.notify();
        }
    }

    pub(super) fn hover_part(&mut self, path: &str, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.hovered_part = Some(path.to_string());
        } else if self.hovered_part.as_deref() == Some(path) {
            self.hovered_part = None;
        } else {
            return;
        }
        cx.notify();
    }

    /// 这一侧有文件操作在跑（工单 21）：菜单里改文件的项不能点。
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        cx.notify();
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// 下一次列目录后选中这一行。
    pub fn select_after_load(&mut self, name: String) {
        self.select_after_load = Some(name);
    }

    fn sync_listing(&mut self, cx: &mut Context<Self>) {
        let context = ListingContext {
            tab: self.tab,
            path: self.path.clone(),
            selection: Rc::new(self.selection.clone()),
            transfer_enabled: self.transfer_enabled,
            dispatch: Some(self.dispatch.clone()),
            card: Some(cx.entity().downgrade()),
            menu_hit: self.menu_hit.clone(),
            geometry: self.marquee.geometry.clone(),
            placeholder: self.placeholder(self.table.read(cx).delegate().hidden_count()),
        };
        self.table.update(cx, |table, cx| {
            table.delegate_mut().configure(context);
            cx.notify();
        });
    }

    /// 没有行时列表写什么：读到过目录才写「空目录」，之前写正在干什么。
    /// 空目录在重读时一直写「空目录」，读得久了才改口，免得快速刷新时闪。
    /// 只有隐藏文件、又不显示的目录不算空。
    fn placeholder(&self, hidden: usize) -> ListPlaceholder {
        if self.connection == ConnectionState::Connecting {
            ListPlaceholder::Connecting
        } else if self.loading && (!self.listed || self.slow_load) {
            ListPlaceholder::Reading
        } else if self.listed && hidden > 0 {
            ListPlaceholder::OnlyHidden
        } else if self.listed {
            ListPlaceholder::Empty
        } else if self.connection == ConnectionState::Disconnected {
            ListPlaceholder::NotConnected
        } else if self.error.is_some() {
            ListPlaceholder::Unreadable
        } else {
            // 第一次读还没发出去。
            ListPlaceholder::Reading
        }
    }

    fn update_selection(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Selection, &[&str]),
    ) {
        let order: Vec<String> = self
            .table
            .read(cx)
            .delegate()
            .order()
            .into_iter()
            .map(str::to_string)
            .collect();
        let order: Vec<&str> = order.iter().map(String::as_str).collect();
        change(&mut self.selection, &order);
        self.sync_listing(cx);
        cx.notify();
    }

    /// 鼠标点了一行，修饰键已经算好。
    pub fn click_row(&mut self, name: &str, mode: ClickMode, cx: &mut Context<Self>) {
        self.clear_menu_row(cx);
        self.update_selection(cx, |selection, order| selection.click(name, mode, order));
    }

    /// 按下没选中的行就只选它：右键菜单对着高亮的项目，拖动也只拖它。选中的行不动选择，拖动带上全部。
    pub fn select_pressed(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self.selection.contains(name) {
            self.update_selection(cx, |selection, order| {
                selection.click(name, ClickMode::Replace, order)
            });
        }
    }

    /// 表格给右键过的行描边，选择归我们，所以下一次点击、移动、拖动都把它清掉。
    pub(super) fn clear_menu_row(&mut self, cx: &mut Context<Self>) {
        self.table
            .update(cx, |table, cx| table.set_right_clicked_row(None, cx));
    }

    pub fn select_only(&mut self, name: &str, cx: &mut Context<Self>) {
        self.update_selection(cx, |selection, order| selection.select_only(name, order));
        self.scroll_to_cursor(cx);
    }

    pub fn move_cursor(&mut self, motion: CursorMotion, extend: bool, cx: &mut Context<Self>) {
        self.clear_menu_row(cx);
        let page = self.table.read(cx).visible_range().rows().len();
        self.update_selection(cx, |selection, order| {
            selection.move_cursor(motion, page, extend, order);
        });
        self.scroll_to_cursor(cx);
    }

    /// 光标行出了可见范围才滚；表格自己的 `scroll_to_row` 总把那行放到最上面。
    fn scroll_to_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(cursor) = self.selection.cursor().map(str::to_string) else {
            return;
        };
        self.table.update(cx, |table, cx| {
            let Some(ix) = table.delegate().position(&cursor) else {
                return;
            };
            let visible = table.visible_range().rows().clone();
            if visible.is_empty() || ix < visible.start {
                table.scroll_to_row(ix, cx);
            } else if ix + 1 >= visible.end {
                table.scroll_to_row(ix + 2 - visible.len().max(1), cx);
            }
        });
    }

    pub fn toggle_selection(&mut self, cx: &mut Context<Self>) {
        self.update_selection(cx, |selection, order| selection.toggle_cursor(order));
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.update_selection(cx, |selection, order| selection.select_all(order));
    }

    /// 本地选中项的路径：要上传的。
    pub fn upload_sources(&self, cx: &App) -> Vec<PathBuf> {
        self.selected_names(cx)
            .into_iter()
            .map(|name| PathBuf::from(&self.path).join(name))
            .collect()
    }

    /// 连接断了：`message` 写在状态栏，正在读的作废。
    pub fn disconnected(&mut self, message: String, cx: &mut Context<Self>) {
        self.request_id += 1;
        self.pending = None;
        self.finish_loading();
        self.error = Some(message);
        self.sync_listing(cx);
        cx.notify();
    }

    /// 读目录出了什么错，或者连接为什么断了，下一次读目录时清掉。
    pub fn problem(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// 开始读目录，返回这次的请求号。路径栏的失败不写卡片的错误，也不清它。
    pub fn begin_load(&mut self, intent: LoadIntent, cx: &mut Context<Self>) -> u64 {
        self.request_id += 1;
        if intent != LoadIntent::PathBar {
            self.error = None;
        }
        self.pending = Some(intent);
        self.loading = true;
        self.sync_listing(cx);
        let id = self.request_id;
        self.slow_load = false;
        self.slow_load_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SLOW_LOAD).await;
            let _ = this.update(cx, |this, cx| {
                if this.loading && this.request_id == id {
                    this.slow_load = true;
                    this.sync_listing(cx);
                    cx.notify();
                }
            });
        }));
        cx.notify();
        self.request_id
    }

    /// 这一侧的一个文件正在为编辑器读：读得久了状态栏才说，和读目录一样。
    pub fn begin_opening(&mut self, name: String, cx: &mut Context<Self>) {
        self.opening = None;
        self.opening_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SLOW_LOAD).await;
            let _ = this.update(cx, |this, cx| {
                this.opening = Some(name);
                cx.notify();
            });
        }));
    }

    pub fn finish_opening(&mut self, cx: &mut Context<Self>) {
        self.opening_timer = None;
        if self.opening.take().is_some() {
            cx.notify();
        }
    }

    fn finish_loading(&mut self) {
        self.loading = false;
        self.slow_load = false;
        self.slow_load_timer = None;
    }

    /// 读目录的结果。返回要改读哪里：初始目录读不出来时，调用方接着读下一个。
    #[must_use]
    pub fn apply_listing(
        &mut self,
        request_id: u64,
        result: Result<DirectoryListing, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(String, LoadIntent)> {
        if request_id != self.request_id {
            return None;
        }
        self.finish_loading();
        let pending = self.pending.take();
        match result {
            Ok(listing) => {
                if listing.path() != self.path {
                    self.selection.clear();
                    self.hovered_part = None;
                }
                self.path = listing.path().into();
                self.listed = true;
                self.error = None;
                let mut rows: Vec<_> = listing
                    .entries()
                    .iter()
                    .map(FileEntry::from_directory_entry)
                    .collect();
                if self.parent_path() != self.path {
                    rows.insert(0, FileEntry::parent());
                }
                self.table.update(cx, |table, cx| {
                    table.delegate_mut().set_rows(rows);
                    table.refresh(cx);
                });
                self.update_selection(cx, |selection, order| selection.retain(order));
                if let Some(name) = self.select_after_load.take() {
                    self.select_only(&name, cx);
                }
                if pending == Some(LoadIntent::PathBar) {
                    self.close_path_edit(window, cx);
                }
            }
            Err(error) => {
                self.select_after_load = None;
                match pending {
                    Some(LoadIntent::PathBar) => self.path_edit_failed(error.into(), cx),
                    Some(intent @ LoadIntent::Start { .. }) => {
                        if let Some(next) = intent.next_start() {
                            self.sync_listing(cx);
                            cx.notify();
                            return Some(next);
                        }
                        self.error = Some(error);
                    }
                    _ => self.error = Some(error),
                }
                self.sync_listing(cx);
            }
        }
        cx.notify();
        None
    }

    /// 在后台读本地目录。初始目录读不出来时接着读下一个。
    pub fn load_local(
        &mut self,
        path: String,
        intent: LoadIntent,
        provider: SharedLocalDirectoryProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.begin_load(intent, cx);
        self.load_task = Some(cx.spawn_in(window, async move |this, cx| {
            let reader = provider.clone();
            let result = cx
                .background_spawn(async move {
                    reader.list(&PathBuf::from(path)).map_err(|error| {
                        t!("sftp_tab.list.read_failed", error = format!("{error:#}")).to_string()
                    })
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some((next, intent)) = this.apply_listing(id, result, window, cx) {
                    this.load_local(next, intent, provider, window, cx);
                }
            });
        }));
    }

    /// 菜单要显示的东西，开菜单时读。
    pub(super) fn menu_state(&self, cx: &App) -> CardMenuState {
        let targets = self.selected_names(cx);
        let entries = self.selected_entries(cx);
        CardMenuState {
            remote: self.is_remote(),
            tab: self.tab,
            edits_file: match entries.as_slice() {
                [entry] if !entry.is_dir() && !entry.is_parent() => {
                    Some(self.child_path_of(&entry.name))
                }
                _ => None,
            },
            targets,
            can_go_up: self.parent_path() != self.path,
            can_modify: self.takes_commands() && !self.busy,
            can_transfer: self.transfer_enabled,
            show_hidden: ShowHiddenFiles::get(self.side, cx),
        }
    }

    /// 状态栏显示的东西（原型 `fcard` 的 `stat`）。
    fn status(&self, cx: &App) -> CardStatus {
        let (items, selected, hidden) = {
            let table = self.table.read(cx);
            let listing = table.delegate();
            let items = listing.rows().iter().filter(|row| !row.is_parent()).count();
            (
                items,
                self.selection.targets(&listing.order()).len(),
                listing.hidden_count(),
            )
        };
        let hidden = (hidden > 0).then(|| t!("sftp_tab.status.hidden", count = hidden));
        let counts: SharedString = if selected > 0 {
            t!(
                "sftp_tab.status.items_selected",
                count = items,
                selected = selected
            )
        } else {
            t!("sftp_tab.status.items", count = items)
        };
        let note = if let Some(name) = &self.opening {
            Some(t!("sftp_tab.status.opening", name = name))
        } else if self.slow_load {
            Some(t!("sftp_tab.list.reading"))
        } else {
            None
        };
        let problem = self.error.clone().map(SharedString::from);
        match (self.side, self.connection) {
            (CardSide::Local, _) => CardStatus {
                state: None,
                details: vec![counts],
                problem,
                note,
                right: hidden.into_iter().collect(),
            },
            (CardSide::Remote, ConnectionState::Connecting) => CardStatus {
                state: Some((t!("sftp_tab.status.connecting"), StatusTone::Pending)),
                details: self.endpoint.clone().into_iter().collect(),
                problem: None,
                note: None,
                right: Vec::new(),
            },
            (CardSide::Remote, ConnectionState::Connected) => CardStatus {
                state: Some((t!("sftp_tab.status.connected"), StatusTone::Ok)),
                details: self
                    .endpoint
                    .clone()
                    .into_iter()
                    .chain(self.latency.map(|latency| latency_label(latency).into()))
                    .collect(),
                problem,
                note,
                right: std::iter::once(counts).chain(hidden).collect(),
            },
            (CardSide::Remote, ConnectionState::Disconnected) => CardStatus {
                state: Some((t!("sftp_tab.status.disconnected"), StatusTone::Bad)),
                details: Vec::new(),
                problem,
                note: None,
                right: self.listed.then_some(counts).into_iter().collect(),
            },
        }
    }

    fn render_header(&self, tokens: &Tokens) -> impl IntoElement + use<> {
        let offline = self.connection != ConnectionState::Connected;
        let tile = if offline {
            self.tile.clone().offline()
        } else {
            self.tile.clone()
        };
        div()
            .id(self.side.header_id())
            .test_support()
            .aria_label(format!("{} {}", self.name, self.path))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(7.))
            .h(CARD_HEADER_HEIGHT)
            .pl(px(12.))
            .pr(px(8.))
            .child(render_program_tile(
                ElementId::Name(format!("{}-tile", self.side.card_id()).into()),
                &tile,
                tokens,
            ))
            .child(
                div()
                    .flex_none()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .whitespace_nowrap()
                    .when(offline, |name| name.text_color(tokens.offline))
                    .child(self.name.clone()),
            )
            .child(
                div()
                    .min_w_0()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(tokens.muted)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(self.path.clone()),
            )
    }

    fn render_status(&self, cx: &App, tokens: &Tokens) -> impl IntoElement + use<> {
        let status = self.status(cx);
        let segment = |text: SharedString| div().min_w_0().truncate().child(text);
        let state_color = status.state.as_ref().map(|(_, tone)| match tone {
            StatusTone::Ok => tokens.busy,
            StatusTone::Pending => tokens.faint,
            StatusTone::Bad => tokens.bad,
        });
        div()
            .id(self.side.status_id())
            .test_support()
            .aria_label(status.label())
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .h(STATUS_BAR_HEIGHT)
            .pl(px(12.))
            .pr(px(10.))
            .text_size(px(11.))
            .text_color(tokens.faint)
            .border_t_1()
            .border_color(tokens.line)
            .whitespace_nowrap()
            .overflow_hidden()
            .when_some(status.state.clone(), |bar, (state, _)| {
                bar.child(
                    div()
                        .flex_none()
                        .when_some(state_color, |state, color| state.text_color(color))
                        .child(format!("\u{25cf} {state}")),
                )
            })
            .children(status.details.iter().cloned().map(segment))
            .when_some(status.problem.clone(), |bar, problem| {
                bar.child(segment(problem).text_color(tokens.bad))
            })
            .when_some(status.note.clone(), |bar, note| bar.child(segment(note)))
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .flex_none()
                    .gap(px(12.))
                    .children(status.right.iter().cloned().map(segment)),
            )
    }
}

/// 状态的颜色：连着是绿的，正在连是灰的，断开是红的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusTone {
    Ok,
    Pending,
    Bad,
}

/// 状态栏显示的东西。
struct CardStatus {
    /// 「● 已连接」：只有远程一侧有。
    state: Option<(SharedString, StatusTone)>,
    details: Vec<SharedString>,
    /// 红字：断开的原因，或者读目录的错误。
    problem: Option<SharedString>,
    /// 灰字：正在读取、正在打开。
    note: Option<SharedString>,
    right: Vec<SharedString>,
}

impl CardStatus {
    /// 无障碍名称：状态栏上从左到右的每一段，用「 · 」隔开。测试靠它断言。
    fn label(&self) -> String {
        self.state
            .iter()
            .map(|(state, _)| state.to_string())
            .chain(self.details.iter().map(ToString::to_string))
            .chain(self.problem.iter().map(ToString::to_string))
            .chain(self.note.iter().map(ToString::to_string))
            .chain(self.right.iter().map(ToString::to_string))
            .collect::<Vec<_>>()
            .join(" \u{b7} ")
    }
}

impl MarqueeList for FileCard {
    type Rows = FileListing;

    fn table(&self) -> &Entity<TableState<FileListing>> {
        &self.table
    }

    fn list_marquee(&self) -> &ListMarquee {
        &self.marquee
    }

    fn list_marquee_mut(&mut self) -> &mut ListMarquee {
        &mut self.marquee
    }

    fn selection(&self, _: &App) -> Selection {
        self.selection.clone()
    }

    fn update_selection(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Selection, &[&str]),
    ) {
        FileCard::update_selection(self, cx, change);
    }

    /// 任何一次按下都去掉 `DataTable` 给右键过的行画的描边：菜单对着什么，只看选中。
    fn on_list_press(&mut self, cx: &mut Context<Self>) {
        self.clear_menu_row(cx);
    }
}

impl Render for FileCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let side = self.side;
        let remote = self.is_remote();
        let focused = self.current;
        let card = cx.entity().downgrade();
        let menu_hit = self.menu_hit.clone();
        let dispatch = self.dispatch.clone();
        let list_focus = self.focus_handle(cx);
        let drop_highlight = tokens.selection_solid;
        let header_focus = list_focus.clone();
        div()
            .id(side.card_id())
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .rounded(CARD_RADIUS)
            .bg(if focused {
                tokens.card_focused
            } else {
                tokens.card
            })
            .border_1()
            .border_color(if focused {
                tokens.card_focused_border
            } else {
                tokens.card_border
            })
            .shadow(vec![BoxShadow {
                color: if focused {
                    tokens.shadow_focused
                } else {
                    tokens.shadow
                },
                offset: point(px(0.), px(6.)),
                blur_radius: px(22.),
                spread_radius: px(-2.),
                inset: false,
            }])
            .child(
                // 点头部也把焦点交给这一侧的文件列表。
                div()
                    .flex_none()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.focus(&header_focus, cx)
                    })
                    .child(self.render_header(&tokens)),
            )
            .child(self.render_path_bar(window, &tokens, cx))
            .child(
                // 名称格和标题在按下冒泡时说一声，剩下的就是空白：从那里拖是框选，
                // 点一下或者右键清掉选中（WinSCP）。
                marquee::select_area(
                    div()
                        .id(side.list_id())
                        .flex_1()
                        .min_h_0()
                        .mx(px(4.))
                        .text_size(px(12.5))
                        .key_context(side.key_context()),
                    self,
                    cx,
                )
                // 行和列标题的右键在这个捕获之后才记，所以空白处什么都没记。
                .capture_any_mouse_down({
                    let menu_hit = menu_hit.clone();
                    move |event, _, _| {
                        if event.button == MouseButton::Right {
                            *menu_hit.borrow_mut() = None;
                        }
                    }
                })
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|card, event: &MouseDownEvent, _, cx| {
                        marquee::press_empty(card, event.modifiers, cx)
                    }),
                )
                // 放到列表本身的文件进它的目录。
                .when(remote, |this| {
                    this.drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(drop_highlight))
                })
                .when(self.transfer_enabled, |this| {
                    if remote {
                        offer_drop::<PathBuf>(this, &tokens)
                    } else {
                        offer_drop::<String>(this, &tokens)
                    }
                })
                .map(|this| {
                    accept_drops(
                        this,
                        side,
                        self.transfer_enabled,
                        self.dispatch.clone(),
                        self.tab,
                        self.path.clone(),
                    )
                })
                // 行、列标题和空白处共用一个菜单，挂在容器上：两个菜单会一起开。
                .context_menu(move |menu, _, cx| {
                    let menu = menu.action_context(dispatch.clone());
                    let Some(state) = card.upgrade().map(|card| card.read(cx).menu_state(cx))
                    else {
                        return menu;
                    };
                    match menu_hit.borrow().clone() {
                        Some(MenuHit::Item(_)) => item_menu(menu, &state),
                        // 列标题没有菜单：空菜单不会打开。
                        Some(MenuHit::Column(_)) => menu,
                        None => directory_menu(menu, &state),
                    }
                })
                .child(
                    DataTable::new(&self.table)
                        .stripe(false)
                        .bordered(false)
                        .with_size(Size::Size(ROW_HEIGHT)),
                )
                .child(self.marquee.canvas())
                .children(self.marquee.rectangle(cx)),
            )
            .child(
                div()
                    .flex_none()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.focus(&list_focus, cx)
                    })
                    .child(self.render_status(cx, &tokens)),
            )
    }
}
