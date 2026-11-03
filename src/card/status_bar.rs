//! 卡片底部状态栏（高 24）：左边是连接状态和详情（本地卡片写「本地终端」和 shell 版本），
//! 右边是编码、终端类型、尺寸。远程卡片的地址、延迟、系统在 M2（工单 12）加。

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use qishell_terminal::{TERM_NAME, TERMINAL_ENCODING};

use super::CardId;
use crate::theme::{STATUS_BAR_HEIGHT, Tokens};

/// 状态栏要显示的东西，render 前从卡片取好。
pub(super) struct StatusBar {
    /// 绿点后面的状态：「本地终端」。
    pub state: SharedString,
    /// 状态后面的详情：shell 版本，还没查到时没有。
    pub detail: Option<SharedString>,
    pub columns: usize,
    pub rows: usize,
}

impl StatusBar {
    /// 右边的尺寸：「96×18」。
    fn size(&self) -> String {
        format!("{}×{}", self.columns, self.rows)
    }

    /// 无障碍名称：状态栏上从左到右的每一段，用「 · 」隔开。测试靠它断言。
    fn label(&self) -> String {
        let mut parts: Vec<String> = vec![self.state.to_string()];
        parts.extend(self.detail.as_ref().map(ToString::to_string));
        parts.extend([
            TERMINAL_ENCODING.to_string(),
            TERM_NAME.to_string(),
            self.size(),
        ]);
        parts.join(" · ")
    }
}

pub(super) fn render_status_bar(
    id: CardId,
    status: &StatusBar,
    tokens: &Tokens,
) -> impl IntoElement {
    div()
        .id(id.element("card-status"))
        .test_support()
        .aria_label(status.label())
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .h(STATUS_BAR_HEIGHT)
        .pl(px(12.))
        .pr(px(10.))
        .text_size(px(11.))
        .text_color(tokens.faint)
        .border_t_1()
        .border_color(tokens.line)
        .whitespace_nowrap()
        .overflow_hidden()
        .child(
            div()
                .text_color(tokens.busy)
                .child(format!("● {}", status.state)),
        )
        .when_some(status.detail.clone(), |bar, detail| bar.child(detail))
        .child(
            div()
                .ml_auto()
                .flex()
                .gap(px(12.))
                .child(TERMINAL_ENCODING)
                .child(TERM_NAME)
                .child(status.size()),
        )
}

#[cfg(test)]
mod tests {
    use super::StatusBar;

    #[test]
    fn the_label_reads_the_bar_from_left_to_right() {
        let bar = StatusBar {
            state: "本地终端".into(),
            detail: Some("PowerShell 7.5.2".into()),
            columns: 96,
            rows: 18,
        };
        assert_eq!(
            bar.label(),
            "本地终端 · PowerShell 7.5.2 · UTF-8 · xterm-256color · 96×18"
        );
        let unknown = StatusBar {
            detail: None,
            ..bar
        };
        assert_eq!(unknown.label(), "本地终端 · UTF-8 · xterm-256color · 96×18");
    }
}
