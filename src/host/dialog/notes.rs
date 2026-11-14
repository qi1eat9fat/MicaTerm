//! 主机表单的备注：一行（26 号，用户定：备注单行，所以在表单里按 Enter 就是保存），悬停主机时显示。

use gpui_kit::component::input::InputState;
use gpui_kit::*;

use micaterm_hosts::HostDraft;

use super::HostDialog;
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::form::{FieldErrors, form_field, form_group};
use crate::ui::modal::{ModalInputKind, modal_input};

pub const FIELD_NOTES: &str = "host-notes";

pub(super) struct NotesSection {
    notes: Entity<InputState>,
}

impl NotesSection {
    pub(super) fn new(
        draft: &HostDraft,
        window: &mut Window,
        cx: &mut Context<HostDialog>,
    ) -> Self {
        Self {
            // 以前存的多行备注合成一行。
            notes: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("host.dialog.notes_placeholder"))
                    .default_value(draft.notes.replace(['\r', '\n'], " "))
            }),
        }
    }

    pub(super) fn fill(&self, draft: &mut HostDraft, _: &mut FieldErrors, cx: &App) {
        draft.notes = self.notes.read(cx).value();
    }

    pub(super) fn render(
        &self,
        _: &FieldErrors,
        tokens: &Tokens,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let label = t!("host.dialog.notes");
        form_group([form_field(
            Some(label.clone()),
            modal_input(
                FIELD_NOTES,
                label,
                &self.notes,
                ModalInputKind::Text,
                false,
                tokens,
                window,
                cx,
            ),
            Vec::new(),
            tokens,
        )])
    }
}
