// 参考 since2006/shell-rs @ b2a307a: src/explorer/path_label.rs 改写
// 改造：样子按全部界面原型的 `.crumbs`；点空白处或 Ctrl+O 变成输入框（`.crumbs.edit`），
// 取代 WinSCP 的「打开目录」对话框。
//! 文件列表上面的路径栏：平时是可点的分段路径，指针下那一级和它之前的都亮；
//! 点空白处或按 Ctrl+O 变成输入框，回车切换，Esc 取消；去不了就描红写原因，不切换。

use std::ops::Range;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{CardSide, FileCard, path_ancestors};
use crate::app::SftpTabCommand;
use crate::i18n::t;
use crate::theme::Tokens;

/// 路径栏的字号（原型 `.crumbs`）。
const FONT_SIZE: Pixels = px(12.5);

/// 路径栏的一段：带分隔符的文字（`/home/tester/` 是 `/`、`home/`、`tester/`），和它打开的目录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathPart {
    pub label: String,
    pub path: String,
}

pub fn path_parts(path: &str, side: CardSide) -> Vec<PathPart> {
    let separator = separator(side);
    path_ancestors(path, side)
        .into_iter()
        .enumerate()
        .map(|(ix, (title, path))| PathPart {
            // 根的名字就是根自己，已经带着分隔符。
            label: if ix == 0 {
                title
            } else {
                format!("{title}{separator}")
            },
            path,
        })
        .collect()
}

fn separator(side: CardSide) -> char {
    if side.is_remote() {
        '/'
    } else {
        std::path::MAIN_SEPARATOR
    }
}

/// 放不下时把哪几段折叠成 `…`：根留着，然后是 `…`，然后是放得下的最后几段。全放得下时是空的。
/// 连最后一段都放不下时，根和它之间全折叠，最后一段截断。
pub fn folded_parts(widths: &[f32], ellipsis: f32, available: f32) -> Range<usize> {
    if widths.len() < 3 || widths.iter().sum::<f32>() <= available {
        return 0..0;
    }
    let mut start = widths.len() - 1;
    let mut used = widths[0] + ellipsis + widths[start];
    while start > 2 && used + widths[start - 1] <= available {
        start -= 1;
        used += widths[start];
    }
    1..start
}

/// 路径栏的输入态。
pub struct PathEdit {
    input: Entity<InputState>,
    /// 上一次回车为什么没去成：描红写在右边。一改文字就清掉。
    error: Option<SharedString>,
    _subscription: Subscription,
}

impl FileCard {
    /// 路径栏变成输入框，选中当前路径（点空白处、Ctrl+O）。已经是输入框就把焦点给它。
    pub fn begin_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(edit) = &self.path_edit {
            let input = edit.input.clone();
            input.update(cx, |input, cx| input.focus(window, cx));
            return;
        }
        let path = self.path.clone();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(path));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let text = input.read(cx).value().to_string();
                    this.submit_path_edit(text, window, cx);
                }
                InputEvent::Change => {
                    if let Some(edit) = &mut this.path_edit {
                        edit.error = None;
                        cx.notify();
                    }
                }
                // 点了别处就不改了。窗口切走不算：切回来还接着输入。
                InputEvent::Blur if window.is_window_active() => {
                    this.path_edit = None;
                    cx.notify();
                }
                _ => {}
            },
        );
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        self.path_edit = Some(PathEdit {
            input,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn submit_path_edit(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if text.is_empty() {
            self.close_path_edit(window, cx);
            return;
        }
        let side = self.side;
        self.dispatch_command(SftpTabCommand::SubmitPath { side, text }, window, cx);
    }

    /// 关掉输入框，焦点回到文件列表（Esc，或者已经切过去了）。
    pub(super) fn close_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 先放掉，失焦事件才不会再关一次。
        if self.path_edit.take().is_some() {
            let focus = self.focus_handle(cx);
            window.focus(&focus, cx);
            cx.notify();
        }
    }

    /// 路径栏输入的目录去不了：描红写原因，留在输入框，不切换。
    pub(super) fn path_edit_failed(&mut self, reason: SharedString, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.path_edit {
            edit.error = Some(reason);
            cx.notify();
        }
    }

    /// 路径栏上的错误，测试用。
    pub fn path_edit_error(&self) -> Option<SharedString> {
        self.path_edit.as_ref().and_then(|edit| edit.error.clone())
    }

    /// 路径栏是不是输入框。
    pub fn is_editing_path(&self) -> bool {
        self.path_edit.is_some()
    }

    /// 路径栏（原型 `.crumbs`）。
    pub(super) fn render_path_bar(
        &self,
        window: &mut Window,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match &self.path_edit {
            Some(edit) => self.render_path_input(edit, tokens, cx),
            None => self.render_path_parts(window, tokens, cx),
        }
    }

    fn render_path_input(
        &self,
        edit: &PathEdit,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hint = edit
            .error
            .clone()
            .unwrap_or_else(|| t!("sftp_tab.path.hint"));
        let failed = edit.error.is_some();
        div()
            .id(self.side.path_id())
            .test_support()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .mt(px(2.))
            .mb(px(6.))
            .mx(px(10.))
            .h(px(26.))
            .pl(px(2.))
            .pr(px(6.))
            .rounded(px(8.))
            .bg(tokens.field_focused)
            .border(px(1.5))
            .border_color(if failed { tokens.bad } else { tokens.accent })
            .text_size(FONT_SIZE)
            // Esc 取消。按键先过输入框自己的绑定，所以在捕获阶段拦下来。
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close_path_edit(window, cx);
                }
            }))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&edit.input)
                        .id(self.side.path_input_id())
                        .appearance(false)
                        .small(),
                ),
            )
            .child(
                div()
                    .id(self.side.path_hint_id())
                    .test_support()
                    .aria_label(hint.clone())
                    .flex_none()
                    .max_w(relative(0.6))
                    .truncate()
                    .text_size(px(11.5))
                    .text_color(if failed { tokens.bad } else { tokens.faint })
                    .child(hint),
            )
            .into_any_element()
    }

    fn render_path_parts(
        &self,
        window: &mut Window,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let side = self.side;
        let enabled = self.takes_commands();
        let parts = self.shown_parts(window);
        let hot = self
            .hovered_part
            .as_ref()
            .and_then(|path| parts.iter().position(|part| &part.path == path));
        let last = parts.len().saturating_sub(1);
        let (text, muted, hover) = (tokens.text, tokens.muted, tokens.hover);
        let width = self.label_width.clone();
        let card = cx.entity_id();
        div()
            .id(self.side.path_id())
            .test_support()
            // 窗口列表这一侧现在的目录，读完了才写。
            .when(!self.loading, |this| this.aria_value(self.path.clone()))
            .aria_selected(self.current)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .mt(px(2.))
            .mb(px(6.))
            .mx(px(10.))
            .h(px(26.))
            .pl(px(8.))
            .pr(px(4.))
            .rounded(px(8.))
            .bg(tokens.field)
            .overflow_hidden()
            .text_size(FONT_SIZE)
            .whitespace_nowrap()
            // 点路径栏的空白处变成输入框（点某一级时那一级把冒泡拦下了）。
            .when(enabled, |this| {
                this.on_click(cx.listener(move |this, _, window, cx| {
                    this.dispatch_command(SftpTabCommand::EditPath { side }, window, cx);
                }))
            })
            .children(parts.into_iter().enumerate().map(|(ix, part)| {
                let current = ix == last;
                let path = part.path.clone();
                let strong = enabled && (current || hot.is_some_and(|hot| ix <= hot));
                div()
                    .id(SharedString::from(format!("path:{}", part.path)))
                    .test_support()
                    .role(Role::Link)
                    .px(px(2.))
                    .rounded(px(5.))
                    .text_color(if strong { text } else { muted })
                    .when(current, |this| this.font_weight(FontWeight::SEMIBOLD))
                    .map(|this| {
                        if current {
                            this.min_w_0().truncate()
                        } else {
                            this.flex_shrink_0()
                        }
                    })
                    .when(enabled, |this| {
                        let target = part.path.clone();
                        this.cursor_pointer()
                            .hover(move |style| style.bg(hover))
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                this.hover_part(&path, *hovered, cx)
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.dispatch_command(
                                    SftpTabCommand::Navigate {
                                        side,
                                        path: target.clone(),
                                    },
                                    window,
                                    cx,
                                );
                            }))
                    })
                    .child(part.label)
            }))
            // 上一帧有多宽决定折叠哪几段；宽度变了下一帧生效。
            .child(
                canvas(
                    move |bounds, window, _| {
                        if width.get() != Some(bounds.size.width) {
                            width.set(Some(bounds.size.width));
                            window.on_next_frame(move |_, cx| cx.notify(card));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    }

    /// 要显示的段：上一帧放不下时中间折叠成 `…`。
    fn shown_parts(&self, window: &mut Window) -> Vec<PathPart> {
        let side = self.side;
        let parts = path_parts(&self.path, side);
        let Some(available) = self.label_width.get() else {
            return parts;
        };
        if parts.len() < 3 {
            return parts;
        }
        let font = window.text_style().font();
        let measure = |text: &str| {
            let run = TextRun {
                len: text.len(),
                font: font.clone(),
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line =
                window
                    .text_system()
                    .shape_line(text.to_string().into(), FONT_SIZE, &[run], None);
            // 每段左右各留 2。
            f32::from(line.width) + 4.
        };
        let widths: Vec<f32> = parts.iter().map(|part| measure(&part.label)).collect();
        let ellipsis = format!("\u{2026}{}", separator(side));
        // 留一点余量，量的和排的舍入可能不一样；路径栏左右还有 12 的内边距。
        let folded = folded_parts(&widths, measure(&ellipsis), f32::from(available) - 13.);
        if folded.is_empty() {
            return parts;
        }
        let mut shown = vec![parts[0].clone()];
        // `…` 打开它藏起来的最后一级。
        shown.push(PathPart {
            label: ellipsis,
            path: parts[folded.end - 1].path.clone(),
        });
        shown.extend(parts[folded.end..].iter().cloned());
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::{CardSide, PathPart, folded_parts, path_parts};

    #[test]
    fn parts_end_in_the_separator_and_open_their_directory() {
        let parts = path_parts("/home/tester", CardSide::Remote);
        let labels: Vec<_> = parts.iter().map(|part| part.label.as_str()).collect();
        assert_eq!(labels, ["/", "home/", "tester/"]);
        assert_eq!(
            parts[1],
            PathPart {
                label: "home/".into(),
                path: "/home".into()
            }
        );
        assert_eq!(path_parts("/", CardSide::Remote).len(), 1);
        assert!(path_parts("", CardSide::Remote).is_empty());
    }

    #[test]
    fn folding_keeps_the_root_and_as_many_last_parts_as_fit() {
        let widths = [10., 50., 50., 50.];
        assert_eq!(folded_parts(&widths, 10., 160.), 0..0, "全放得下");
        // `/…/b/c/` 是 10 + 10 + 50 + 50。
        assert_eq!(folded_parts(&widths, 10., 125.), 1..2);
        assert_eq!(folded_parts(&widths, 10., 80.), 1..3, "只放得下最后一段");
        assert_eq!(folded_parts(&widths, 10., 10.), 1..3, "最后一段截断");
        assert_eq!(folded_parts(&[10., 500.], 10., 100.), 0..0, "没什么可折叠");
    }
}
