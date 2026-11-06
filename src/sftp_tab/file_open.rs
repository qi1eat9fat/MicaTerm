//! 打开文件：编辑器（工单 23）和预览（工单 24）经 SFTP 标签自己的连接读写。
//!
//! 文件先读后开：打不开的（太大、不是文本）在这里说明原因，不会闪一下标签。
//! `SftpTab::execute` 和 `on_event` 的分支都转调这里的函数，`FileOpenState` 是 `SftpTab` 里归这两个工单的状态。
// 参考 since2006/shell-rs @ b2a307a: src/explorer/file_edit.rs
// 改造：对话框走 `DialogLayer`；文件在不在已开的编辑器里由窗口壳判断（`SftpTabEvent::EditFile`）。

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use futures::channel::oneshot;
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;

use super::preview::{IMAGE_LIMIT, Preview, PreviewContent, PreviewKind, open_preview_dialog};
use super::{ConnectionState, SftpTab, SftpTabEvent, format_size};
use crate::app::{SftpAction, SftpDispatch as _, SftpTabCommand};
use crate::backend::sftp::{
    EDIT_LIMIT, FileBytes, FileStamp, ReadFailure, RemotePath, SaveFailure, SftpCommand, TextFile,
    read_local_bytes, read_local_text,
};
use crate::editor::EditorLocation;
use crate::i18n::t;
use crate::ui::DialogLayer;
use crate::ui::confirm::{Choice, ChoiceButton, ChoiceTone, ConfirmLine, open_choice_dialog};

/// 等工作线程回答的读写，在 `finish_*` 里回给发起的人。
enum PendingFile {
    Read(oneshot::Sender<Result<TextFile, ReadFailure>>),
    Write(oneshot::Sender<Result<FileStamp, SaveFailure>>),
    Bytes(oneshot::Sender<Result<FileBytes, ReadFailure>>),
}

/// `SftpTab` 里归打开文件的状态：等工作线程回答的读写（按请求号）、正在读的文件、开着的预览。
#[derive(Default)]
pub struct FileOpenState {
    next_request: u64,
    pending: HashMap<u64, PendingFile>,
    opening: HashSet<EditorLocation>,
    preview: Option<DialogLayer>,
}

/// 读文件是为了什么：打不开时的说法不一样。
#[derive(Clone, Copy)]
enum Opening {
    Edit,
    Preview(PreviewKind),
}

impl SftpTab {
    /// 用编辑器打开 `path`，没写就是光标所在的文件（F4、回车、双击）。已经开着就只切过去，
    /// 所以先问窗口壳：它再调 `read_for_editor`。
    pub(super) fn open_for_editing(
        &mut self,
        remote: bool,
        path: Option<String>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(location) = self.edit_target(remote, path.as_deref(), cx) {
            cx.emit(SftpTabEvent::EditFile(location));
        }
    }

    /// `path` 说的文件；没写就是这一侧只选中的那一个（右键菜单对着的就是它），否则是光标所在的文件。
    /// 目录或者什么都没有是 `None`。
    fn edit_target(&self, remote: bool, path: Option<&str>, cx: &App) -> Option<EditorLocation> {
        let card = self.card(remote).read(cx);
        let path = match path {
            Some(path) => path.to_string(),
            None => {
                let selected = card.selected_entries(cx);
                let entry = match selected.as_slice() {
                    [only] => only.clone(),
                    _ => card.cursor_entry(cx)?,
                };
                if entry.is_dir() || entry.is_parent() {
                    return None;
                }
                card.child_path_of(&entry.name)
            }
        };
        Some(if remote {
            EditorLocation::Remote(RemotePath::new(path).ok()?)
        } else {
            EditorLocation::Local(path.into())
        })
    }

    /// 为新的编辑器标签读文件。读完发 `SftpTabEvent::FileRead`；打不开的在这里说明。
    pub fn read_for_editor(
        &mut self,
        location: EditorLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_open.opening.contains(&location) {
            return;
        }
        let Some(read) = self.read_file(&location, window, cx) else {
            return;
        };
        self.while_opening(
            location,
            read,
            window,
            cx,
            |this, location, result, window, cx| match result {
                Ok(file) => cx.emit(SftpTabEvent::FileRead {
                    location,
                    file: Rc::new(file),
                }),
                Err(failure) => this.refuse_open(&location, Opening::Edit, failure, window, cx),
            },
        );
    }

    /// 等 `read` 读完，这期间文件卡片显示正在打开，同一个文件不再读第二遍；读完交给 `done`。
    fn while_opening<T: 'static>(
        &mut self,
        location: EditorLocation,
        read: Task<Result<T, ReadFailure>>,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(
            &mut Self,
            EditorLocation,
            Result<T, ReadFailure>,
            &mut Window,
            &mut Context<Self>,
        ) + 'static,
    ) {
        self.file_open.opening.insert(location.clone());
        let remote = location.is_remote();
        let name = location.name();
        self.card(remote)
            .clone()
            .update(cx, |card, cx| card.begin_opening(name, cx));
        cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.file_open.opening.remove(&location);
                this.card(remote)
                    .clone()
                    .update(cx, |card, cx| card.finish_opening(cx));
                done(this, location, result, window, cx);
            });
        })
        .detach();
    }

    /// 读整个文本文件。远程没连着时是 `None`，这里已经告诉用户了。
    pub fn read_file(
        &mut self,
        location: &EditorLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<TextFile, ReadFailure>>> {
        match location {
            EditorLocation::Local(path) => {
                let (provider, path) = (self.local_files.clone(), path.clone());
                Some(cx.background_spawn(async move { read_local_text(provider.as_ref(), &path) }))
            }
            EditorLocation::Remote(path) => {
                if !self.reachable(window, cx) {
                    return None;
                }
                let (sender, receiver) = oneshot::channel();
                let request_id = self.next_file_request(PendingFile::Read(sender));
                self.send(SftpCommand::ReadFile {
                    request_id,
                    path: path.clone(),
                });
                Some(cx.spawn(async move |_, _| {
                    receiver.await.unwrap_or_else(|_| {
                        Err(ReadFailure::Failed(
                            t!("editor.file.connection_lost").to_string(),
                        ))
                    })
                }))
            }
        }
    }

    /// 读整个文件，不管是不是文本，比 `limit` 大就不读。远程没连着时是 `None`，这里已经告诉用户了。
    pub fn read_bytes(
        &mut self,
        location: &EditorLocation,
        limit: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<FileBytes, ReadFailure>>> {
        match location {
            EditorLocation::Local(path) => {
                let (provider, path) = (self.local_files.clone(), path.clone());
                Some(cx.background_spawn(async move {
                    read_local_bytes(provider.as_ref(), &path, limit)
                }))
            }
            EditorLocation::Remote(path) => {
                if !self.reachable(window, cx) {
                    return None;
                }
                let (sender, receiver) = oneshot::channel();
                let request_id = self.next_file_request(PendingFile::Bytes(sender));
                self.send(SftpCommand::ReadBytes {
                    request_id,
                    path: path.clone(),
                    limit,
                });
                Some(cx.spawn(async move |_, _| {
                    receiver.await.unwrap_or_else(|_| {
                        Err(ReadFailure::Failed(
                            t!("editor.file.connection_lost").to_string(),
                        ))
                    })
                }))
            }
        }
    }

    /// 把字节原地写回远程文件。没连着时是 `None`，这里已经告诉用户了。
    pub fn write_remote_file(
        &mut self,
        path: RemotePath,
        bytes: Vec<u8>,
        expected: Option<FileStamp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<FileStamp, SaveFailure>>> {
        if !self.reachable(window, cx) {
            return None;
        }
        let (sender, receiver) = oneshot::channel();
        let request_id = self.next_file_request(PendingFile::Write(sender));
        self.send(SftpCommand::WriteFile {
            request_id,
            path,
            bytes,
            expected,
        });
        Some(cx.spawn(async move |_, _| {
            // 回答之前连接断了：可能写了一半。
            receiver.await.unwrap_or_else(|_| {
                Err(SaveFailure::Interrupted(
                    t!("editor.file.connection_lost").to_string(),
                ))
            })
        }))
    }

    /// 这一侧正好显示着文件所在的目录就重读：大小和时间跟着保存变。
    pub fn reload_if_showing(
        &mut self,
        location: &EditorLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remote = location.is_remote();
        if remote && self.state != ConnectionState::Connected {
            return;
        }
        if self.card(remote).read(cx).path() == location.directory() {
            self.reload(remote, window, cx);
        }
    }

    /// 有写回在等工作线程回答。
    pub(super) fn writing_file(&self) -> bool {
        self.file_open
            .pending
            .values()
            .any(|pending| matches!(pending, PendingFile::Write(_)))
    }

    /// 连接断了：还在等的读写不会有回答了，发起的人各自收到失败。
    pub(super) fn abandon_file_requests(&mut self) {
        self.file_open.pending.clear();
    }

    /// 双击：图片预览，别的和回车一样（目录进去，文件编辑）。
    pub(super) fn open_double_clicked(
        &mut self,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let card = self.card(remote).read(cx);
        let picture = card
            .cursor_entry(cx)
            .filter(|entry| {
                !entry.is_dir()
                    && !entry.is_parent()
                    && PreviewKind::of(&entry.name).is_some_and(PreviewKind::preview_only)
            })
            .map(|entry| card.child_path_of(&entry.name));
        match picture {
            Some(path) => self.open_for_preview(remote, Some(path), window, cx),
            None => self.open_cursor_entry(remote, window, cx),
        }
    }

    /// 预览 `path`，没写就是这一侧只选中的那个文件或者光标所在的文件。读的总是服务器上或磁盘上的内容，
    /// 编辑器里没保存的修改不出现。预览不了的文件什么都不做（菜单上本来就没有这一项）。
    pub(super) fn open_for_preview(
        &mut self,
        remote: bool,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(location) = self.edit_target(remote, path.as_deref(), cx) else {
            return;
        };
        let Some(kind) = PreviewKind::of(&location.name()) else {
            return;
        };
        if self.file_open.opening.contains(&location) {
            return;
        }
        match kind {
            PreviewKind::Image(format) => {
                let Some(read) = self.read_bytes(&location, IMAGE_LIMIT, window, cx) else {
                    return;
                };
                self.while_opening(
                    location,
                    read,
                    window,
                    cx,
                    move |this, location, result, window, cx| match result {
                        Ok(bytes) => {
                            let size = bytes.len() as u64;
                            let image = Image::from_bytes(format, bytes.into_inner());
                            let content = PreviewContent::Image(image);
                            this.show_preview(&location, kind, size, content, window, cx);
                        }
                        Err(failure) => {
                            let opening = Opening::Preview(kind);
                            this.refuse_open(&location, opening, failure, window, cx);
                        }
                    },
                );
            }
            PreviewKind::Markdown => {
                let Some(read) = self.read_file(&location, window, cx) else {
                    return;
                };
                self.while_opening(
                    location,
                    read,
                    window,
                    cx,
                    move |this, location, result, window, cx| match result {
                        Ok(file) => {
                            let size = file.stamp().size();
                            let (text, _, _) = file.into_parts();
                            let content = PreviewContent::Markdown(text.into());
                            this.show_preview(&location, kind, size, content, window, cx);
                        }
                        Err(failure) => {
                            let opening = Opening::Preview(kind);
                            this.refuse_open(&location, opening, failure, window, cx);
                        }
                    },
                );
            }
        }
    }

    /// 文件读好了：打开预览。「下载…」只给远程文件，目标是本地一侧现在的目录；不带连接的代号，
    /// 连接换过也照样能下载。「编辑」给 Markdown 和 SVG。
    fn show_preview(
        &mut self,
        location: &EditorLocation,
        kind: PreviewKind,
        bytes: u64,
        content: PreviewContent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remote = location.is_remote();
        let tab = self.id();
        let download = remote.then(|| {
            SftpAction::new(
                tab,
                SftpTabCommand::DownloadPaths {
                    paths: vec![location.path()],
                    target: self.local().read(cx).path(),
                },
            )
        });
        let edit = (!kind.preview_only()).then(|| {
            SftpAction::new(
                tab,
                SftpTabCommand::Edit {
                    remote,
                    path: Some(location.path()),
                },
            )
        });
        let place = if remote {
            t!(
                "preview.place_remote",
                host = self.host().name,
                directory = location.directory()
            )
        } else {
            t!(
                "preview.place_local",
                local = t!("sftp_tab.local_name"),
                directory = location.directory()
            )
        };
        let layer = DialogLayer::new();
        self.close_preview(window, cx);
        open_preview_dialog(
            Preview {
                name: location.name().into(),
                place,
                bytes,
                content,
                download,
                edit,
                dispatch: self.dispatch.clone(),
            },
            layer.clone(),
            window,
            cx,
        );
        self.file_open.preview = Some(layer);
    }

    /// 关掉开着的预览（标签关了）。
    pub(super) fn close_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(layer) = self.file_open.preview.take() {
            layer.close(window, cx);
        }
    }

    /// 工作线程读完了文本文件（`SftpEvent::FileRead`）。
    pub(super) fn finish_file_read(
        &mut self,
        request_id: u64,
        result: Result<TextFile, ReadFailure>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if let Some(PendingFile::Read(sender)) = self.file_open.pending.remove(&request_id) {
            let _ = sender.send(result);
        }
    }

    /// 工作线程写完了（`SftpEvent::FileWritten`）。
    pub(super) fn finish_file_write(
        &mut self,
        request_id: u64,
        result: Result<FileStamp, SaveFailure>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if let Some(PendingFile::Write(sender)) = self.file_open.pending.remove(&request_id) {
            let _ = sender.send(result);
        }
    }

    /// 工作线程读完了预览要的字节（`SftpEvent::BytesRead`）。
    pub(super) fn finish_bytes_read(
        &mut self,
        request_id: u64,
        result: Result<FileBytes, ReadFailure>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if let Some(PendingFile::Bytes(sender)) = self.file_open.pending.remove(&request_id) {
            let _ = sender.send(result);
        }
    }

    /// 远程连着；否则说明原因（断开时问要不要重新连接）。
    fn reachable(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.state {
            ConnectionState::Connected => true,
            ConnectionState::Disconnected => {
                self.offer_reconnect(window, cx);
                false
            }
            ConnectionState::Connecting => {
                window.push_notification(Notification::warning(t!("editor.file.connecting")), cx);
                false
            }
        }
    }

    fn next_file_request(&mut self, pending: PendingFile) -> u64 {
        self.file_open.next_request += 1;
        let id = self.file_open.next_request;
        self.file_open.pending.insert(id, pending);
        id
    }

    /// 文件为什么没打开。太大、不是文本是要用户决定的（远程文件还能下载）；其余的是错误。
    fn refuse_open(
        &mut self,
        location: &EditorLocation,
        opening: Opening,
        failure: ReadFailure,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = location.name();
        if let ReadFailure::Failed(message) = &failure {
            window.push_notification(
                Notification::error(message.clone())
                    .title(t!("editor.file.open_failed", name = name)),
                cx,
            );
            return;
        }
        let reason = opening.refusal(&name, &failure);
        let (id, title) = match opening {
            Opening::Edit => ("editor-refused", t!("editor.refused.title")),
            Opening::Preview(_) => ("preview-refused", t!("preview.refused.title")),
        };
        // 本地文件本来就在这台电脑上，没有「下载…」。
        let download = match location {
            EditorLocation::Remote(path) => Some(SftpTabCommand::DownloadPaths {
                paths: vec![path.to_string()],
                target: self.local().read(cx).path(),
            }),
            EditorLocation::Local(_) => None,
        };
        let mut lines = vec![ConfirmLine::Text(reason)];
        let mut buttons = vec![ChoiceButton::new("close", t!("editor.refused.close"))];
        if download.is_some() {
            lines.push(ConfirmLine::Text(t!("editor.refused.download_hint")));
            buttons.push(
                ChoiceButton::new("download", t!("editor.refused.download"))
                    .tone(ChoiceTone::Primary),
            );
        }
        let (dispatch, tab, generation) = (self.dispatch.clone(), self.id(), self.generation());
        let focus = window.focused(cx);
        open_choice_dialog(
            Choice {
                id,
                title,
                subtitle: Some(name.into()),
                lines,
                check: None,
                buttons,
            },
            DialogLayer::new(),
            Box::new(move |outcome, window, cx| {
                if let Some(focus) = &focus {
                    window.focus(focus, cx);
                }
                if outcome.button == Some("download")
                    && let Some(command) = download
                {
                    dispatch.dispatch_sftp_action(
                        &SftpAction::new(tab, command).with_generation(generation),
                        window,
                        cx,
                    );
                }
            }),
            window,
            cx,
        );
    }
}

impl Opening {
    /// 文件为什么打不开，一句话。
    fn refusal(self, name: &str, failure: &ReadFailure) -> SharedString {
        let limit = format_size(EDIT_LIMIT);
        match (self, failure) {
            (Self::Edit, ReadFailure::TooLarge(size)) => t!(
                "editor.refused.too_large",
                name = name,
                size = format_size(*size),
                limit = limit
            ),
            (Self::Edit, ReadFailure::NotText) => {
                t!("editor.refused.not_text", name = name, limit = limit)
            }
            (Self::Edit, _) => t!("editor.refused.not_file", name = name),
            (Self::Preview(PreviewKind::Image(_)), ReadFailure::TooLarge(size)) => t!(
                "preview.refused.too_large_image",
                name = name,
                size = format_size(*size),
                limit = format_size(IMAGE_LIMIT)
            ),
            (Self::Preview(PreviewKind::Markdown), ReadFailure::TooLarge(size)) => t!(
                "preview.refused.too_large_markdown",
                name = name,
                size = format_size(*size),
                limit = limit
            ),
            (Self::Preview(_), ReadFailure::NotText) => {
                t!("preview.refused.not_text", name = name)
            }
            (Self::Preview(_), _) => t!("preview.refused.not_file", name = name),
        }
    }
}
