// 参考 since2006/shell-rs @ b2a307a: src/editor/editor_panel.rs
// 改造：不是 Dock 面板，是标签轨道上的一项；对话框走 `DialogLayer`；样子照 26 号「编辑器」（照 Zed 编辑器）。
//! 编辑器标签（工单 23）：编辑一个远程文件或本地文件，不能分屏。
//!
//! 远程文件经打开它的 SFTP 标签的连接读写，本地文件直接读写磁盘。文件先读后开（`sftp_tab::file_open`）。
//! `has_unsaved_changes` 决定标签上的橙点和关闭时要不要问。
//!
//! 样子（26 号，UI 重做工单 11）：没有卡片外框、头部和「重新加载」「保存」按钮——保存走 Ctrl+S，状态栏写
//! 「未保存」，重新加载是状态栏最右边的小图标。代码区和终端同一种字体、字号、配色（`theme::code_colors`），
//! 底色是内容区；行号右对齐、淡色，当前行号稍亮，当前行一层很淡的底色。查找替换见 `find`。

mod find;
mod model;

use std::path::PathBuf;

use gpui_kit::base::input::{DiagnosticColors, InputEditorStyle};
use gpui_kit::component::input::{Editor, EditorState, InputEvent, Position, Rope, RopeExt as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Icon, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use micaterm_terminal::{TerminalColors, TerminalFont};

pub use model::{
    cursor_label, encoding_label, indentation, language_for, language_label, line_end_label,
};

use crate::app::{CatalogIcon, EDITOR_CONTEXT, ReloadEditor};
use crate::backend::sftp::{
    FileStamp, ReadFailure, RemotePath, SaveFailure, SharedLocalDirectoryProvider, TextFile,
    TextFormat, describe_read_failure, read_local_text, write_local_text,
};
use crate::card::StatusDot;
use crate::i18n::t;
use crate::programs::Program;
use crate::sftp_tab::{SftpTab, format_changed, format_size};
use crate::theme::space;
use crate::theme::{Tokens, TypeScale, size};
use crate::ui::DialogLayer;
use crate::ui::confirm::{
    Choice, ChoiceButton, ChoiceTone, Confirm, ConfirmLine, open_choice_dialog, open_confirm_dialog,
};
use crate::ui::icon_button::IconButton;

/// 编辑器的行高（26 号 `.ed-code`）。
const LINE_HEIGHT: f32 = 1.6;
/// 代码区上下留的边（26 号 `.ed-code` 的 padding 10，减去编辑器自己留的）。
const CODE_TOP: Pixels = px(4.);
/// 状态栏每一段左右的内边距（和终端状态栏一致）。
const SEGMENT_PAD: f32 = 7.;

/// 编辑器标签打开的文件在哪里。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EditorLocation {
    Local(PathBuf),
    Remote(RemotePath),
}

impl EditorLocation {
    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote(_))
    }

    /// 文件名。
    pub fn name(&self) -> String {
        match self {
            Self::Remote(path) => path.file_name().to_string(),
            Self::Local(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
        }
    }

    /// 完整路径，写法同文件卡片。
    pub fn path(&self) -> String {
        match self {
            Self::Remote(path) => path.to_string(),
            Self::Local(path) => path.to_string_lossy().into_owned(),
        }
    }

    /// 文件所在的目录，写法同文件卡片。
    pub fn directory(&self) -> String {
        match self {
            Self::Remote(path) => path.parent().to_string(),
            Self::Local(path) => path
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }
}

/// 编辑器标签要告诉窗口壳的事。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorTabEvent {
    /// 标签轨道上显示的东西变了（橙点）。
    Changed,
    /// 文件写好了：显示着它所在目录的文件卡片要重读。
    Saved,
    /// 这次写回结束了，不管成败（失败已经告诉用户了）：等着它的关闭、退出可以接着走了。
    SaveEnded { saved: bool },
    /// 问过了（或者不用问）：可以关掉这个标签。
    CloseRequested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Saving,
    Reloading,
}

/// 打开一个编辑器标签要的东西。
pub struct EditorOptions {
    pub location: EditorLocation,
    /// 状态栏左边的名字：主机名（本地文件写「本机」）。
    pub place: SharedString,
    /// 打开它的 SFTP 标签：本地文件是 `None`。
    pub sftp: Option<WeakEntity<SftpTab>>,
    pub local_files: SharedLocalDirectoryProvider,
    pub file: TextFile,
    /// 窗口壳的焦点：按钮的 action 经它派发。
    pub dispatch: FocusHandle,
}

/// 编辑器标签。远程文件经打开它的 SFTP 标签读写（`sftp`），本地文件不属于任何 SFTP 标签。
pub struct EditorTab {
    location: EditorLocation,
    /// 文件名：标签上的名称。
    name: SharedString,
    place: SharedString,
    sftp: Option<WeakEntity<SftpTab>>,
    local_files: SharedLocalDirectoryProvider,
    language: &'static str,
    state: Entity<EditorState>,
    /// 上次读到或存下的文字：有没有未保存的修改就比它。
    saved: Rope,
    format: TextFormat,
    /// 文件读到或存下时的样子；保存写到一半断了就是 `None`：文件已经不是读到的那样，下次不核对。
    stamp: Option<FileStamp>,
    unsaved: bool,
    phase: Phase,
    /// 这次保存成功后关掉标签（关闭时选了「保存」）。
    close_after_save: bool,
    cursor: Position,
    /// 开着的冲突框或者关闭确认，不再叠一个。
    dialog: Option<DialogLayer>,
    /// 开着的查找浮卡。
    find: Option<find::EditorFind>,
    /// 上次查的：关了浮卡再开（没选着文字时）接着查它。
    last_query: SharedString,
    dispatch: FocusHandle,
    _task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<EditorTabEvent> for EditorTab {}

impl EditorTab {
    pub fn new(options: EditorOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let EditorOptions {
            location,
            place,
            sftp,
            local_files,
            file,
            dispatch,
        } = options;
        let name = location.name();
        let language = language_for(&name);
        let (text, format, stamp) = file.into_parts();
        let tab_size = indentation(language, &text);
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .tab_size(tab_size)
                .default_value(text)
        });
        let saved = state.read(cx).text().clone();
        let subscriptions = vec![
            cx.subscribe(&state, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.update_unsaved(cx);
                    this.recount(cx);
                }
            }),
            // 光标动了，文字没变。
            cx.observe(&state, |this, state, cx| {
                let cursor = state.read(cx).cursor_position();
                if cursor != this.cursor {
                    this.cursor = cursor;
                    cx.notify();
                }
            }),
        ];
        Self {
            location,
            name: name.into(),
            place,
            sftp,
            local_files,
            language,
            state,
            saved,
            format,
            stamp: Some(stamp),
            unsaved: false,
            phase: Phase::Idle,
            close_after_save: false,
            cursor: Position::default(),
            dialog: None,
            find: None,
            last_query: SharedString::default(),
            dispatch,
            _task: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn location(&self) -> &EditorLocation {
        &self.location
    }

    /// 打开它的 SFTP 标签：本地文件是 `None`。
    pub fn sftp(&self) -> Option<&WeakEntity<SftpTab>> {
        self.sftp.as_ref()
    }

    /// 有没有未保存的修改：标签上的橙点，关闭时要问。
    pub fn has_unsaved_changes(&self) -> bool {
        self.unsaved
    }

    /// 正在写回：写回是截断重写，写完之前不能关，也不能退出。
    pub fn is_saving(&self) -> bool {
        self.phase == Phase::Saving
    }

    /// 文件名。
    pub fn file_name(&self) -> SharedString {
        self.name.clone()
    }

    /// 编辑器的文字，测试用。
    pub fn text(&self, cx: &App) -> String {
        self.state.read(cx).value().to_string()
    }

    pub(crate) fn rail_name(&self) -> SharedString {
        self.name.clone()
    }

    pub(crate) fn rail_directory(&self) -> SharedString {
        self.place.clone()
    }

    pub(crate) fn rail_tile(&self) -> Program {
        crate::programs::editor_program(&self.name)
    }

    pub(crate) fn rail_dot(&self) -> StatusDot {
        if self.unsaved {
            StatusDot::Attention
        } else {
            StatusDot::None
        }
    }

    /// 标签到前面来时焦点给文字。
    pub(crate) fn focus_target(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }

    fn update_unsaved(&mut self, cx: &mut Context<Self>) {
        let unsaved = {
            let text = self.state.read(cx).text();
            text.len() != self.saved.len() || *text != self.saved
        };
        if unsaved != self.unsaved {
            self.unsaved = unsaved;
            cx.emit(EditorTabEvent::Changed);
        }
        cx.notify();
    }

    /// Ctrl+S、头部的「保存」。
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.write(false, window, cx);
    }

    /// 关闭确认选了「保存」：存好了再关，存不成标签留着。
    fn save_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.unsaved {
            cx.emit(EditorTabEvent::CloseRequested);
            return;
        }
        self.close_after_save = true;
        self.write(false, window, cx);
    }

    /// 把文字写回去；`overwrite` 不核对别人改过没有。写的时候还能打字，存的是这一刻的文字。
    fn write(&mut self, overwrite: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.phase != Phase::Idle || (!self.unsaved && !overwrite) {
            return;
        }
        let snapshot = self.state.read(cx).text().clone();
        let bytes = self.format.encode(&snapshot.to_string());
        let expected = if overwrite { None } else { self.stamp };
        let write = match &self.location {
            EditorLocation::Remote(path) => {
                let path = path.clone();
                let task = self
                    .sftp
                    .as_ref()
                    .and_then(WeakEntity::upgrade)
                    .and_then(|sftp| {
                        sftp.update(cx, |sftp, cx| {
                            sftp.write_remote_file(path, bytes, expected, window, cx)
                        })
                    });
                match task {
                    Some(task) => task,
                    // 没连着：SFTP 标签已经说了。
                    None => {
                        self.close_after_save = false;
                        return;
                    }
                }
            }
            EditorLocation::Local(path) => {
                let (provider, path) = (self.local_files.clone(), path.clone());
                cx.background_spawn(async move {
                    write_local_text(provider.as_ref(), &path, &bytes, expected)
                })
            }
        };
        self.phase = Phase::Saving;
        cx.notify();
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = write.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_save(snapshot, result, window, cx)
            });
        }));
    }

    fn finish_save(
        &mut self,
        snapshot: Rope,
        result: Result<FileStamp, SaveFailure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.phase = Phase::Idle;
        let close = std::mem::take(&mut self.close_after_save);
        let name = self.name.clone();
        let saved = result.is_ok();
        match result {
            Ok(stamp) => {
                self.stamp = Some(stamp);
                self.saved = snapshot;
                self.update_unsaved(cx);
                cx.emit(EditorTabEvent::Saved);
                if close && !self.unsaved {
                    cx.emit(EditorTabEvent::CloseRequested);
                }
            }
            Err(SaveFailure::Changed { now }) => self.ask_about_conflict(now, close, window, cx),
            Err(SaveFailure::Interrupted(message)) => {
                // 磁盘上的既不是读到的，也不是想存的。
                self.stamp = None;
                window.push_notification(
                    Notification::error(t!("editor.save.interrupted", message = message))
                        .title(t!("editor.save.interrupted_title", name = name)),
                    cx,
                );
            }
            Err(SaveFailure::Failed(message)) => window.push_notification(
                Notification::error(message).title(t!("editor.save.failed", name = name)),
                cx,
            ),
        }
        cx.emit(EditorTabEvent::SaveEnded { saved });
        cx.notify();
    }

    /// 保存时发现文件被别人改过（原型的冲突框）：重新加载、取消或者覆盖。
    fn ask_about_conflict(
        &mut self,
        now: Option<FileStamp>,
        close: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog_open(window, cx) {
            return;
        }
        let remote = self.location.is_remote();
        let lead = if remote {
            t!("editor.changed.lead_remote")
        } else {
            t!("editor.changed.lead_local")
        };
        let mut lines = vec![ConfirmLine::Text(lead)];
        lines.extend(self.stamp.map(|stamp| ConfirmLine::Detail {
            label: t!("editor.changed.opened"),
            value: stamp_label(stamp).into(),
        }));
        lines.push(ConfirmLine::Detail {
            label: t!("editor.changed.now"),
            value: now
                .map(|stamp| SharedString::from(stamp_label(stamp)))
                .unwrap_or_else(|| t!("editor.changed.gone")),
        });
        lines.push(ConfirmLine::Text(if now.is_some() {
            t!("editor.changed.warning")
        } else {
            t!("editor.changed.warning_gone")
        }));
        // 文件没了就没什么可重新加载的。
        let mut buttons = Vec::new();
        if now.is_some() {
            buttons.push(ChoiceButton::new("reload", t!("editor.changed.reload")).left());
        }
        buttons.push(ChoiceButton::new("cancel", t!("editor.changed.cancel")));
        buttons.push(
            ChoiceButton::new("overwrite", t!("editor.changed.overwrite")).tone(ChoiceTone::Danger),
        );
        let layer = DialogLayer::new();
        self.dialog = Some(layer.clone());
        let (editor, focus) = (cx.weak_entity(), self.focus_target(cx));
        open_choice_dialog(
            Choice {
                id: "editor-changed",
                title: if remote {
                    t!("editor.changed.title_remote")
                } else {
                    t!("editor.changed.title_local")
                },
                subtitle: Some(self.name.clone()),
                lines,
                check: None,
                buttons,
            },
            layer,
            Box::new(move |outcome, window, cx| {
                window.focus(&focus, cx);
                let _ = editor.update(cx, |editor, cx| match outcome.button {
                    Some("overwrite") => {
                        editor.close_after_save = close;
                        editor.write(true, window, cx);
                    }
                    Some("reload") => editor.reload(window, cx),
                    _ => {}
                });
            }),
            window,
            cx,
        );
    }

    /// 头部的「重新加载」：有未保存的修改先问。
    pub fn request_reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.unsaved {
            self.reload(window, cx);
            return;
        }
        if self.dialog_open(window, cx) {
            return;
        }
        let layer = DialogLayer::new();
        self.dialog = Some(layer.clone());
        let (editor, focus) = (cx.weak_entity(), self.focus_target(cx));
        let cancel_focus = focus.clone();
        open_confirm_dialog(
            Confirm {
                id: "editor-reload",
                title: t!("editor.reload.title", name = self.name),
                lines: vec![ConfirmLine::Text(t!("editor.reload.description"))],
                cancel: t!("editor.reload.cancel"),
                confirm: t!("editor.reload.confirm"),
            },
            layer,
            Box::new(move |_, window, cx| {
                window.focus(&focus, cx);
                let _ = editor.update(cx, |editor, cx| editor.reload(window, cx));
                None
            }),
            Box::new(move |window, cx| window.focus(&cancel_focus, cx)),
            window,
            cx,
        );
    }

    /// 重读文件，有修改也丢掉。
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.phase != Phase::Idle {
            return;
        }
        let read = match &self.location {
            EditorLocation::Remote(_) => {
                let location = self.location.clone();
                let task = self
                    .sftp
                    .as_ref()
                    .and_then(WeakEntity::upgrade)
                    .and_then(|sftp| {
                        sftp.update(cx, |sftp, cx| sftp.read_file(&location, window, cx))
                    });
                match task {
                    Some(task) => task,
                    None => return,
                }
            }
            EditorLocation::Local(path) => {
                let (provider, path) = (self.local_files.clone(), path.clone());
                cx.background_spawn(async move { read_local_text(provider.as_ref(), &path) })
            }
        };
        self.phase = Phase::Reloading;
        cx.notify();
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_reload(result, window, cx)
            });
        }));
    }

    fn finish_reload(
        &mut self,
        result: Result<TextFile, ReadFailure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.phase = Phase::Idle;
        match result {
            Ok(file) => {
                let (text, format, stamp) = file.into_parts();
                let line = self.cursor.line;
                self.state.update(cx, |state, cx| {
                    state.set_value(text, window, cx);
                    // 回到光标原来那一行，新文字没那么长就到最后一行。
                    let last = state.text().lines_len().saturating_sub(1) as u32;
                    state.set_cursor_position(Position::new(line.min(last), 0), window, cx);
                });
                self.saved = self.state.read(cx).text().clone();
                self.format = format;
                self.stamp = Some(stamp);
                self.update_unsaved(cx);
            }
            Err(failure) => window.push_notification(
                Notification::error(describe_read_failure(&failure))
                    .title(t!("editor.reload.failed", name = self.name)),
                cx,
            ),
        }
        cx.notify();
    }

    /// 关闭前问：保存、不保存，还是留着标签。
    pub fn confirm_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_open(window, cx) {
            return;
        }
        let layer = DialogLayer::new();
        self.dialog = Some(layer.clone());
        let (editor, focus) = (cx.weak_entity(), self.focus_target(cx));
        open_choice_dialog(
            Choice {
                id: "editor-close",
                title: t!("editor.close.title"),
                subtitle: Some(self.name.clone()),
                lines: vec![ConfirmLine::Text(t!("editor.close.description"))],
                check: None,
                buttons: vec![
                    ChoiceButton::new("discard", t!("editor.close.discard")).left(),
                    ChoiceButton::new("cancel", t!("editor.close.cancel")),
                    ChoiceButton::new("save", t!("editor.close.save")).tone(ChoiceTone::Primary),
                ],
            },
            layer,
            Box::new(move |outcome, window, cx| {
                window.focus(&focus, cx);
                let _ = editor.update(cx, |editor, cx| match outcome.button {
                    Some("save") => editor.save_and_close(window, cx),
                    Some("discard") => cx.emit(EditorTabEvent::CloseRequested),
                    _ => {}
                });
            }),
            window,
            cx,
        );
    }

    fn dialog_open(&self, window: &Window, cx: &App) -> bool {
        self.dialog
            .as_ref()
            .is_some_and(|layer| layer.is_open(window, cx))
    }

    /// 状态栏（26 号 `.status`，和终端窗格同一种）：左边 主机（本地「本机」）/ 文件的完整路径，
    /// 有未保存的修改时一个橙点「未保存」（正在保存、重新加载时写正在做什么）；右边 光标位置 | 编码 |
    /// 换行 | 语言，最右一个「重新加载」小图标。
    fn render_status(&self, tokens: &Tokens, scale: TypeScale) -> impl IntoElement + use<> {
        let state: Option<SharedString> = match self.phase {
            Phase::Saving => Some(t!("editor.status.saving")),
            Phase::Reloading => Some(t!("editor.status.reloading")),
            Phase::Idle => self.unsaved.then(|| t!("editor.status.unsaved")),
        };
        let path = self.location.path();
        let right: [SharedString; 4] = [
            cursor_label(self.cursor.line, self.cursor.character),
            encoding_label(self.format).into(),
            line_end_label(self.format).into(),
            language_label(self.language),
        ];
        let label = std::iter::once(format!("{} / {path}", self.place))
            .chain(state.iter().map(ToString::to_string))
            .chain(right.iter().map(ToString::to_string))
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
        let reload_tip = if self.location.is_remote() {
            t!("editor.command.reload_remote")
        } else {
            t!("editor.command.reload_local")
        };
        // 和终端状态栏同一种分段：段与段之间是贯通整条高度的 1px 竖线，每段左右各留 7。
        let segment = || {
            div()
                .flex()
                .items_center()
                .h_full()
                .px(px(SEGMENT_PAD))
        };
        div()
            .id("editor-status")
            .test_support()
            .aria_label(label)
            .flex()
            .flex_none()
            .items_center()
            .h(size::STATUS_BAR)
            // 和终端状态栏一样从内容区左边开始，段自己留内边距；右边留 4 给重新加载按钮。
            .pr(space::SPACE_4)
            .border_t(size::LINE)
            .border_color(tokens.line)
            .bg(tokens.tabbar)
            .text_size(scale.sm)
            .text_color(tokens.text2)
            .whitespace_nowrap()
            .overflow_hidden()
            // 左边：主机名 | 路径。没有图标，和终端状态栏一样用竖线分开。
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .h_full()
                    .child(segment().flex_none().child(self.place.clone()))
                    .child(
                        segment()
                            .min_w_0()
                            .border_l(size::LINE)
                            .border_color(tokens.line)
                            .child(div().min_w_0().truncate().child(path)),
                    ),
            )
            .when_some(state, |bar, state| {
                bar.child(
                    segment()
                        .flex_none()
                        .gap(space::SPACE_6)
                        .border_l(size::LINE)
                        .border_color(tokens.line)
                        .when(self.unsaved && self.phase == Phase::Idle, |group| {
                            group.child(crate::ui::dot(tokens.warn, size::DOT))
                        })
                        .child(state),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h_full()
                    .children(right.into_iter().enumerate().map(|(ix, part)| {
                        segment()
                            .when(ix > 0, |seg| {
                                seg.border_l(size::LINE).border_color(tokens.line)
                            })
                            .child(part)
                            .into_any_element()
                    }))
                    .child(
                        segment().flex_none().child(
                            IconButton::new(
                                "editor-reload",
                                Icon::new(CatalogIcon::RefreshCw),
                                reload_tip.clone(),
                            )
                            .sm()
                            .tooltip(reload_tip)
                            .dimmed(self.phase != Phase::Idle)
                            .dispatch(Box::new(ReloadEditor), &self.dispatch, tokens),
                        ),
                    ),
            )
    }

    /// 只给这一个编辑器换行号的颜色（26 号：行号 `--faint`、当前行号 `--text2`），并去掉缩进参考线
    /// （26 号没有；它只用 `border` 画参考线，设成透明就不画了）。gpui-component
    /// 拿主题的次要字色、正文色画行号，而这两个颜色全应用都在用，不能改；它每次画都把样式重设一遍，
    /// 所以放一块画布在编辑器前面，等它设完、画之前再换一次。其余照 gpui-component `Input::render`。
    fn override_line_numbers(&self, tokens: &Tokens) -> impl IntoElement + use<> {
        let state = self.state.clone();
        let (line_number, current_line_number) = (tokens.faint, tokens.text2);
        canvas(
            move |_, _, cx| {
                let theme = cx.theme();
                let highlight = theme.highlight_theme.clone();
                let style = InputEditorStyle {
                    foreground: current_line_number,
                    muted_foreground: line_number,
                    background: highlight
                        .style
                        .editor_background
                        .unwrap_or(theme.background),
                    border: transparent_black(),
                    selection: theme.selection,
                    caret: theme.caret,
                    diagnostics: DiagnosticColors {
                        error: highlight.style.status.error(cx),
                        warning: highlight.style.status.warning(cx),
                        info: highlight.style.status.info(cx),
                        hint: highlight.style.status.hint(cx),
                    },
                    editor_invisible: highlight.style.editor_invisible,
                    editor_active_line: highlight.style.editor_active_line,
                    editor_gutter_background: highlight.style.editor_gutter_background,
                    highlight_styles: highlight,
                    fold_icon_renderer: None,
                };
                state.update(cx, |state, _| state.set_editor_style(style));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }
}

/// 冲突框里文件的样子：「1.2 KB · 2026/11/3 9:58」。
fn stamp_label(stamp: FileStamp) -> String {
    let size = format_size(stamp.size());
    let modified = stamp
        .modified()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| format_changed(elapsed.as_secs() as i64));
    match modified {
        Some(modified) => format!("{size} \u{b7} {modified}"),
        None => size,
    }
}

impl Render for EditorTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        // 和终端同一种字体、字号、前景色。
        let font = TerminalFont::current(cx);
        let family = font.family(cx);
        let foreground = TerminalColors::current(cx).foreground();
        let root = div()
            .id("editor-tab")
            .key_context(EDITOR_CONTEXT)
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(tokens.content);
        self.find_actions(root, cx)
            .child(
                div()
                    .id("editor-text")
                    .test_support()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .pt(CODE_TOP)
                    .child(self.override_line_numbers(&tokens))
                    .child(
                        Editor::new(&self.state)
                            .appearance(false)
                            .bordered(false)
                            .h_full()
                            .font_family(family.clone())
                            .text_size(font.size)
                            .line_height(relative(LINE_HEIGHT))
                            .text_color(foreground)
                            .readonly(self.phase == Phase::Reloading)
                            .aria_label(t!("editor.text_label", name = self.name)),
                    )
                    .children(self.render_matches((family, font.size)))
                    .children(self.render_find(cx)),
            )
            .child(self.render_status(&tokens, scale))
            .test_support()
    }
}

#[cfg(test)]
mod tests {
    use super::{EditorLocation, stamp_label};
    use crate::backend::sftp::{FileStamp, RemotePath};
    use std::path::PathBuf;

    #[test]
    fn a_location_names_its_file_and_directory_as_a_card_does() {
        let remote = EditorLocation::Remote(RemotePath::new("/etc/nginx/nginx.conf").unwrap());
        assert_eq!(remote.name(), "nginx.conf");
        assert_eq!(remote.directory(), "/etc/nginx");
        let top = EditorLocation::Remote(RemotePath::new("/hosts").unwrap());
        assert_eq!(top.directory(), "/");
        let local = EditorLocation::Local(PathBuf::from("/Users/me/notes.md"));
        assert_eq!(local.name(), "notes.md");
        assert_eq!(local.directory(), "/Users/me");
        assert_eq!(local.path(), "/Users/me/notes.md");
    }

    #[test]
    fn a_stamp_without_a_time_is_only_its_size() {
        assert_eq!(stamp_label(FileStamp::new(2048, None)), "2.0 KB");
    }
}
