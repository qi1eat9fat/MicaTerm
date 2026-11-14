// 参考 since2006/shell-rs @ b2a307a: src/explorer/file_pane.rs 改写
// 裁剪：去掉两行工具栏、目录下拉、书签、历史（后退前进）、单栏「文件」的窄列表和目录跟随。
// 外观照 26 号「SFTP」的一栏（`.sf-col`）：路径栏、表格（`ui::table`）、状态栏，没有卡片外框和头部。
//! 文件卡片：SFTP 标签里本地或远程的一侧（一栏）。

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use micaterm_ui::Menu;

use super::file_listing::{ListPlaceholder, accept_drops, offer_drop};
use super::file_menu::{CardMenuState, directory_menu, item_menu};
use super::marquee::{self, ListMarquee, MarqueeList};
use super::path_bar::PathEdit;
use super::{
    ClickMode, CursorMotion, FileEntry, FileListing, Selection, SftpTabId, ShowHiddenFiles,
    child_path,
};
use crate::app::{
    CatalogIcon, LOCAL_FILE_LIST_CONTEXT, REMOTE_FILE_LIST_CONTEXT, SftpAction, SftpDispatch as _,
    SftpTabCommand,
};
use crate::backend::sftp::{DirectoryListing, RemotePath, SharedLocalDirectoryProvider};
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale, size};
use crate::ui::table;

/// 目录读多久还没回来，状态栏才说「正在读取」：大多数目录一下就到，说了只会闪。
const SLOW_LOAD: Duration = Duration::from_millis(300);

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

    /// 另一侧。
    pub fn other(self) -> Self {
        match self {
            Self::Local => Self::Remote,
            Self::Remote => Self::Local,
        }
    }

    /// 卡片的元素 id。一次只画一个 SFTP 标签，不用带标签的 id。
    pub(super) fn card_id(self) -> &'static str {
        match self {
            Self::Local => "local-card",
            Self::Remote => "remote-card",
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

    /// 表格里放行的那一块（`uniform_list`）。
    fn rows_id(self) -> &'static str {
        match self {
            Self::Local => "local-file-rows",
            Self::Remote => "remote-file-rows",
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

/// 开着的右键菜单：画在鼠标处。
struct OpenMenu {
    position: Point<Pixels>,
    view: Entity<Menu>,
    _dismiss: Subscription,
}

/// 文件卡片：SFTP 标签里的一栏（26 号 `.sf-col`）。
pub struct FileCard {
    pub(super) side: CardSide,
    /// 所在的 SFTP 标签，命令发给它。
    pub(super) tab: SftpTabId,
    /// 状态栏上的名称：「本机」或主机名。
    name: SharedString,
    pub(super) path: String,
    home: String,
    pub(super) listing: FileListing,
    scroll: UniformListScrollHandle,
    list_focus: FocusHandle,
    /// 路径栏上一帧的宽度，决定折叠哪几级。
    pub(super) label_width: Rc<Cell<Option<Pixels>>>,
    /// 用过的最后一侧：标签再到前面来时焦点回到这里，选中的行也是 `--sel`。
    pub(super) current: bool,
    pub(super) selection: Selection,
    /// 上一次右键点在一行上（不是 `..`、不是空白处）：开文件的菜单还是空白处的菜单。
    pub(super) menu_hit: Rc<Cell<bool>>,
    menu: Option<OpenMenu>,
    pub(super) marquee: ListMarquee,
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
    pub(super) transfer_enabled: bool,
    /// 这一侧有文件操作在跑（工单 21）。
    busy: bool,
    /// 路径栏的输入态。
    pub(super) path_edit: Option<PathEdit>,
    pub(super) dispatch: FocusHandle,
    _subscriptions: Vec<Subscription>,
    load_task: Option<Task<()>>,
}

impl FileCard {
    pub fn new(
        side: CardSide,
        tab: SftpTabId,
        name: SharedString,
        home: String,
        dispatch: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut listing = FileListing::new(side);
        listing.set_show_hidden(ShowHiddenFiles::get(side, cx));
        let subscriptions =
            vec![cx.observe_global::<ShowHiddenFiles>(|card, cx| card.sync_hidden_files(cx))];
        Self {
            side,
            tab,
            name,
            path: home.clone(),
            home,
            listing,
            scroll: UniformListScrollHandle::new(),
            list_focus: cx.focus_handle(),
            label_width: Rc::default(),
            current: side == CardSide::Remote,
            selection: Selection::default(),
            menu_hit: Rc::default(),
            menu: None,
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
            transfer_enabled: false,
            busy: false,
            path_edit: None,
            dispatch,
            _subscriptions: subscriptions,
            load_task: None,
        }
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
    pub fn selected_names(&self, _: &App) -> Vec<String> {
        self.selection.targets(&self.listing.order())
    }

    pub fn selected_entries(&self, _: &App) -> Vec<FileEntry> {
        let listing = &self.listing;
        self.selection
            .targets(&listing.order())
            .iter()
            .filter_map(|name| listing.entry(listing.position(name)?).cloned())
            .collect()
    }

    /// 键盘光标所在的行。
    pub fn cursor_entry(&self, _: &App) -> Option<FileEntry> {
        let cursor = self.selection.cursor()?;
        let listing = &self.listing;
        listing.entry(listing.position(cursor)?).cloned()
    }

    /// 显示的行，按显示的顺序。
    pub fn entries(&self, _: &App) -> Vec<FileEntry> {
        self.listing.rows().to_vec()
    }

    /// 目录里的全部，隐藏文件也在：新名字不能和它们撞。
    pub fn listed_entries(&self, _: &App) -> Vec<FileEntry> {
        self.listing.listed().to_vec()
    }

    /// 这一侧的隐藏文件显示或不显示了：重新列行。隐藏起来的文件也从选中项里去掉。
    fn sync_hidden_files(&mut self, cx: &mut Context<Self>) {
        let show = ShowHiddenFiles::get(self.side, cx);
        if self.listing.shows_hidden() == show {
            return;
        }
        self.listing.set_show_hidden(show);
        self.update_selection(cx, |selection, order| selection.retain(order));
    }

    /// 文件列表的焦点：键盘命令在它上面。
    pub fn focus_handle(&self, _: &App) -> FocusHandle {
        self.list_focus.clone()
    }

    pub fn child_path_of(&self, name: &str) -> String {
        child_path(&self.path, name, self.side)
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
        super::expand_path(path, &self.path, &self.home, self.side)
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
        cx.notify();
    }

    pub fn set_current(&mut self, current: bool, cx: &mut Context<Self>) {
        if self.current != current {
            self.current = current;
            cx.notify();
        }
    }

    /// 这一侧有文件操作在跑（工单 21）：菜单里改文件的项不能点。
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        cx.notify();
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// 目录正在读。
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// 下一次列目录后选中这一行。
    pub fn select_after_load(&mut self, name: String) {
        self.select_after_load = Some(name);
    }

    /// 没有行时列表写什么：读到过目录才写「空目录」，之前写正在干什么。
    /// 空目录在重读时一直写「空目录」，读得久了才改口，免得快速刷新时闪。
    /// 只有隐藏文件、又不显示的目录不算空。
    fn placeholder(&self) -> ListPlaceholder {
        if self.connection == ConnectionState::Connecting {
            ListPlaceholder::Connecting
        } else if self.loading && (!self.listed || self.slow_load) {
            ListPlaceholder::Reading
        } else if self.listed && self.listing.hidden_count() > 0 {
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
        let order = self.listing.order();
        change(&mut self.selection, &order);
        cx.notify();
    }

    /// 鼠标点了一行，修饰键已经算好。
    pub fn click_row(&mut self, name: &str, mode: ClickMode, cx: &mut Context<Self>) {
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

    pub fn select_only(&mut self, name: &str, cx: &mut Context<Self>) {
        self.update_selection(cx, |selection, order| selection.select_only(name, order));
        self.scroll_to_cursor();
    }

    pub fn move_cursor(&mut self, motion: CursorMotion, extend: bool, cx: &mut Context<Self>) {
        // 一页是列表里放得下的行数。
        let page = self
            .marquee
            .geometry
            .list
            .get()
            .map(|list| ((list.size.height - table::HEADER_HEIGHT) / table::ROW_HEIGHT) as usize)
            .unwrap_or(1)
            .max(1);
        self.update_selection(cx, |selection, order| {
            selection.move_cursor(motion, page, extend, order);
        });
        self.scroll_to_cursor();
    }

    /// 光标行出了可见范围才滚。
    fn scroll_to_cursor(&mut self) {
        let Some(ix) = self
            .selection
            .cursor()
            .and_then(|cursor| self.listing.position(cursor))
        else {
            return;
        };
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
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
        let id = self.request_id;
        self.slow_load = false;
        self.slow_load_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SLOW_LOAD).await;
            let _ = this.update(cx, |this, cx| {
                if this.loading && this.request_id == id {
                    this.slow_load = true;
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
                    self.scroll.scroll_to_item(0, ScrollStrategy::Top);
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
                self.listing.set_rows(rows);
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
                            cx.notify();
                            return Some(next);
                        }
                        self.error = Some(error);
                    }
                    _ => self.error = Some(error),
                }
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
            side: self.side,
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
            has_properties: self.is_remote() || !cfg!(windows),
        }
    }

    /// 在 `position` 开右键菜单：右键点在一行上是文件的菜单，否则是空白处的菜单。
    fn open_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.menu_state(cx);
        let on_item = self.menu_hit.get();
        let focus = self.list_focus.clone();
        let view = Menu::build(window, cx, move |menu, _, _| {
            let menu = menu.action_context(focus);
            if on_item {
                item_menu(menu, &state)
            } else {
                directory_menu(menu, &state)
            }
        });
        let dismiss = cx.subscribe_in(
            &view,
            window,
            |this, menu: &Entity<Menu>, _: &DismissEvent, _, cx| {
                if this.menu.as_ref().is_some_and(|open| &open.view == menu) {
                    this.menu = None;
                }
                cx.notify();
            },
        );
        window.focus(&view.focus_handle(cx), cx);
        self.menu = Some(OpenMenu {
            position,
            view,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    /// 状态栏（26 号 `.status`，用户定只留「本机 / 主机名」和「共几项 · 已选几项」）。
    /// 正在连接、断开、读不出来、读得久了时名字后面写一句，平时不写连接状态。
    fn status(&self) -> CardStatus {
        let items = self
            .listing
            .rows()
            .iter()
            .filter(|row| !row.is_parent())
            .count();
        let selected = self.selection.targets(&self.listing.order()).len();
        let counts = self.listed.then(|| {
            t!(
                "sftp_tab.status.items_selected",
                count = items,
                selected = selected
            )
        });
        let note = if let Some(name) = &self.opening {
            Some(t!("sftp_tab.status.opening", name = name))
        } else if self.slow_load {
            Some(t!("sftp_tab.list.reading"))
        } else {
            None
        };
        let (state, problem, note) = match self.connection {
            ConnectionState::Connecting => (Some(t!("sftp_tab.status.connecting")), None, None),
            ConnectionState::Disconnected => (
                Some(t!("sftp_tab.status.disconnected")),
                self.error.clone().map(SharedString::from),
                None,
            ),
            ConnectionState::Connected => (None, self.error.clone().map(SharedString::from), note),
        };
        CardStatus {
            disconnected: self.connection == ConnectionState::Disconnected,
            state,
            problem,
            note,
            counts,
        }
    }

    fn render_status(&self, tokens: &Tokens, scale: TypeScale) -> impl IntoElement + use<> {
        let status = self.status();
        let icon = if self.is_remote() {
            CatalogIcon::Server
        } else {
            CatalogIcon::Monitor
        };
        let segment = |text: SharedString| div().min_w_0().truncate().child(text);
        let bad = tokens.err;
        div()
            .id(self.side.status_id())
            .test_support()
            .aria_label(status.label())
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .h(size::STATUS_BAR)
            .pl(px(12.))
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
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    // 13 的图标，和窗格状态栏一样往下挪半个像素才和字面居中（真程序 150% 实测）。
                    .child(
                        div()
                            .flex_none()
                            .relative()
                            .top(px(0.5))
                            .text_color(tokens.icon)
                            .child(Icon::new(icon).size(px(13.))),
                    )
                    .child(self.name.clone()),
            )
            .when_some(status.state.clone(), |bar, state| {
                bar.child(segment(state).text_color(if status.disconnected {
                    bad
                } else {
                    tokens.st_dim
                }))
            })
            .when_some(status.problem.clone(), |bar, problem| {
                bar.child(segment(problem).text_color(bad))
            })
            .when_some(status.note.clone(), |bar, note| bar.child(segment(note)))
            .child(div().ml_auto().flex_none().children(status.counts.clone()))
    }
}

/// 状态栏显示的东西。
struct CardStatus {
    disconnected: bool,
    /// 「正在连接」「已断开」：只有远程一侧有，连着时不写。
    state: Option<SharedString>,
    /// 红字：断开的原因，或者读目录的错误。
    problem: Option<SharedString>,
    /// 正在读取、正在打开。
    note: Option<SharedString>,
    /// 右边的「4 项 · 已选 1 项」，读到过目录才有。
    counts: Option<SharedString>,
}

impl CardStatus {
    /// 无障碍名称：名字后面的每一段和右边的计数，用「 · 」隔开。测试靠它断言。
    fn label(&self) -> String {
        self.state
            .iter()
            .chain(&self.problem)
            .chain(&self.note)
            .chain(&self.counts)
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" \u{b7} ")
    }
}

impl MarqueeList for FileCard {
    fn scroll_handle(&self) -> &UniformListScrollHandle {
        &self.scroll
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
}

impl Render for FileCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let side = self.side;
        let remote = self.is_remote();
        let list_focus = self.list_focus.clone();
        let menu_hit = self.menu_hit.clone();
        let drop_highlight = tokens.sel;
        let rows = self.listing.rows().len();
        let list = if rows == 0 {
            self.render_placeholder(self.placeholder(), &tokens)
        } else {
            table::body(side.rows_id(), rows, &self.scroll, cx, Self::render_rows)
                .into_any_element()
        };
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
            .bg(tokens.content)
            .text_size(scale.ui)
            .child(self.render_path_bar(window, &tokens, cx))
            .child(
                // 行和标题在按下冒泡时说一声，剩下的就是空白：从那里拖是框选，点一下或者右键清掉选中。
                marquee::select_area(
                    div()
                        .id(side.list_id())
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .key_context(side.key_context())
                        .track_focus(&self.list_focus),
                    self,
                    cx,
                )
                // 左键按下焦点给这一侧的列表；右键的焦点给菜单，关菜单时回到列表。
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.focus(&list_focus, cx)
                })
                // 行的右键在这个捕获之后才记，所以空白处什么都没记。
                .capture_any_mouse_down(move |event, _, _| {
                    if event.button == MouseButton::Right {
                        menu_hit.set(false);
                    }
                })
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|card, event: &MouseDownEvent, window, cx| {
                        marquee::press_empty(card, event.modifiers, cx);
                        card.open_menu(event.position, window, cx);
                        // 开菜单时焦点给了菜单，别让列表在按下之后再把它抢回去。
                        window.prevent_default();
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
                .child(self.render_header(&tokens, cx))
                .child(list)
                .child(table::scrollbar(&self.scroll))
                .child(self.marquee.canvas())
                .children(self.marquee.rectangle(&tokens)),
            )
            .child(self.render_status(&tokens, scale))
            .children(self.menu.as_ref().map(|menu| {
                deferred(
                    anchored()
                        .position(menu.position)
                        .snap_to_window_with_margin(px(8.))
                        .child(menu.view.clone()),
                )
                .with_priority(POPUP_PRIORITY)
            }))
    }
}
