//! SFTP 标签（工单 20 起）。这一步只是占位，后面的提交接上内容。

use gpui_kit::*;

use crate::card::StatusDot;
use crate::programs::Program;

/// SFTP 标签。
pub struct SftpTab {
    focus_handle: FocusHandle,
}

impl SftpTab {
    pub(crate) fn rail_name(&self) -> SharedString {
        SharedString::default()
    }

    pub(crate) fn rail_directory(&self, _: &App) -> SharedString {
        SharedString::default()
    }

    pub(crate) fn rail_tile(&self) -> Program {
        crate::programs::sftp_program()
    }

    pub(crate) fn rail_dot(&self) -> StatusDot {
        StatusDot::None
    }

    pub(crate) fn focus_target(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SftpTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus_handle)
    }
}
