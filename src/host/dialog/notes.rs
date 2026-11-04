//! 主机对话框的「备注」组：写点什么，悬停主机时显示。

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::*;

use qishell_hosts::HostDraft;

use super::HostDialog;
use super::form::{FieldErrors, form_group};
use crate::i18n::t;
use crate::theme::Tokens;

pub const FIELD_NOTES: &str = "host-notes";

pub(super) struct NotesSection {
    notes: Entity<TextareaState>,
}

impl NotesSection {
    pub(super) fn new(draft: &HostDraft, window: &mut Window, cx: &mut Context<HostDialog>) -> Self {
        Self {
            notes: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 6)
                    .placeholder(t!("host.dialog.notes_placeholder"))
                    .default_value(draft.notes.clone())
            }),
        }
    }

    pub(super) fn fill(&self, draft: &mut HostDraft, _: &mut FieldErrors, cx: &App) {
        draft.notes = self.notes.read(cx).value();
    }

    pub(super) fn render(&self, _: &FieldErrors, tokens: &Tokens) -> AnyElement {
        form_group(
            t!("host.dialog.notes"),
            [div()
                .mb(px(10.))
                .child(
                    Textarea::new(&self.notes)
                        .id(FIELD_NOTES)
                        .aria_label(t!("host.dialog.notes"))
                        .small(),
                )
                .into_any_element()],
            tokens,
        )
    }
}
