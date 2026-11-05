//! 命令片段对话框（原型 `snipfull` 里的「编辑命令片段」）：名称、命令（等宽、可以多行）、
//! 「点击时自动执行」。新建和编辑共用；保存时才校验，校验过了才写进存储。
// 参考 since2006/shell-rs @ b2a307a: src/snippets/snippet_dialog.rs（去掉分类和作用域；
// 校验错误写在字段下面，不弹通知）。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::*;

use crate::host::{HostStore, SnippetDraft, SnippetDraftError, SnippetId};
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::DialogLayer;
use crate::ui::confirm::{Confirm, ConfirmLine, open_confirm_dialog};
use crate::ui::form::{FieldNote, form_field};

/// 对话框宽约 500（spec 第 4 节「编辑表单」）。
const DIALOG_WIDTH: Pixels = px(500.);

/// 对话框的表单。保存时校验，过了才碰存储。
pub struct SnippetForm {
    store: Entity<HostStore>,
    /// 编辑的是哪个片段；新建时是 `None`。
    editing: Option<SnippetId>,
    name: Entity<InputState>,
    command: Entity<TextareaState>,
    run_on_click: bool,
    /// 上一次保存为什么没过。
    error: Option<SnippetDraftError>,
}

impl SnippetForm {
    fn new(
        editing: Option<SnippetId>,
        store: Entity<HostStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let draft = editing
            .and_then(|id| store.read(cx).snippet(id).map(|snippet| snippet.draft()))
            // 新建的片段默认只输入：执行一条还没看过的命令要用户自己勾。
            .unwrap_or_else(|| SnippetDraft::new("", ""));
        let editing = editing.filter(|id| store.read(cx).snippet(*id).is_some());
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("snippets.dialog.name_placeholder"))
                .default_value(draft.name.clone())
        });
        let command = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 10)
                .placeholder(t!("snippets.dialog.command_placeholder"))
                .default_value(draft.command.clone())
        });
        Self {
            store,
            editing,
            name,
            command,
            run_on_click: draft.run_on_click,
            error: None,
        }
    }

    /// 校验并写进存储。返回对话框能不能关。
    fn commit(&mut self, cx: &mut Context<Self>) -> bool {
        let draft = match SnippetDraft::validated(
            &self.name.read(cx).value(),
            &self.command.read(cx).value(),
        ) {
            Ok(draft) => draft.with_run_on_click(self.run_on_click),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return false;
            }
        };
        let editing = self.editing;
        self.store.update(cx, |store, cx| match editing {
            Some(id) => {
                store.update_snippet(id, draft, cx);
            }
            None => {
                store.insert_snippet(draft, cx);
            }
        });
        true
    }
}

impl Render for SnippetForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        // 两个字段的错误共用一个元素 id `snippet-form-error`。
        let error = |field: SnippetDraftError| -> Vec<FieldNote> {
            (self.error == Some(field))
                .then(|| FieldNote::Error {
                    field: "snippet-form",
                    message: match field {
                        SnippetDraftError::Name => t!("snippets.dialog.name_required"),
                        SnippetDraftError::Command => t!("snippets.dialog.command_required"),
                    },
                })
                .into_iter()
                .collect()
        };
        div()
            .id("snippet-form")
            .flex()
            .flex_col()
            .text_size(px(12.5))
            .child(form_field(
                Some(t!("snippets.dialog.name")),
                Input::new(&self.name).id("snippet-name").small(),
                error(SnippetDraftError::Name),
                &tokens,
            ))
            .child(form_field(
                Some(t!("snippets.dialog.command")),
                div().id("snippet-command").test_support().w_full().child(
                    Textarea::new(&self.command)
                        .text_size(px(12.5))
                        .font_family(cx.theme().mono_font_family.clone()),
                ),
                error(SnippetDraftError::Command),
                &tokens,
            ))
            .child(form_field(
                None,
                Checkbox::new("snippet-run-on-click")
                    .label(t!("snippets.dialog.run_on_click"))
                    .checked(self.run_on_click)
                    .small()
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.run_on_click = *checked;
                        cx.notify();
                    })),
                Vec::new(),
                &tokens,
            ))
    }
}

/// 打开新建（`editing` 为 `None`）或编辑片段的对话框。
pub fn open_snippet_dialog(
    editing: Option<SnippetId>,
    store: Entity<HostStore>,
    window: &mut Window,
    cx: &mut App,
) {
    let form = cx.new(|cx| SnippetForm::new(editing, store, window, cx));
    let (title, subtitle) = match form.read(cx).editing {
        Some(id) => (
            t!("snippets.dialog.edit_title"),
            form.read(cx)
                .store
                .read(cx)
                .snippet(id)
                .map(|snippet| snippet.name.clone()),
        ),
        None => (t!("snippets.dialog.new_title"), None),
    };
    let tokens = Tokens::current(cx);
    DialogLayer::new().open(
        {
            let form = form.clone();
            move |dialog, _, _| {
                let commit = form.clone();
                dialog
                    .title(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(8.))
                            .child(title.clone())
                            .children(subtitle.clone().map(|name| {
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(tokens.faint)
                                    .child(name)
                            })),
                    )
                    .w(DIALOG_WIDTH)
                    .rounded(px(14.))
                    // 只由按钮和 Esc 关，点旁边不关：免得写了一半的命令丢掉。
                    .overlay_closable(false)
                    .child(form.clone())
                    .footer(
                        DialogFooter::new()
                            .child(
                                DialogClose::new()
                                    .trigger(|button| button.label(t!("snippets.dialog.cancel"))),
                            )
                            .child(
                                DialogAction::new().child(
                                    Button::new("snippet-save")
                                        .primary()
                                        .label(t!("snippets.dialog.save")),
                                ),
                            ),
                    )
                    .on_ok(move |_, _, cx| commit.update(cx, |form, cx| form.commit(cx)))
            }
        },
        window,
        cx,
    );
    // 打开就能打字：要在打开的同一次更新里给焦点，否则对话框自己的焦点会抢过去。
    let name = form.read(cx).name.clone();
    name.update(cx, |name, cx| name.focus(window, cx));
}

/// 删除前确认。确认了才删。
pub fn confirm_delete_snippet(
    id: SnippetId,
    store: Entity<HostStore>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(name) = store
        .read(cx)
        .snippet(id)
        .map(|snippet| snippet.name.clone())
    else {
        return;
    };
    open_confirm_dialog(
        Confirm {
            id: "snippet-delete",
            title: t!("snippets.delete.title", name = name),
            lines: vec![ConfirmLine::Text(t!("snippets.delete.description"))],
            cancel: t!("snippets.dialog.cancel"),
            confirm: t!("snippets.delete.ok"),
        },
        DialogLayer::new(),
        Box::new(move |_, _, cx| {
            store.update(cx, |store, cx| {
                store.remove_snippet(id, cx);
            });
            None
        }),
        Box::new(|_, _| {}),
        window,
        cx,
    );
}
