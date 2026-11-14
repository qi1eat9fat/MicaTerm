// 参考 since2006/shell-rs @ b2a307a: src/explorer/properties_dialog.rs 改写
// 改造：走 `DialogLayer`；内容照原型 `sftpprops`（位置、所有者、3×3、八进制、递归）；
// 不一致的位画成「混合」，八进制的那一位写 `?`（原型 `7?5`），而不是整个清空。
//! 属性：选中项的 rwx 权限，3×3 复选框和八进制数字同步。多选时不一致的位保持每项原样，除非用户改了它。
//! 样子是共用的 `ui::modal` 弹窗（26 号）：头部锁图标 + 「属性」+ 淡字「文件名 · 主机」，
//! 勾选框 12 见方，底部「取消 Esc」「应用 Enter」。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Icon, WindowExt as _, notification::Notification};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::operations::OperationSender;
use super::{CardSide, FileEntry, FileKind, FileOperation, SftpTab};
use crate::app::CatalogIcon;
use crate::backend::sftp::PermissionEdit;
use crate::i18n::t;
use crate::theme::{MONO_FONT_FAMILY, Tokens, size};
use crate::ui::DialogLayer;
use crate::ui::form::{FieldNote, form_field, form_group};
use crate::ui::modal::{
    ModalButtonTone, ModalHead, ModalInputKind, OnClick, modal_button, modal_checkbox,
    modal_dialog, modal_input, modal_input_disabled, render_modal,
};

/// 九个 rwx 位，从所有者的读开始。
const BITS: [u32; 9] = [
    0o400, 0o200, 0o100, 0o040, 0o020, 0o010, 0o004, 0o002, 0o001,
];

/// 复选框每列的宽（原型 `.perm`）。
const GRID_COLUMN: Pixels = px(70.);
/// 第一列（所有者、组、其他）的宽。
const GRID_LABEL: Pixels = px(56.);
/// 八进制框的宽：三位数字。
const OCTAL_WIDTH: Pixels = px(90.);

/// 一项或多项的权限草稿。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermissionDraft {
    value: u32,
    /// 各项之间不一样的位。
    mixed: u32,
    /// 用户动过的位（复选框或八进制）。
    touched: u32,
}

impl PermissionDraft {
    pub fn from_modes(modes: &[u32]) -> Self {
        let all = modes.iter().fold(0o777, |bits, mode| bits & mode) & 0o777;
        let any = modes.iter().fold(0, |bits, mode| bits | mode) & 0o777;
        Self {
            value: all,
            mixed: any & !all,
            touched: 0,
        }
    }

    pub fn is_checked(&self, bit: u32) -> bool {
        self.value & bit != 0
    }

    /// 各项不一样，用户也还没动过。
    pub fn is_mixed(&self, bit: u32) -> bool {
        self.mixed & !self.touched & bit != 0
    }

    pub fn set_bit(&mut self, bit: u32, on: bool) {
        self.touched |= bit;
        if on {
            self.value |= bit;
        } else {
            self.value &= !bit;
        }
    }

    /// 八进制输入：写了数字的位整位替换，`?` 的那一位不动。
    fn apply_octal(&mut self, typed: OctalInput) {
        self.value = (self.value & !typed.mask) | (typed.value & typed.mask);
        self.touched |= typed.mask;
    }

    /// 八进制框里的字：有不一致的位的那一位写 `?`。
    pub fn octal(&self) -> String {
        [6, 3, 0]
            .into_iter()
            .map(|shift| {
                if self.mixed & !self.touched & (0o7 << shift) != 0 {
                    '?'
                } else {
                    char::from_digit((self.value >> shift) & 0o7, 8).unwrap_or('0')
                }
            })
            .collect()
    }

    /// 要做的改动：只有用户动过的位。
    pub fn edit(&self) -> PermissionEdit {
        PermissionEdit::new(self.value & self.touched, !self.value & self.touched)
    }
}

/// 解析好的八进制输入：`mask` 是写了数字的那些位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OctalInput {
    value: u32,
    mask: u32,
}

/// 解析三位八进制，每一位可以是 `?`（不动）。写到一半的不算。
fn parse_octal(text: &str) -> Option<OctalInput> {
    let digits: Vec<char> = text.chars().collect();
    let [first, second, third] = digits.as_slice() else {
        return None;
    };
    let mut typed = OctalInput { value: 0, mask: 0 };
    for (digit, shift) in [(first, 6), (second, 3), (third, 0)] {
        if *digit != '?' {
            typed.value |= digit.to_digit(8)? << shift;
            typed.mask |= 0o7 << shift;
        }
    }
    Some(typed)
}

/// 输入框里允许出现的字。
fn allowed_in_octal(text: &str) -> bool {
    text.chars().count() <= 3 && text.chars().all(|c| c == '?' || ('0'..='7').contains(&c))
}

/// 属性框的内容。
struct PropertiesForm {
    items: Vec<FileEntry>,
    location: String,
    side: CardSide,
    draft: PermissionDraft,
    /// 链接没有自己的权限，只改别的；也可能服务器根本没给权限。
    editable: bool,
    has_dirs: bool,
    recursive: bool,
    add_x_to_dirs: bool,
    octal: Entity<InputState>,
    head: ModalHead,
    send: OperationSender,
    /// 对话框是哪一层：应用、取消时只关自己。
    layer: DialogLayer,
    /// 关了以后焦点回到文件列表。
    list: FocusHandle,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

impl PropertiesForm {
    #[allow(clippy::too_many_arguments)]
    fn new(
        items: Vec<FileEntry>,
        location: String,
        side: CardSide,
        head: ModalHead,
        send: OperationSender,
        list: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let modes: Vec<u32> = items
            .iter()
            .filter(|item| !item.is_link())
            .filter_map(|item| item.permissions)
            .collect();
        let draft = PermissionDraft::from_modes(&modes);
        let octal = cx.new(|cx| {
            InputState::new(window, cx)
                .validate(|text, _| allowed_in_octal(text))
                .default_value(draft.octal())
        });
        // `set_value`（点复选框时）不发 `Change`：这里只收用户打的字。回车就是「应用」。
        let subscription =
            cx.subscribe_in(
                &octal,
                window,
                |this, input, event, window, cx| match event {
                    InputEvent::Change => {
                        if let Some(typed) = parse_octal(input.read(cx).value().as_ref()) {
                            this.draft.apply_octal(typed);
                            cx.notify();
                        }
                    }
                    InputEvent::PressEnter { .. } => this.commit(window, cx),
                    _ => {}
                },
            );
        Self {
            has_dirs: items.iter().any(|item| item.kind == FileKind::Dir),
            editable: !modes.is_empty(),
            items,
            location,
            side,
            draft,
            recursive: false,
            add_x_to_dirs: true,
            octal,
            head,
            send,
            layer: DialogLayer::new(),
            list,
            focus_handle: cx.focus_handle(),
            _subscription: subscription,
        }
    }

    /// 「应用」（回车也是）：关掉对话框；改了什么才发操作，什么都没改就不跑一趟。
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let operation = self.operation();
        self.close(window, cx);
        if let Some(operation) = operation {
            self.send.send(operation, window, cx);
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layer.close(window, cx);
        window.focus(&self.list, cx);
    }

    /// 各项都一样就是那个值，否则说不一致。
    fn shared(values: impl Iterator<Item = Option<SharedString>>) -> SharedString {
        let values: Vec<_> = values.collect();
        match values.first() {
            Some(first) if values.iter().all(|value| value == first) => first
                .clone()
                .unwrap_or_else(|| t!("sftp_files.properties.unknown")),
            Some(_) => t!("sftp_files.properties.mixed"),
            None => t!("sftp_files.properties.unknown"),
        }
    }

    /// 要发的操作；什么都没改就是 `None`，不跑一趟。
    fn operation(&self) -> Option<FileOperation> {
        let recursive = self.recursive && self.has_dirs;
        let add_x_to_dirs = recursive && self.add_x_to_dirs;
        let edit = self.draft.edit();
        if !self.editable || (edit.is_empty() && !add_x_to_dirs) {
            return None;
        }
        Some(FileOperation::Permissions {
            // 符号链接不参与。
            names: self
                .items
                .iter()
                .filter(|item| !item.is_link())
                .map(|item| item.name.to_string())
                .collect(),
            edit,
            recursive,
            add_x_to_dirs,
        })
    }

    fn set_bit(&mut self, bit: u32, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.draft.set_bit(bit, on);
        let octal = self.draft.octal();
        self.octal
            .update(cx, |input, cx| input.set_value(octal, window, cx));
        cx.notify();
    }

    /// 一个权限位：12 见方的勾选框（同 `modal_checkbox`），勾上是正文色底、白勾；不一致时是「混合」
    /// （正文色底、白色短横），点了就勾上。改不了时淡掉一半、点不动。
    fn render_bit(&self, index: usize, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let bit = BITS[index];
        let mixed = self.draft.is_mixed(bit) && self.editable;
        let checked = self.draft.is_checked(bit);
        let editable = self.editable;
        div()
            .id(("perm", index))
            .test_support()
            .aria_label(if mixed {
                t!("sftp_files.properties.mixed")
            } else {
                SharedString::default()
            })
            .role(Role::CheckBox)
            .aria_toggled(if mixed {
                Toggled::Mixed
            } else if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(size::CHECKBOX)
            .rounded(px(3.))
            .map(|box_| {
                if mixed {
                    box_.bg(tokens.text)
                        .child(div().w(px(6.)).h(px(1.5)).bg(tokens.content))
                } else if checked {
                    box_.bg(tokens.text)
                        .text_color(tokens.content)
                        .child(Icon::new(CatalogIcon::Check).size(px(10.)))
                } else {
                    box_.border_1().border_color(tokens.line_strong)
                }
            })
            .when(!editable, |box_| box_.opacity(0.5))
            .when(editable, |box_| {
                box_.cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // 混合的位点了就勾上；其余的反过来。
                        let on = mixed || !this.draft.is_checked(bit);
                        this.set_bit(bit, on, window, cx)
                    }))
            })
            .into_any_element()
    }

    /// 3×3（原型 `.perm`）。
    fn render_grid(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let label = |text: SharedString| div().flex_none().w(GRID_LABEL).child(text);
        let cell = |content: AnyElement| div().flex_none().w(GRID_COLUMN).child(content);
        let header = div()
            .flex()
            .items_center()
            .text_color(tokens.st_dim)
            .child(label(SharedString::default()))
            .children(
                [
                    t!("sftp_files.properties.read"),
                    t!("sftp_files.properties.write"),
                    t!("sftp_files.properties.execute"),
                ]
                .map(|verb| cell(verb.into_any_element())),
            );
        let classes = [
            t!("sftp_files.properties.class_owner"),
            t!("sftp_files.properties.class_group"),
            t!("sftp_files.properties.class_others"),
        ];
        let mut rows = Vec::new();
        for (row, class) in classes.into_iter().enumerate() {
            let bits: Vec<_> = (0..3)
                .map(|column| cell(self.render_bit(row * 3 + column, tokens, cx)))
                .collect();
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .h(px(20.))
                    .child(label(class))
                    .children(bits),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(header)
            .children(rows)
    }
}

impl Focusable for PropertiesForm {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PropertiesForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let owner = t!(
            "sftp_files.properties.owner_group",
            owner = Self::shared(self.items.iter().map(|item| item.owner.clone())),
            group = Self::shared(self.items.iter().map(|item| item.group.clone()))
        );
        let notes = if self.editable {
            Vec::new()
        } else if self.items.iter().all(FileEntry::is_link) {
            vec![FieldNote::Hint(t!("sftp_files.properties.links"))]
        } else {
            vec![FieldNote::Hint(t!("sftp_files.properties.no_rights"))]
        };
        let octal_label = t!("sftp_files.properties.octal");
        let octal = if self.editable {
            modal_input(
                "perm-octal",
                octal_label.clone(),
                &self.octal,
                ModalInputKind::Mono,
                false,
                &tokens,
                window,
                cx,
            )
        } else {
            modal_input_disabled(
                "perm-octal",
                octal_label.clone(),
                &self.octal,
                ModalInputKind::Mono,
                &tokens,
                window,
                cx,
            )
        };
        let rights = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(self.render_grid(&tokens, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().text_color(tokens.text2).child(octal_label))
                    .child(div().flex_none().w(OCTAL_WIDTH).child(octal)),
            );
        let value = |id: &'static str, text: SharedString| {
            div()
                .id(id)
                .test_support()
                .aria_label(text.clone())
                .min_w_0()
                .truncate()
                .text_color(tokens.text2)
                .child(text)
        };
        let mut rows = vec![form_field(
            Some(t!("sftp_files.properties.location")),
            value("file-properties-location", self.location.clone().into())
                .font_family(MONO_FONT_FAMILY),
            Vec::new(),
            &tokens,
        )];
        if self.side.is_remote() {
            rows.push(form_field(
                Some(t!("sftp_files.properties.owner")),
                value("file-properties-owner", owner),
                Vec::new(),
                &tokens,
            ));
        }
        rows.push(form_field(
            Some(t!("sftp_files.properties.rights")),
            rights,
            notes,
            &tokens,
        ));
        if self.has_dirs && self.editable {
            let recursive = self.recursive;
            rows.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(modal_checkbox(
                        "perm-recursive",
                        recursive,
                        t!("sftp_files.properties.recursive"),
                        None,
                        Box::new(cx.listener(|this, _, _, cx| {
                            this.recursive = !this.recursive;
                            cx.notify();
                        })),
                        &tokens,
                    ))
                    // 「目录加 x」只在递归时算数：不递归时淡掉、点不动。
                    .child(
                        div()
                            .when(!recursive, |row| row.opacity(0.5))
                            .child(modal_checkbox(
                                "perm-dir-x",
                                self.add_x_to_dirs,
                                t!("sftp_files.properties.dir_x"),
                                None,
                                Box::new(cx.listener(|this, _, _, cx| {
                                    if this.recursive {
                                        this.add_x_to_dirs = !this.add_x_to_dirs;
                                        cx.notify();
                                    }
                                })),
                                &tokens,
                            )),
                    )
                    .into_any_element(),
            );
        }
        let cancel: OnClick = Box::new(cx.listener(|this, _, window, cx| this.close(window, cx)));
        let footer = div()
            .flex()
            .gap(px(6.))
            .child(modal_button(
                "perm-cancel",
                t!("sftp_files.cancel"),
                Some("Esc"),
                ModalButtonTone::Plain,
                Some(cancel),
                &tokens,
                window,
                cx,
            ))
            .child(modal_button(
                "perm-commit",
                t!("sftp_files.properties.commit"),
                Some("Enter"),
                ModalButtonTone::Primary,
                Some(Box::new(
                    cx.listener(|this, _, window, cx| this.commit(window, cx)),
                )),
                &tokens,
                window,
                cx,
            ));
        render_modal(
            Some(self.head.clone()),
            form_group(rows),
            footer,
            &tokens,
            window,
            cx,
        )
        .id("file-properties")
        .test_support()
        .track_focus(&self.focus_handle)
    }
}

impl SftpTab {
    /// 属性（F9）。Windows 本地不支持改权限：菜单里没有这一项，按键说一声。
    pub(super) fn open_properties(
        &mut self,
        side: CardSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let card = self.card(side).read(cx);
        let items = card.selected_entries(cx);
        let location = card.path();
        let list = card.focus_handle(cx);
        if items.is_empty() || !self.can_start_operation(side, window, cx) {
            return;
        }
        if !side.is_remote() && cfg!(windows) {
            window.push_notification(
                Notification::info(t!("sftp_files.properties.unsupported")),
                cx,
            );
            return;
        }
        let place = if side.is_remote() {
            self.rail_name()
        } else {
            t!("sftp_tab.local_name")
        };
        let subtitle = if let [item] = items.as_slice() {
            t!(
                "sftp_files.properties.subtitle_one",
                name = item.name,
                place = place
            )
        } else {
            t!(
                "sftp_files.properties.subtitle_many",
                count = items.len(),
                place = place
            )
        };
        let head = ModalHead {
            icon: CatalogIcon::Lock,
            title: t!("sftp_files.properties.title"),
            detail: Some(subtitle),
        };
        let send = self.operation_sender(side);
        let form = cx.new(|cx| {
            PropertiesForm::new(items, location, side, head, send, list.clone(), window, cx)
        });
        let (editable, octal, layer) = {
            let form = form.read(cx);
            (form.editable, form.octal.clone(), form.layer.clone())
        };
        let focus = form.read(cx).focus_handle.clone();
        layer.open_dismissable(
            move |dialog, _, _| {
                modal_dialog(dialog)
                    // 回车由八进制框自己处理；框改不了时 gpui-component 的 `Confirm` 也不关框。
                    .on_ok(|_, _, _| false)
                    .child(form.clone())
            },
            // Esc 关掉的：焦点回到文件列表。
            move |window, cx| window.focus(&list, cx),
            window,
            cx,
        );
        if editable {
            octal.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        } else {
            window.focus(&focus, cx);
        }
        self.operations.dialog = Some(layer);
    }
}

#[cfg(test)]
mod tests {
    // 显式导入：`gpui_kit::*` 会遮蔽 `#[test]`。
    use super::{OctalInput, PermissionDraft, allowed_in_octal, parse_octal};
    use crate::backend::sftp::PermissionEdit;

    #[test]
    fn one_item_edits_exactly_what_the_user_changed() {
        let mut draft = PermissionDraft::from_modes(&[0o100_644]);
        assert_eq!(draft.octal(), "644");
        draft.set_bit(0o100, true);
        assert_eq!(draft.octal(), "744");
        assert_eq!(draft.edit(), PermissionEdit::new(0o100, 0));
        draft.apply_octal(parse_octal("600").unwrap());
        assert_eq!(draft.edit(), PermissionEdit::exact(0o600));
        assert_eq!(
            draft.edit().apply(0o104_755, false, false),
            0o104_600,
            "setuid 留着"
        );
    }

    #[test]
    fn differing_bits_stay_per_item_until_touched() {
        let mut draft = PermissionDraft::from_modes(&[0o644, 0o755]);
        assert!(draft.is_checked(0o400) && !draft.is_mixed(0o400));
        assert!(draft.is_mixed(0o100) && !draft.is_checked(0o100));
        // 三位都有不一致的位。
        assert_eq!(draft.octal(), "???");
        draft.set_bit(0o002, true);
        let edit = draft.edit();
        assert_eq!(edit.apply(0o644, false, false), 0o646);
        assert_eq!(edit.apply(0o755, false, false), 0o757);
    }

    #[test]
    fn the_octal_field_marks_only_the_digits_that_differ() {
        let mut draft = PermissionDraft::from_modes(&[0o755, 0o735]);
        assert_eq!(draft.octal(), "7?5");
        assert!(draft.is_mixed(0o040) && !draft.is_mixed(0o010));
        // 打的字里 `?` 那一位不动，别的位算用户改的。
        draft.apply_octal(parse_octal("6?5").unwrap());
        assert_eq!(draft.octal(), "6?5");
        let edit = draft.edit();
        assert_eq!(edit.apply(0o755, false, false), 0o655);
        assert_eq!(edit.apply(0o735, false, false), 0o635);
        // 全写数字就都定下来了。
        draft.apply_octal(parse_octal("640").unwrap());
        assert_eq!(draft.octal(), "640");
    }

    #[test]
    fn directories_can_gain_search_where_they_can_be_read() {
        let edit = PermissionEdit::exact(0o644);
        assert_eq!(edit.apply(0o40_700, true, true), 0o40_755);
        assert_eq!(edit.apply(0o40_700, true, false), 0o40_644);
        assert_eq!(edit.apply(0o100_700, false, true), 0o100_644);
    }

    #[test]
    fn only_three_octal_digits_parse() {
        assert_eq!(
            parse_octal("755"),
            Some(OctalInput {
                value: 0o755,
                mask: 0o777
            })
        );
        assert_eq!(
            parse_octal("7?5"),
            Some(OctalInput {
                value: 0o705,
                mask: 0o707
            })
        );
        assert_eq!(parse_octal("75"), None);
        assert_eq!(parse_octal("7555"), None);
        assert_eq!(parse_octal("789"), None);
        assert!(allowed_in_octal("7?"));
        assert!(!allowed_in_octal("8"));
        assert!(!allowed_in_octal("1234"));
    }
}
