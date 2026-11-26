// 参考 since2006/shell-rs @ b2a307a: src/explorer/file_dialogs.rs 改写
// 改造：走 `DialogLayer`；校验错误写在字段下面，不弹通知（spec 第 4 节）；删除的确认框照原型 `.dlg.confirm`；
// 本地名称在 Windows 上多查一道非法字符。
//! 删除的确认框，和重命名、新建共用的名称框。名称框的样子是共用的 `ui::modal` 弹窗（26 号）：
//! 头部图标 + 「重命名」「新建文件夹」+ 淡字原名，一栏名称，底部「取消 Esc」「重命名 Enter」。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::operations::OperationSender;
use super::{CardSide, FileOperation, NewEntryKind, SftpTab};
use crate::app::CatalogIcon;
use crate::i18n::t;
use crate::theme::Tokens;
use crate::theme::space;
use crate::ui::DialogLayer;
use crate::ui::confirm::{Confirm, ConfirmLine, open_confirm_dialog};
use crate::ui::form::{FieldNote, form_field};
use crate::ui::modal::{
    ModalButtonTone, ModalHead, ModalInputKind, OnClick, modal_button, modal_dialog, modal_input,
    render_modal,
};

/// 名称框的输入框的元素 id；错误是 `entry-name-error`，确定按钮是 `entry-name-commit`。
const NAME_FIELD: &str = "entry-name";

/// Windows 不允许出现在文件名里的字符（`/` 另有文案）。
const WINDOWS_ILLEGAL: [char; 8] = ['\\', ':', '*', '?', '"', '<', '>', '|'];

/// 查一个要新建或改成的名字。`existing` 是目录里现有的名字（隐藏文件也算）；
/// 最后说了算的还是服务器或文件系统。
pub(super) fn validate_entry_name(
    name: &str,
    original: Option<&str>,
    existing: &[String],
    windows_names: bool,
) -> Result<(), SharedString> {
    if name.trim().is_empty() {
        return Err(t!("sftp_files.name.empty"));
    }
    if matches!(name, "." | "..") {
        return Err(t!("sftp_files.name.dots"));
    }
    if name.contains(['/', '\0']) {
        return Err(t!("sftp_files.name.slash"));
    }
    if windows_names && name.contains(WINDOWS_ILLEGAL) {
        return Err(t!("sftp_files.name.windows_chars"));
    }
    if original == Some(name) {
        return Err(t!("sftp_files.name.unchanged"));
    }
    if existing.iter().any(|existing| existing == name) {
        return Err(t!("sftp_files.name.exists", name = name));
    }
    Ok(())
}

impl NewEntryKind {
    fn title(self) -> SharedString {
        match self {
            NewEntryKind::Folder => t!("sftp_files.new.folder_title"),
            NewEntryKind::File => t!("sftp_files.new.file_title"),
        }
    }

    fn default_name(self) -> SharedString {
        match self {
            NewEntryKind::Folder => t!("sftp_files.new.folder_name"),
            NewEntryKind::File => t!("sftp_files.new.file_name"),
        }
    }
}

/// 名称框是做什么的。
#[derive(Clone)]
enum NameIntent {
    Rename(String),
    New(NewEntryKind),
}

struct NameForm {
    intent: NameIntent,
    existing: Vec<String>,
    windows_names: bool,
    input: Entity<InputState>,
    label: SharedString,
    head: ModalHead,
    commit: SharedString,
    error: Option<SharedString>,
    send: OperationSender,
    /// 对话框是哪一层：做了、取消时只关自己。
    layer: DialogLayer,
    /// 关了以后焦点回到文件列表。
    list: FocusHandle,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

impl NameForm {
    /// 校验：过了就是要做的操作，没过就写在字段下面。
    fn operation(&mut self, cx: &mut Context<Self>) -> Option<FileOperation> {
        let name = self.input.read(cx).value().to_string();
        let original = match &self.intent {
            NameIntent::Rename(from) => Some(from.as_str()),
            NameIntent::New(_) => None,
        };
        if let Err(error) = validate_entry_name(&name, original, &self.existing, self.windows_names)
        {
            self.error = Some(error);
            cx.notify();
            return None;
        }
        Some(match &self.intent {
            NameIntent::Rename(from) => FileOperation::Rename {
                from: from.clone(),
                to: name,
            },
            NameIntent::New(kind) => FileOperation::Create { kind: *kind, name },
        })
    }

    /// 「重命名」「新建」（回车也是）：过了就关掉对话框、发出操作。
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(operation) = self.operation(cx) {
            self.close(window, cx);
            self.send.send(operation, window, cx);
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layer.close(window, cx);
        window.focus(&self.list, cx);
    }
}

impl Focusable for NameForm {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for NameForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let notes = self
            .error
            .clone()
            .map(|message| FieldNote::Error {
                field: NAME_FIELD,
                message,
            })
            .into_iter()
            .collect();
        let body = form_field(
            Some(self.label.clone()),
            modal_input(
                NAME_FIELD,
                self.label.clone(),
                &self.input,
                ModalInputKind::Text,
                self.error.is_some(),
                &tokens,
                window,
                cx,
            ),
            notes,
            &tokens,
        );
        let cancel: OnClick = Box::new(cx.listener(|this, _, window, cx| this.close(window, cx)));
        let footer = div()
            .flex()
            .gap(space::SPACE_6)
            .child(modal_button(
                "entry-name-cancel",
                t!("sftp_files.cancel"),
                Some("Esc"),
                ModalButtonTone::Plain,
                Some(cancel),
                &tokens,
                window,
                cx,
            ))
            .child(modal_button(
                "entry-name-commit",
                self.commit.clone(),
                Some("Enter"),
                ModalButtonTone::Primary,
                Some(Box::new(
                    cx.listener(|this, _, window, cx| this.commit(window, cx)),
                )),
                &tokens,
                window,
                cx,
            ));
        render_modal(Some(self.head.clone()), body, footer, &tokens, window, cx)
            .id("entry-name-dialog")
            .test_support()
            .track_focus(&self.focus_handle)
    }
}

impl SftpTab {
    /// 问要不要删除这一侧选中的（Delete）。本地的进回收站，远程的就没了，框里写清楚。
    pub(super) fn confirm_delete(
        &mut self,
        side: CardSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let names = self.card(side).read(cx).selected_names(cx);
        if names.is_empty() || !self.can_start_operation(side, window, cx) {
            return;
        }
        let question = if let [name] = names.as_slice() {
            t!("sftp_files.delete.question_one", name = name)
        } else {
            t!("sftp_files.delete.question_many", count = names.len())
        };
        let consequence = if side.is_remote() {
            t!("sftp_files.delete.remote")
        } else if cfg!(windows) {
            t!("sftp_files.delete.recycle_bin")
        } else {
            t!("sftp_files.delete.trash")
        };
        let list = self.card(side).read(cx).focus_handle(cx);
        let list_after_cancel = list.clone();
        let send = self.operation_sender(side);
        let layer = DialogLayer::new();
        open_confirm_dialog(
            Confirm {
                id: "file-delete",
                title: t!("sftp_files.delete.title"),
                lines: vec![ConfirmLine::Text(question), ConfirmLine::Text(consequence)],
                cancel: t!("sftp_files.cancel"),
                confirm: t!("sftp_files.delete.confirm"),
            },
            layer.clone(),
            Box::new(move |_, window, cx| {
                send.send(FileOperation::Delete(names), window, cx);
                window.focus(&list, cx);
                None
            }),
            Box::new(move |window, cx| window.focus(&list_after_cancel, cx)),
            window,
            cx,
        );
        self.operations.dialog = Some(layer);
    }

    /// 重命名（F2）：只选中一项时。
    pub(super) fn open_rename(
        &mut self,
        side: CardSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let [name] = self.card(side).read(cx).selected_names(cx).as_slice() {
            let intent = NameIntent::Rename(name.clone());
            self.open_name_dialog(side, intent, window, cx);
        }
    }

    /// 新建文件夹或文件。
    pub(super) fn open_new(
        &mut self,
        side: CardSide,
        kind: NewEntryKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_name_dialog(side, NameIntent::New(kind), window, cx);
    }

    fn open_name_dialog(
        &mut self,
        side: CardSide,
        intent: NameIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_start_operation(side, window, cx) {
            return;
        }
        let card = self.card(side).read(cx);
        let existing: Vec<String> = card
            .listed_entries(cx)
            .iter()
            .filter(|entry| !entry.is_parent())
            .map(|entry| entry.name.to_string())
            .collect();
        let list = card.focus_handle(cx);
        let (icon, title, subtitle, initial, commit, label) = match &intent {
            NameIntent::Rename(name) => (
                CatalogIcon::Pencil,
                t!("sftp_files.rename.title"),
                Some(SharedString::from(name.clone())),
                SharedString::from(name.clone()),
                t!("sftp_files.rename.commit"),
                t!("sftp_files.rename.label"),
            ),
            NameIntent::New(kind) => (
                match kind {
                    NewEntryKind::Folder => CatalogIcon::FolderPlus,
                    NewEntryKind::File => CatalogIcon::FilePlus,
                },
                kind.title(),
                None,
                kind.default_name(),
                t!("sftp_files.new.commit"),
                t!("sftp_files.new.label"),
            ),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let layer = DialogLayer::new();
        let send = self.operation_sender(side);
        let form = cx.new(|cx| {
            // 改了字，上一次的错误就收起来，提交时再查；回车就是「重命名」「新建」。
            let subscription = cx.subscribe_in(
                &input,
                window,
                |form: &mut NameForm, _, event, window, cx| match event {
                    InputEvent::Change => {
                        if form.error.take().is_some() {
                            cx.notify();
                        }
                    }
                    InputEvent::PressEnter { .. } => form.commit(window, cx),
                    _ => {}
                },
            );
            NameForm {
                intent,
                existing,
                windows_names: !side.is_remote() && cfg!(windows),
                input: input.clone(),
                label,
                head: ModalHead {
                    icon,
                    title,
                    detail: subtitle,
                },
                commit,
                error: None,
                send,
                layer: layer.clone(),
                list: list.clone(),
                focus_handle: cx.focus_handle(),
                _subscription: subscription,
            }
        });
        layer.open_dismissable(
            move |dialog, _, cx| {
                modal_dialog(dialog, &Tokens::current(cx))
                    // 回车由名称框自己处理（校验过了才关）。
                    .on_ok(|_, _, _| false)
                    .child(form.clone())
            },
            // Esc 关掉的：焦点回到文件列表。
            move |window, cx| window.focus(&list, cx),
            window,
            cx,
        );
        // 打开就能打字，打字就替换：要在打开的同一次更新里给焦点，否则对话框自己的焦点会抢过去。
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        self.operations.dialog = Some(layer);
    }
}

#[cfg(test)]
mod tests {
    use super::validate_entry_name;

    #[test]
    fn names_are_checked_against_the_listing() {
        let existing = vec!["a.txt".to_string(), "目录".to_string()];
        let check = |name, original| validate_entry_name(name, original, &existing, false);
        assert!(check("b.txt", None).is_ok());
        assert_eq!(check("  ", None).unwrap_err(), "Name is required");
        assert!(check("..", None).is_err());
        assert!(check("a/b", None).is_err());
        assert_eq!(check("a.txt", Some("a.txt")).unwrap_err(), "The name did not change");
        assert_eq!(
            check("目录", Some("a.txt")).unwrap_err(),
            "An item named \"目录\" already exists"
        );
        // 只改大小写不算撞名。
        assert!(check("A.txt", Some("a.txt")).is_ok());
    }

    #[test]
    fn windows_characters_only_count_on_the_local_side_of_windows() {
        assert!(validate_entry_name(r"a\b", None, &[], false).is_ok());
        assert!(validate_entry_name("a:b", None, &[], false).is_ok());
        assert!(validate_entry_name(r"a\b", None, &[], true).is_err());
        assert!(validate_entry_name("a:b", None, &[], true).is_err());
        assert!(validate_entry_name("a b.txt", None, &[], true).is_ok());
    }
}
