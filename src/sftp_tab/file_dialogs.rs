// 参考 since2006/shell-rs @ b2a307a: src/explorer/file_dialogs.rs 改写
// 改造：走 `DialogLayer`；校验错误写在字段下面，不弹通知（spec 第 4 节）；删除的确认框照原型 `.dlg.confirm`；
// 本地名称在 Windows 上多查一道非法字符。
//! 删除的确认框，和重命名、新建共用的名称框。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::{FileOperation, NewEntryKind, SftpTab};
use crate::i18n::t;
use crate::theme::Tokens;
use crate::ui::DialogLayer;
use crate::ui::confirm::{Confirm, ConfirmLine, open_confirm_dialog};
use crate::ui::form::{FieldNote, dialog_footer, dialog_title, form_field, text_input};

/// 小对话框的宽（原型 `.dlg.small`）。
pub(super) const SMALL_DIALOG_WIDTH: Pixels = px(470.);

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
    error: Option<SharedString>,
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
}

impl Render for NameForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        div()
            .flex()
            .flex_col()
            .text_size(px(12.5))
            .child(form_field(
                Some(self.label.clone()),
                // 和左边 30 高的字段名对齐。
                div().flex().items_center().min_h(px(30.)).child(text_input(
                    NAME_FIELD,
                    self.label.clone(),
                    &self.input,
                    self.error.is_some(),
                    &tokens,
                )),
                notes,
                &tokens,
            ))
    }
}

impl SftpTab {
    /// 问要不要删除这一侧选中的（Delete）。本地的进回收站，远程的就没了，框里写清楚。
    pub(super) fn confirm_delete(
        &mut self,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let names = self.card(remote).read(cx).selected_names(cx);
        if names.is_empty() || !self.can_start_operation(remote, window, cx) {
            return;
        }
        let question = if let [name] = names.as_slice() {
            t!("sftp_files.delete.question_one", name = name)
        } else {
            t!("sftp_files.delete.question_many", count = names.len())
        };
        let consequence = if remote {
            t!("sftp_files.delete.remote")
        } else if cfg!(windows) {
            t!("sftp_files.delete.recycle_bin")
        } else {
            t!("sftp_files.delete.trash")
        };
        let list = self.card(remote).read(cx).focus_handle(cx);
        let list_after_cancel = list.clone();
        let send = self.operation_sender(remote);
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
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let [name] = self.card(remote).read(cx).selected_names(cx).as_slice() {
            let intent = NameIntent::Rename(name.clone());
            self.open_name_dialog(remote, intent, window, cx);
        }
    }

    /// 新建文件夹或文件。
    pub(super) fn open_new(
        &mut self,
        remote: bool,
        kind: NewEntryKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_name_dialog(remote, NameIntent::New(kind), window, cx);
    }

    fn open_name_dialog(
        &mut self,
        remote: bool,
        intent: NameIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_start_operation(remote, window, cx) {
            return;
        }
        let card = self.card(remote).read(cx);
        let existing: Vec<String> = card
            .listed_entries(cx)
            .iter()
            .filter(|entry| !entry.is_parent())
            .map(|entry| entry.name.to_string())
            .collect();
        let list = card.focus_handle(cx);
        let (title, subtitle, initial, commit, label) = match &intent {
            NameIntent::Rename(name) => (
                t!("sftp_files.rename.title"),
                Some(SharedString::from(name.clone())),
                SharedString::from(name.clone()),
                t!("sftp_files.rename.commit"),
                t!("sftp_files.rename.label"),
            ),
            NameIntent::New(kind) => (
                kind.title(),
                None,
                kind.default_name(),
                t!("sftp_files.new.commit"),
                t!("sftp_files.new.label"),
            ),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let form = cx.new(|cx| {
            // 改了字，上一次的错误就收起来，提交时再查。
            let subscription = cx.subscribe(&input, |form: &mut NameForm, _, event, cx| {
                if matches!(event, InputEvent::Change) && form.error.take().is_some() {
                    cx.notify();
                }
            });
            NameForm {
                intent,
                existing,
                windows_names: !remote && cfg!(windows),
                input: input.clone(),
                label,
                error: None,
                _subscription: subscription,
            }
        });
        let tokens = Tokens::current(cx);
        let send = self.operation_sender(remote);
        let layer = DialogLayer::new();
        layer.open_dismissable(
            move |dialog, _, _| {
                let (form_ok, send) = (form.clone(), send.clone());
                dialog
                    .title(dialog_title(title.clone(), subtitle.clone(), &tokens))
                    .w(SMALL_DIALOG_WIDTH)
                    .rounded(px(14.))
                    // 只由按钮和 Esc 关，点旁边不关。
                    .overlay_closable(false)
                    .child(form.clone())
                    .footer(dialog_footer(
                        "entry-name-commit",
                        commit.clone(),
                        t!("sftp_files.cancel"),
                    ))
                    .on_ok(move |_, window, cx| {
                        let operation = form_ok.update(cx, |form, cx| form.operation(cx));
                        match operation {
                            Some(operation) => {
                                send.send(operation, window, cx);
                                true
                            }
                            None => false,
                        }
                    })
            },
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
        assert_eq!(check("  ", None).unwrap_err(), "名称不能为空");
        assert!(check("..", None).is_err());
        assert!(check("a/b", None).is_err());
        assert_eq!(check("a.txt", Some("a.txt")).unwrap_err(), "名称未改变");
        assert_eq!(
            check("目录", Some("a.txt")).unwrap_err(),
            "已有名为「目录」的项目"
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
