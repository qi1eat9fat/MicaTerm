//! 新标签页（26 号「新标签页」，照 Zed `workspace/src/welcome.rs` 收窄）：一个标签都没有时
//! （启动时，或关掉最后一个标签后）标签区显示它，标签栏这时是空的。取代原来的开始卡片。
//!
//! - 上面一个极淡的大标志（没有标题和标语）；
//! - 下面一列宽 300、行高 32：图标 + 名字，快捷键靠右（等宽、`Ctrl+Shift+T` 写法）；
//!   没有默认底色，悬停才出底色；没有最近连接，没有「导入 SSH 配置」（用户定）。
//!
//! 没有标签时它持有焦点（`new_tab_focus`），快捷键才有焦点路径可走；行经窗口壳的焦点派发 action。

use gpui_kit::component::Icon;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::*;

use crate::app::{CatalogIcon, MARK_THIN_ICON, NewHost, NewLocalTerminalTab};
use crate::i18n::t;
use crate::theme::space;
use crate::theme::{Tokens, TypeScale, size};
use crate::workspace::Workspace;

/// 大标志的边长（26 号 `.wd-logo`）。
const LOGO: Pixels = px(72.);
/// 标志和第一行之间。
const LOGO_GAP: Pixels = px(24.);
/// 整列往上提一点，看着在正中。
const LIFT: Pixels = px(40.);

impl Workspace {
    /// 新标签页。
    pub(super) fn render_new_tab_page(
        &self,
        target: &FocusHandle,
        tokens: &Tokens,
        window: &Window,
        cx: &App,
    ) -> impl IntoElement + use<> {
        let scale = TypeScale::current(cx);
        let row =
            |id: &'static str, icon: CatalogIcon, label: SharedString, action: Box<dyn Action>| {
                let keys = shortcut_text(action.as_ref(), window);
                render_row(id, icon, label, keys, action, target, tokens, scale)
            };
        div()
            .id("new-tab-page")
            .test_support()
            .track_focus(&self.new_tab_focus)
            .aria_label(t!("workspace.new_tab.title"))
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .bg(tokens.content)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(size::NEW_TAB_COLUMN)
                    .mb(LIFT)
                    .child(
                        div()
                            .flex()
                            .justify_center()
                            .mb(LOGO_GAP)
                            .text_color(tokens.line_strong)
                            .child(Icon::default().path(MARK_THIN_ICON).size(LOGO)),
                    )
                    .child(row(
                        "new-tab-new-host",
                        CatalogIcon::Plus,
                        t!("workspace.new_tab.new_host"),
                        Box::new(NewHost),
                    ))
                    .child(row(
                        "new-tab-local-terminal",
                        CatalogIcon::SquareTerminal,
                        t!("workspace.new_tab.local_terminal"),
                        Box::new(NewLocalTerminalTab),
                    )),
            )
    }
}

/// 快捷键的写法（用户定）：键之间用 `+`，如 `Ctrl+Shift+T`；macOS 上是 `⇧⌘N` 这样的符号。
fn shortcut_text(action: &dyn Action, window: &Window) -> Option<SharedString> {
    let binding = window.highest_precedence_binding_for_action(action)?;
    let stroke = binding.keystrokes().first()?;
    Some(Kbd::format(stroke.as_keystroke()).into())
}

/// 一行（26 号 `.wel .wbtn`）：图标单独居中；名字和快捷键行高都等于行高、盒子居中，再按两种字体的
/// 度量把快捷键挪到和名字同一条基线上（GPUI 的 `items_baseline` 对不齐两种字体）。
#[allow(clippy::too_many_arguments)]
fn render_row(
    id: &'static str,
    icon: CatalogIcon,
    label: SharedString,
    keys: Option<SharedString>,
    action: Box<dyn Action>,
    target: &FocusHandle,
    tokens: &Tokens,
    scale: TypeScale,
) -> impl IntoElement {
    let target = target.clone();
    let hover = tokens.hover;
    let text = tokens.text;
    div()
        .id(id)
        .test_support()
        .aria_label(label.clone())
        .flex()
        .items_center()
        .gap(space::SPACE_10)
        .h(size::NEW_TAB_ROW)
        .px(space::SPACE_8)
        .rounded(size::RADIUS_MD)
        .text_size(scale.body)
        .text_color(tokens.label)
        .cursor_pointer()
        .hover(move |style| style.bg(hover).text_color(text))
        .on_click(move |_, window, cx| target.dispatch_action(action.as_ref(), window, cx))
        .child(
            div()
                .flex_none()
                .text_color(tokens.icon)
                .child(Icon::new(icon).size(size::ICON_MD)),
        )
        .child(
            div()
                .flex_none()
                .line_height(size::NEW_TAB_ROW)
                .child(label),
        )
        .children(keys.map(|keys| {
            div()
                .id((id, 1usize))
                .test_support()
                .aria_label(keys.clone())
                .ml_auto()
                .pl(space::SPACE_12)
                .flex_none()
                .line_height(size::NEW_TAB_ROW)
                .text_size(scale.body)
                .text_color(tokens.st_dim)
                .child(keys)
        }))
}
