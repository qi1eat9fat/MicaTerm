//! 编辑器的查找替换（26 号「编辑器」，UI 重做工单 11）：和终端同一张查找浮卡（`micaterm_ui::FindCard`），
//! 多一行「替换」。Ctrl+F 开查找、Ctrl+H 连替换一起开（截下编辑器自带的 `Search` / `Replace`）；
//! 智能大小写（查询里没有大写就不分大小写），没有 Aa、正则开关。
//!
//! - 输入查询：从打开时光标所在处往后找第一个，光标跳到它后面（文字跟着滚）；
//! - Enter / F3 下一个，Shift+Enter / Shift+F3 上一个，到头绕回；Esc 关掉，焦点回到文字；
//! - 替换框里 Enter 替换当前这个并跳到下一个，Ctrl+Alt+Enter 全部替换（一次撤销就能全部撤回）。
//!
//! 匹配的高亮自己画在文字上面，颜色是 `FindStyle` 里和终端同一套：其余匹配是很淡的暖黄底 + 1px 黄边
//! （透出下面的字），当前项是实心暖黄底，字重画成深色。编辑器自带的那套高亮用的是选区的颜色，不用。

use std::ops::Range;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::*;
use micaterm_ui::{FindButton, FindCard, FindStyle, FindTextButton, ReplaceRow};

use super::{EditorTab, Phase};
use crate::app::{
    CatalogIcon, DismissEditorFind, EDITOR_FIND_CONTEXT, FindNextInEditor, FindPreviousInEditor,
    ReplaceAllInEditor, ReplaceInEditor,
};
use crate::i18n::t;

/// 开着的查找浮卡。
pub(super) struct EditorFind {
    query: Entity<InputState>,
    replacement: Entity<InputState>,
    /// 显示「替换」那一行（Ctrl+H 打开的）。
    replace_mode: bool,
    /// 全部匹配，按位置。文字和查询一变就重算。
    matches: Vec<Range<usize>>,
    current: Option<usize>,
    /// 打开查找时光标在哪：输入查询时从这里往后找第一个。
    anchor: usize,
    /// 上次找的查询：查询框说变了而文字没变时不重找。
    searched: SharedString,
    _subscriptions: Vec<Subscription>,
}

/// `text` 里 `query` 出现的地方（字节范围，不重叠）。智能大小写：查询里没有大写字母就不分大小写。
pub fn find_matches(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    if query.chars().any(char::is_uppercase) {
        return text
            .match_indices(query)
            .map(|(start, found)| start..start + found.len())
            .collect();
    }
    // 查询全是小写：文字一个字一个字转成小写再比。
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
    let first = query.chars().next().unwrap_or_default();
    let mut found = Vec::new();
    let mut from = 0;
    for (start, c) in text.char_indices() {
        if start < from || lower(c) != first {
            continue;
        }
        let mut end = start;
        let mut rest = text[start..].char_indices();
        let matched = query.chars().all(|wanted| match rest.next() {
            Some((offset, c)) if lower(c) == wanted => {
                end = start + offset + c.len_utf8();
                true
            }
            _ => false,
        });
        if matched {
            found.push(start..end);
            from = end;
        }
    }
    found
}

/// `offset` 处或之后的第一个匹配（结束在 `offset` 的也算：光标停在匹配后面时还是它）；后面没有就绕回第一个。
fn first_from(matches: &[Range<usize>], offset: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(
        matches
            .iter()
            .position(|range| range.end >= offset)
            .unwrap_or(0),
    )
}

impl EditorTab {
    /// 开查找浮卡（Ctrl+F），`replace` 时连「替换」那一行（Ctrl+H）。已经开着就把焦点给查询、全选它；
    /// 选着一段（一行以内的）文字时，它就是查询。
    pub(super) fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (selected, cursor) = {
            let state = self.state.read(cx);
            (state.selected_value().to_string(), state.cursor())
        };
        if self.find.is_none() {
            let query =
                cx.new(|cx| InputState::new(window, cx).placeholder(t!("editor.find.placeholder")));
            let replacement = cx.new(|cx| {
                InputState::new(window, cx).placeholder(t!("editor.find.replace_placeholder"))
            });
            let subscriptions = vec![
                cx.subscribe_in(
                    &query,
                    window,
                    |this, _, event: &InputEvent, _, cx| match event {
                        InputEvent::Change => this.query_changed(cx),
                        InputEvent::PressEnter { shift, .. } => this.step_find(!*shift, cx),
                        _ => {}
                    },
                ),
                cx.subscribe_in(
                    &replacement,
                    window,
                    |this, _, event: &InputEvent, window, cx| {
                        if let InputEvent::PressEnter { .. } = event {
                            this.replace_current(window, cx);
                        }
                    },
                ),
            ];
            self.find = Some(EditorFind {
                query,
                replacement,
                replace_mode: false,
                matches: Vec::new(),
                current: None,
                anchor: cursor,
                searched: SharedString::default(),
                _subscriptions: subscriptions,
            });
        }
        let Some(find) = &mut self.find else {
            return;
        };
        find.replace_mode = replace;
        find.anchor = cursor;
        let query = find.query.clone();
        let last = self.last_query.clone();
        query.update(cx, |input, cx| {
            if !selected.is_empty() && !selected.contains('\n') {
                input.set_value(selected, window, cx);
            } else if input.value().is_empty() && !last.is_empty() {
                input.set_value(last, window, cx);
            }
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        if let Some(find) = &mut self.find {
            find.searched = find.query.read(cx).value();
        }
        self.search_from_anchor(cx);
    }

    /// 查询框说变了：文字真的变了才重新找（回车时它也会说一声）。
    fn query_changed(&mut self, cx: &mut Context<Self>) {
        let Some(find) = &mut self.find else {
            return;
        };
        let query = find.query.read(cx).value();
        if query == find.searched {
            return;
        }
        find.searched = query;
        self.search_from_anchor(cx);
    }

    /// 查询变了：重算，从打开时的位置往后找第一个，光标跳过去。
    fn search_from_anchor(&mut self, cx: &mut Context<Self>) {
        self.recount(cx);
        let Some(find) = &mut self.find else {
            return;
        };
        find.current = first_from(&find.matches, find.anchor);
        self.reveal_current(cx);
    }

    /// 文字或查询变了：重算全部匹配。当前项尽量留在原来的位置附近。
    pub(super) fn recount(&mut self, cx: &mut Context<Self>) {
        let Some(find) = &mut self.find else {
            return;
        };
        let query = find.query.read(cx).value().to_string();
        let text = self.state.read(cx).text().to_string();
        let near = find
            .current
            .and_then(|ix| find.matches.get(ix))
            .map(|range| range.start);
        find.matches = find_matches(&text, &query);
        find.current = near.and_then(|offset| first_from(&find.matches, offset));
        cx.notify();
    }

    /// 光标跳到当前项后面（文字滚到看得见），焦点留在浮卡里。
    fn reveal_current(&mut self, cx: &mut Context<Self>) {
        let Some(range) = self
            .find
            .as_ref()
            .and_then(|find| find.matches.get(find.current?).cloned())
        else {
            cx.notify();
            return;
        };
        self.state.update(cx, |state, cx| {
            state.set_selected_range(range.end..range.end, cx)
        });
        cx.notify();
    }

    /// 下一个（`forward`）或上一个，到头绕回。
    fn step_find(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(find) = &mut self.find else {
            return;
        };
        let count = find.matches.len();
        if count == 0 {
            return;
        }
        find.current = Some(match (find.current, forward) {
            (Some(ix), true) => (ix + 1) % count,
            (Some(ix), false) => (ix + count - 1) % count,
            (None, true) => 0,
            (None, false) => count - 1,
        });
        self.reveal_current(cx);
    }

    /// 替换当前项，跳到它后面的下一个。重新加载时文字只读，不替换。
    fn replace_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.phase != Phase::Idle {
            return;
        }
        let Some((range, replacement)) = self.find.as_ref().and_then(|find| {
            let range = find.matches.get(find.current?)?.clone();
            Some((range, find.replacement.read(cx).value().to_string()))
        }) else {
            return;
        };
        let after = range.start + replacement.len();
        self.state.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
            state.replace(replacement, window, cx);
        });
        self.recount(cx);
        if let Some(find) = &mut self.find {
            find.current = first_from(&find.matches, after);
        }
        self.reveal_current(cx);
    }

    /// 全部替换：一次改完，撤销一次就全部回来。
    fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.phase != Phase::Idle {
            return;
        }
        let Some((matches, replacement)) = self.find.as_ref().map(|find| {
            (
                find.matches.clone(),
                find.replacement.read(cx).value().to_string(),
            )
        }) else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        let mut text = self.state.read(cx).text().to_string();
        for range in matches.iter().rev() {
            text.replace_range(range.clone(), &replacement);
        }
        self.state
            .update(cx, |state, cx| state.replace_all(text, window, cx));
        self.recount(cx);
    }

    /// 关掉浮卡（Esc、×），焦点回到文字。
    fn dismiss_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(find) = self.find.take() {
            self.last_query = find.query.read(cx).value();
            self.state.update(cx, |state, cx| state.focus(window, cx));
            cx.notify();
        }
    }

    /// 查找浮卡上的动作：挂在编辑器标签的根上。
    pub(super) fn find_actions(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        use gpui_kit::component::input::{Replace, Search};
        root
            // 编辑器自带的 Ctrl+F / Ctrl+H 截下来，开自己的浮卡。
            .capture_action(cx.listener(|this, _: &Search, window, cx| {
                cx.stop_propagation();
                this.open_find(false, window, cx);
            }))
            .capture_action(cx.listener(|this, _: &Replace, window, cx| {
                cx.stop_propagation();
                this.open_find(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FindNextInEditor, _, cx| this.step_find(true, cx)))
            .on_action(
                cx.listener(|this, _: &FindPreviousInEditor, _, cx| this.step_find(false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &DismissEditorFind, window, cx| {
                    this.dismiss_find(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &ReplaceInEditor, window, cx| {
                    this.replace_current(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &ReplaceAllInEditor, window, cx| {
                    this.replace_all(window, cx)
                }),
            )
    }

    /// 「当前/总数」：有查询才写。
    fn find_count(find: &EditorFind, cx: &App) -> Option<SharedString> {
        if find.query.read(cx).value().is_empty() {
            return None;
        }
        let total = find.matches.len();
        Some(match find.current {
            Some(ix) if total > 0 => format!("{}/{total}", ix + 1).into(),
            _ => format!("0/{total}").into(),
        })
    }

    /// 查找浮卡（开着时）。
    pub(super) fn render_find(&self, cx: &App) -> Option<Div> {
        let find = self.find.as_ref()?;
        let navigate = !find.matches.is_empty();
        let editable = self.phase == Phase::Idle;
        let replace = find.replace_mode.then(|| {
            ReplaceRow::new(
                &find.replacement,
                "editor-replace",
                t!("editor.find.replace_placeholder"),
                Icon::new(CatalogIcon::ArrowRightLeft),
            )
            .button(
                FindTextButton::new(
                    "editor-replace-one",
                    t!("editor.find.replace"),
                    Some("Enter".into()),
                    |_, window, cx| window.dispatch_action(Box::new(ReplaceInEditor), cx),
                )
                .enabled(navigate && editable),
            )
            .button(
                FindTextButton::new(
                    "editor-replace-all",
                    t!("editor.find.replace_all"),
                    Some("Ctrl+Alt+Enter".into()),
                    |_, window, cx| window.dispatch_action(Box::new(ReplaceAllInEditor), cx),
                )
                .enabled(navigate && editable),
            )
        });
        Some(
            FindCard::new(&find.query, "editor-find", t!("editor.find.placeholder"))
                .count("editor-find-count", Self::find_count(find, cx))
                .button(
                    FindButton::new(
                        "editor-find-previous",
                        IconName::ChevronUp,
                        t!("editor.find.previous"),
                        Some("Shift+Enter".into()),
                        |_, window, cx| window.dispatch_action(Box::new(FindPreviousInEditor), cx),
                    )
                    .enabled(navigate),
                )
                .button(
                    FindButton::new(
                        "editor-find-next",
                        IconName::ChevronDown,
                        t!("editor.find.next"),
                        Some("Enter".into()),
                        |_, window, cx| window.dispatch_action(Box::new(FindNextInEditor), cx),
                    )
                    .enabled(navigate),
                )
                .button(FindButton::new(
                    "editor-find-close",
                    IconName::Close,
                    t!("editor.find.close"),
                    Some("Esc".into()),
                    |_, window, cx| window.dispatch_action(Box::new(DismissEditorFind), cx),
                ))
                .replace(replace)
                .build(cx)
                .key_context(EDITOR_FIND_CONTEXT),
        )
    }

    /// 匹配的高亮：画在编辑器上面（放在编辑器后面），只画看得见的。`font` 是编辑器的字体和字号，
    /// 当前项的字用它重画成深色。
    pub(super) fn render_matches(&self, font: (SharedString, Pixels)) -> Option<AnyElement> {
        let find = self.find.as_ref()?;
        if find.matches.is_empty() {
            return None;
        }
        let state = self.state.clone();
        let matches = find.matches.clone();
        let current = find.current;
        Some(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| {
                    let style = FindStyle::current(cx);
                    // 先量出每个匹配在哪（读编辑器），再画（画字要用 `cx`）。
                    let (clip, marks) = {
                        let state = state.read(cx);
                        let clip = state.text_bounds().unwrap_or(bounds).intersect(&bounds);
                        let text = state.text();
                        let marks: Vec<(usize, Bounds<Pixels>, SharedString)> = matches
                            .iter()
                            .enumerate()
                            .filter_map(|(ix, range)| {
                                let line = state.range_to_bounds(range)?;
                                line.intersects(&clip).then(|| {
                                    let slice = if current == Some(ix) {
                                        text.slice(range.clone()).to_string().into()
                                    } else {
                                        SharedString::default()
                                    };
                                    (ix, line, slice)
                                })
                            })
                            .collect();
                        (clip, marks)
                    };
                    let (family, font_size) = font.clone();
                    // 和终端的高亮一样：比行矮，高是字号的 1.3 倍，在行里居中。
                    let height = (font_size * 1.3).round();
                    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                        for (ix, line, slice) in marks {
                            let mark = Bounds::new(
                                point(
                                    line.left(),
                                    line.top() + ((line.size.height - height) / 2.).floor(),
                                ),
                                size(line.size.width, height),
                            );
                            if current == Some(ix) {
                                window.paint_quad(quad(
                                    mark,
                                    px(2.),
                                    style.current,
                                    px(0.),
                                    transparent_black(),
                                    BorderStyle::Solid,
                                ));
                                let run = TextRun {
                                    len: slice.len(),
                                    font: gpui_kit::font(family.clone()),
                                    color: style.current_text,
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                };
                                let shaped =
                                    window
                                        .text_system()
                                        .shape_line(slice, font_size, &[run], None);
                                let _ = shaped.paint(
                                    line.origin,
                                    line.size.height,
                                    TextAlign::Left,
                                    None,
                                    window,
                                    cx,
                                );
                            } else {
                                window.paint_quad(quad(
                                    mark,
                                    px(2.),
                                    style.matched,
                                    px(1.),
                                    style.matched_line,
                                    BorderStyle::Solid,
                                ));
                            }
                        }
                    });
                },
            )
            .absolute()
            .inset_0()
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{find_matches, first_from};

    #[test]
    fn lowercase_queries_ignore_case_and_capitals_do_not() {
        let text = "Port 8080; port 80";
        assert_eq!(find_matches(text, "port"), vec![0..4, 11..15]);
        assert_eq!(find_matches(text, "Port"), vec![0..4]);
        assert_eq!(find_matches(text, "8080"), vec![5..9]);
        assert!(find_matches(text, "").is_empty());
        // 特殊字符照字面找；中文、重叠的只算不重叠的。
        assert_eq!(find_matches("a.b axb", "a.b"), vec![0..3]);
        assert_eq!(find_matches("监听 80 端口", "端口"), vec![10..16]);
        assert_eq!(find_matches("aaaa", "aa"), vec![0..2, 2..4]);
    }

    #[test]
    fn the_first_match_from_a_place_wraps_around() {
        let matches = [2..4, 10..12];
        assert_eq!(first_from(&matches, 0), Some(0));
        assert_eq!(first_from(&matches, 5), Some(1));
        assert_eq!(first_from(&matches, 12), Some(1));
        assert_eq!(first_from(&matches, 13), Some(0));
        assert_eq!(first_from(&[], 0), None);
    }
}
