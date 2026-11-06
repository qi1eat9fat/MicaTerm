//! 打开文件：编辑器（工单 23）和预览（工单 24）经 SFTP 标签自己的连接读写。
//!
//! 工单 20 只留好入口：`SftpTab::execute` 和 `on_event` 的分支都转调这里的函数，
//! `FileOpenState` 是 `SftpTab` 里归这两个工单的状态。
//! 上游参考 shell-rs 的 `src/explorer/file_edit.rs`、`preview.rs`。

use gpui_kit::*;

use super::SftpTab;
use crate::backend::sftp::{FileBytes, FileStamp, ReadFailure, SaveFailure, TextFile};

/// `SftpTab` 里归打开文件的状态：等工作线程回答的读写（按请求号）、正在读的文件。
#[derive(Default)]
pub struct FileOpenState {}

impl SftpTab {
    /// 用编辑器打开 `path`，没写就是光标所在的文件（F4、回车、双击）。
    pub(super) fn open_for_editing(
        &mut self,
        _remote: bool,
        _path: Option<String>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 预览 `path`，没写就是光标所在的文件。
    pub(super) fn open_for_preview(
        &mut self,
        _remote: bool,
        _path: Option<String>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 工作线程读完了文本文件（`SftpEvent::FileRead`）。
    pub(super) fn finish_file_read(
        &mut self,
        _request_id: u64,
        _result: Result<TextFile, ReadFailure>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 工作线程写完了（`SftpEvent::FileWritten`）。
    pub(super) fn finish_file_write(
        &mut self,
        _request_id: u64,
        _result: Result<FileStamp, SaveFailure>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 工作线程读完了预览要的字节（`SftpEvent::BytesRead`）。
    pub(super) fn finish_bytes_read(
        &mut self,
        _request_id: u64,
        _result: Result<FileBytes, ReadFailure>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}
