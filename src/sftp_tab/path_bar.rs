//! SFTP path breadcrumb bar (aligned with Muxy's panel sub-headers): `24px` tall, same as the
//! pane header above and the table header below, with a bottom `1px` hairline. Displays clickable rounded folder crumbs separated by `ChevronRight` glyphs,
//! with the active directory in foreground text. Clicking blank space (or `Ctrl+O`)
//! switches to an inline Muxy input field; `Enter` navigates, `Esc` cancels, and an unreadable
//! target highlights the input border and error hint in `tokens.err`.

use std::ops::Range;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{CardSide, FileCard, path_ancestors};
use crate::app::{CatalogIcon, SftpTabCommand};
use crate::i18n::t;
use crate::theme::{Tokens, TypeScale, WEIGHT_NORMAL, size, space};

/// Path bar height (same as the pane header and table header) and crumb geometry.
const BAR_HEIGHT: Pixels = size::DOCK_HEADER;
const CRUMB_HEIGHT: Pixels = px(20.);
const CRUMB_PADDING: Pixels = px(5.);
const CHEVRON_SIZE: Pixels = px(10.);

/// One clickable directory segment in the path breadcrumb bar.
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
            // Strip trailing slash from local drive roots (`C:\` -> `C:`); remote root stays `/`.
            label: if ix == 0 && side == CardSide::Local && title.len() > 1 {
                title.trim_end_matches(separator).to_string()
            } else {
                title
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

/// Computes the middle slice `1..end` to collapse into `…` when all segments do not fit.
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

/// Active inline path-editing state.
pub struct PathEdit {
    input: Entity<InputState>,
    /// Error from the last failed navigation attempt; cleared on edit.
    error: Option<SharedString>,
    _subscription: Subscription,
}

impl FileCard {
    /// Enters inline path-edit mode and selects the full path (`Ctrl+O` or clicking path bar).
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

    /// Exits inline path editing and restores focus to the file list.
    pub(super) fn close_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_edit.take().is_some() {
            let focus = self.focus_handle(cx);
            window.focus(&focus, cx);
            cx.notify();
        }
    }

    /// Keeps inline path editing open and displays `reason` in danger color.
    pub(super) fn path_edit_failed(&mut self, reason: SharedString, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.path_edit {
            edit.error = Some(reason);
            cx.notify();
        }
    }

    /// Current path-bar error message (used by UI tests).
    pub fn path_edit_error(&self) -> Option<SharedString> {
        self.path_edit.as_ref().and_then(|edit| edit.error.clone())
    }

    /// Whether the path bar is currently in inline input mode.
    pub fn is_editing_path(&self) -> bool {
        self.path_edit.is_some()
    }

    /// Sub-header breadcrumb / inline path input bar.
    pub(super) fn render_path_bar(
        &self,
        window: &mut Window,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scale = TypeScale::current(cx);
        let bar = div()
            .id(self.side.path_id())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .h(BAR_HEIGHT)
            .px(space::SPACE_8)
            .border_b(size::LINE)
            .border_color(tokens.line)
            .bg(tokens.content)
            .text_size(scale.body)
            .whitespace_nowrap()
            .overflow_hidden();
        match &self.path_edit {
            Some(edit) => self.render_path_input(bar, edit, tokens, scale, cx),
            None => self.render_path_parts(bar, window, tokens, scale, cx),
        }
    }

    /// Inline path input capsule with focus/error ring and right-aligned hint.
    fn render_path_input(
        &self,
        bar: Stateful<Div>,
        edit: &PathEdit,
        tokens: &Tokens,
        scale: TypeScale,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hint = edit
            .error
            .clone()
            .unwrap_or_else(|| t!("sftp_tab.path.hint"));
        let failed = edit.error.is_some();
        bar.px(space::SPACE_6)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close_path_edit(window, cx);
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(space::SPACE_6)
                    .h(px(22.))
                    .px(space::SPACE_8)
                    .rounded(size::RADIUS_MD)
                    .bg(tokens.input)
                    .border_1()
                    .border_color(if failed { tokens.err } else { tokens.brand })
                    .child(
                        div()
                            .flex_none()
                            .text_color(if failed { tokens.err } else { tokens.brand })
                            .child(Icon::new(CatalogIcon::Folder).size(size::ICON_SM)),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&edit.input)
                                .id(self.side.path_input_id())
                                .appearance(false)
                                .small()
                                .px_0()
                                .text_size(scale.body),
                        ),
                    )
                    .child(
                        div()
                            .id(self.side.path_hint_id())
                            .test_support()
                            .aria_label(hint.clone())
                            .flex_none()
                            .max_w(relative(0.58))
                            .truncate()
                            .text_size(scale.sm)
                            .text_color(if failed { tokens.err } else { tokens.st_dim })
                            .child(hint),
                    ),
            )
            .test_support()
            .into_any_element()
    }

    fn render_path_parts(
        &self,
        bar: Stateful<Div>,
        window: &mut Window,
        tokens: &Tokens,
        scale: TypeScale,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let side = self.side;
        let enabled = self.takes_commands();
        let parts = self.shown_parts(window, scale.body);
        let has_parts = !parts.is_empty();
        let last = parts.len().saturating_sub(1);
        let (text, text2, faint, hover) = (tokens.text, tokens.text2, tokens.faint, tokens.hover);
        let width = self.label_width.clone();
        let card = cx.entity_id();
        bar.gap(space::SPACE_2)
            .when(!self.loading, |this| this.aria_value(self.path.clone()))
            .aria_selected(self.current)
            .text_color(text2)
            .when(enabled, |this| {
                this.cursor_text()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.dispatch_command(SftpTabCommand::EditPath { side }, window, cx);
                    }))
            })
            .when(has_parts, |this| {
                this.child(
                    div()
                        .flex_none()
                        .pl(space::SPACE_2)
                        .pr(space::SPACE_2)
                        .text_color(tokens.st_dim)
                        .child(Icon::new(CatalogIcon::Folder).size(size::ICON_SM)),
                )
            })
            .children(parts.into_iter().enumerate().flat_map(|(ix, part)| {
                let current = ix == last;
                let sep = (ix > 0).then(|| {
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .text_color(faint)
                        .child(Icon::new(IconName::ChevronRight).size(CHEVRON_SIZE))
                        .into_any_element()
                });
                let crumb = div()
                    .id(SharedString::from(format!("path:{}", part.path)))
                    .test_support()
                    .role(Role::Link)
                    .flex()
                    .items_center()
                    .h(CRUMB_HEIGHT)
                    .px(CRUMB_PADDING)
                    .rounded(size::RADIUS_SM)
                    .text_color(if current { text } else { text2 })
                    .when(current, |this| this.font_weight(WEIGHT_NORMAL))
                    .map(|this| {
                        if current {
                            this.min_w_0()
                                .child(div().min_w_0().truncate().child(part.label))
                        } else {
                            this.flex_shrink_0().child(part.label)
                        }
                    })
                    .when(enabled, |this| {
                        let target = part.path.clone();
                        this.cursor_pointer()
                            .hover(move |style| style.bg(hover).text_color(text))
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
                    .into_any_element();
                sep.into_iter().chain(std::iter::once(crumb))
            }))
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
            .test_support()
            .into_any_element()
    }

    /// Computes visible segments, collapsing middle segments into `…` when narrow.
    fn shown_parts(&self, window: &mut Window, font_size: Pixels) -> Vec<PathPart> {
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
                    .shape_line(text.to_string().into(), font_size, &[run], None);
            f32::from(line.width)
        };
        let padding = f32::from(CRUMB_PADDING) * 2.;
        let chevron = f32::from(CHEVRON_SIZE) + 4.;
        let widths: Vec<f32> = parts
            .iter()
            .enumerate()
            .map(|(ix, part)| {
                let sep = if ix > 0 { chevron } else { 0. };
                measure(&part.label) + padding + sep
            })
            .collect();
        let ellipsis = "\u{2026}";
        // Account for horizontal bar padding (`16`), leading folder icon (`18`), and trailing edit button (`24`).
        let folded = folded_parts(
            &widths,
            measure(ellipsis) + padding + chevron,
            (f32::from(available) - 58.).max(0.),
        );
        if folded.is_empty() {
            return parts;
        }
        let mut shown = vec![parts[0].clone()];
        shown.push(PathPart {
            label: ellipsis.into(),
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
        assert_eq!(labels, ["/", "home", "tester"]);
        assert_eq!(
            parts[1],
            PathPart {
                label: "home".into(),
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
        assert_eq!(folded_parts(&widths, 10., 125.), 1..2);
        assert_eq!(folded_parts(&widths, 10., 80.), 1..3, "只放得下最后一段");
        assert_eq!(folded_parts(&widths, 10., 10.), 1..3, "最后一段截断");
        assert_eq!(folded_parts(&[10., 500.], 10., 100.), 0..0, "没什么可折叠");
    }
}
