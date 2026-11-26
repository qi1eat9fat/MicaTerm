//! 命令片段对话框：名称、命令（等宽、可以多行）。新建和编辑共用；保存时才校验，校验过了才写进存储。
//! 样子是共用的 `ui::modal` 弹窗（26 号），字段照新建主机的写法：字段名在上、框在下；底部「取消 Esc」「保存 Enter」。
//! 名称框里回车就是保存，命令框里回车换行。
// 参考 since2006/shell-rs @ b2a307a: src/snippets/snippet_dialog.rs（去掉分类和作用域；
// 校验错误写在字段下面，不弹通知）。

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::*;

use crate::app::CatalogIcon;
use crate::host::{HostStore, SnippetDraft, SnippetDraftError, SnippetId};
use crate::i18n::t;
use crate::theme::Tokens;
use crate::theme::space;
use crate::ui::DialogLayer;
use crate::ui::confirm::{Confirm, ConfirmLine, open_confirm_dialog};
use crate::ui::form::{FieldNote, form_field, form_group};
use crate::ui::modal::{
    ModalButtonTone, ModalHead, ModalInputKind, OnClick, modal_button, modal_dialog, modal_input,
    modal_textarea, render_modal,
};

/// 两个字段的错误共用的元素 id 前缀：红字是 `snippet-form-error`。
const FORM_ERROR: &str = "snippet-form";

/// 片段对话框。保存时校验，过了才碰存储。
pub struct SnippetForm {
    store: Entity<HostStore>,
    /// 编辑的是哪个片段；新建时是 `None`。
    editing: Option<SnippetId>,
    /// 编辑时头部的淡字：片段原来的名字。
    editing_name: Option<SharedString>,
    name: Entity<InputState>,
    command: Entity<TextareaState>,
    /// 上一次保存为什么没过。
    error: Option<SnippetDraftError>,
    /// 对话框是哪一层：保存、取消时只关自己。
    layer: DialogLayer,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SnippetForm {
    fn new(
        editing: Option<SnippetId>,
        store: Entity<HostStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let snippet = editing.and_then(|id| store.read(cx).snippet(id).cloned());
        let draft = snippet
            .as_ref()
            .map(|snippet| snippet.draft())
            .unwrap_or_else(|| SnippetDraft::new("", ""));
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
        let subscriptions = vec![
            cx.subscribe_in(
                &name,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => this.clear_error(SnippetDraftError::Name, cx),
                    InputEvent::PressEnter { .. } => this.save(window, cx),
                    _ => {}
                },
            ),
            cx.subscribe(&command, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.clear_error(SnippetDraftError::Command, cx);
                    // 空与非空切换时刷新 `modal_textarea` 的占位/等宽字体。
                    cx.notify();
                }
            }),
        ];
        Self {
            store,
            editing: snippet.as_ref().map(|snippet| snippet.id),
            editing_name: snippet.map(|snippet| snippet.name),
            name,
            command,
            error: None,
            layer: DialogLayer::new(),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// 改了出错的那个字段：红字先收起来，保存时再查。
    fn clear_error(&mut self, field: SnippetDraftError, cx: &mut Context<Self>) {
        if self.error == Some(field) {
            self.error = None;
            cx.notify();
        }
    }

    /// 校验并写进存储，写好了关掉对话框；没过就把原因写在字段下面。
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = match SnippetDraft::validated(
            &self.name.read(cx).value(),
            &self.command.read(cx).value(),
        ) {
            Ok(draft) => draft,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
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
        self.layer.close(window, cx);
    }

    /// 字段下面的红字：只在这个字段出错时有。
    fn error_note(&self, field: SnippetDraftError) -> Vec<FieldNote> {
        (self.error == Some(field))
            .then(|| FieldNote::Error {
                field: FORM_ERROR,
                message: match field {
                    SnippetDraftError::Name => t!("snippets.dialog.name_required"),
                    SnippetDraftError::Command => t!("snippets.dialog.command_required"),
                },
            })
            .into_iter()
            .collect()
    }

    fn render_footer(&self, tokens: &Tokens, window: &Window, cx: &mut Context<Self>) -> Div {
        let layer = self.layer.clone();
        let cancel: OnClick = Box::new(move |_, window, cx| layer.close(window, cx));
        div()
            .flex()
            .gap(space::SPACE_6)
            .child(modal_button(
                "snippet-cancel",
                t!("snippets.dialog.cancel"),
                Some("Esc"),
                ModalButtonTone::Plain,
                Some(cancel),
                tokens,
                window,
                cx,
            ))
            .child(modal_button(
                "snippet-save",
                t!("snippets.dialog.save"),
                Some("Enter"),
                ModalButtonTone::Primary,
                Some(Box::new(
                    cx.listener(|this, _, window, cx| this.save(window, cx)),
                )),
                tokens,
                window,
                cx,
            ))
    }
}

impl Focusable for SnippetForm {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SnippetForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let head = ModalHead {
            icon: CatalogIcon::CodeXml,
            title: if self.editing.is_some() {
                t!("snippets.dialog.edit_title")
            } else {
                t!("snippets.dialog.new_title")
            },
            detail: self.editing_name.clone(),
        };
        let body = form_group([
            form_field(
                Some(t!("snippets.dialog.name")),
                modal_input(
                    "snippet-name",
                    t!("snippets.dialog.name"),
                    &self.name,
                    ModalInputKind::Text,
                    self.error == Some(SnippetDraftError::Name),
                    &tokens,
                    window,
                    cx,
                ),
                self.error_note(SnippetDraftError::Name),
                &tokens,
            ),
            form_field(
                Some(t!("snippets.dialog.command")),
                modal_textarea(
                    "snippet-command",
                    t!("snippets.dialog.command"),
                    &self.command,
                    self.error == Some(SnippetDraftError::Command),
                    &tokens,
                    window,
                    cx,
                ),
                self.error_note(SnippetDraftError::Command),
                &tokens,
            ),
        ]);
        let footer = self.render_footer(&tokens, window, cx);
        render_modal(Some(head), body, footer, &tokens, window, cx)
            .id("snippet-form")
            .test_support()
            .track_focus(&self.focus_handle)
    }
}

/// 打开新建（`editing` 为 `None`）或编辑片段的对话框。不点遮罩关闭（免得写了一半的命令丢掉），
/// 用「取消」或 Esc 关。
pub fn open_snippet_dialog(
    editing: Option<SnippetId>,
    store: Entity<HostStore>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| SnippetForm::new(editing, store, window, cx));
    let layer = view.read(cx).layer.clone();
    let name = view.read(cx).name.clone();
    layer.open(
        move |dialog, _, cx| {
            modal_dialog(dialog, &Tokens::current(cx))
                // 回车由名称框自己处理（保存）；命令框里回车是换行。gpui-component 的 `Confirm`
                // 不关框，否则没校验就关掉了。
                .on_ok(|_, _, _| false)
                .child(view.clone())
        },
        window,
        cx,
    );
    // 打开就能打字：要在打开的同一次更新里给焦点，否则对话框自己的焦点会抢过去。
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
