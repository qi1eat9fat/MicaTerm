//! 卡片底部状态栏（高 24）：左边是连接状态和详情，右边是编码、终端类型、尺寸。
//! 本地卡片写「本地终端」和 shell 版本；远程卡片写连接状态、`user@地址:端口`、延迟、系统，
//! 断开时状态和原因变红（工单 12）。

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use qishell_terminal::{TERM_NAME, TERMINAL_ENCODING};

use super::CardId;
use crate::theme::{STATUS_BAR_HEIGHT, Tokens};

/// 状态的颜色：在跑是绿的，正在连接是灰的，断开是红的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StatusTone {
    Ok,
    Pending,
    Bad,
}

/// 状态栏要显示的东西，render 前从卡片取好。
pub(super) struct StatusBar {
    /// 点后面的状态：「本地终端」「已连接」「已断开」。
    pub state: SharedString,
    pub tone: StatusTone,
    /// 状态后面的详情：本地卡片是 shell 版本；远程卡片是地址、延迟、系统。
    pub details: Vec<SharedString>,
    /// 断开的原因（红字）。
    pub reason: Option<SharedString>,
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
        parts.extend(self.details.iter().map(ToString::to_string));
        parts.extend(self.reason.as_ref().map(ToString::to_string));
        parts.extend([
            TERMINAL_ENCODING.to_string(),
            TERM_NAME.to_string(),
            self.size(),
        ]);
        parts.join(" · ")
    }
}

/// 状态栏的一段：卡片窄得放不下时和别的段一起缩，写不下的用「…」截断。
fn segment(text: impl Into<SharedString>) -> Div {
    div().min_w_0().truncate().child(text.into())
}

/// 状态栏。卡片窄得放不下时每一段都缩短、末尾写「…」，不溢出卡片。
pub(super) fn render_status_bar(
    id: CardId,
    status: &StatusBar,
    tokens: &Tokens,
) -> impl IntoElement {
    let state_color = match status.tone {
        StatusTone::Ok => tokens.busy,
        StatusTone::Pending => tokens.faint,
        StatusTone::Bad => tokens.bad,
    };
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
                .id(id.element("card-status-state"))
                .test_support()
                .min_w_0()
                .truncate()
                .text_color(state_color)
                .child(format!("● {}", status.state)),
        )
        .when(
            !status.details.is_empty() || status.reason.is_some(),
            |bar| {
                bar.child(
                    div()
                        .id(id.element("card-status-detail"))
                        .test_support()
                        .flex()
                        .min_w_0()
                        .overflow_hidden()
                        .gap(px(12.))
                        .children(status.details.iter().map(|detail| segment(detail.clone())))
                        .when_some(status.reason.clone(), |details, reason| {
                            details.child(segment(reason).text_color(tokens.bad))
                        }),
                )
            },
        )
        .child(
            div()
                .id(id.element("card-status-terminal"))
                .test_support()
                .ml_auto()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .gap(px(12.))
                .child(segment(TERMINAL_ENCODING))
                .child(segment(TERM_NAME))
                .child(segment(status.size())),
        )
}

#[cfg(test)]
mod tests {
    use super::{StatusBar, StatusTone};

    #[test]
    fn the_label_reads_the_bar_from_left_to_right() {
        let bar = StatusBar {
            state: "本地终端".into(),
            tone: StatusTone::Ok,
            details: vec!["PowerShell 7.5.2".into()],
            reason: None,
            columns: 96,
            rows: 18,
        };
        assert_eq!(
            bar.label(),
            "本地终端 · PowerShell 7.5.2 · UTF-8 · xterm-256color · 96×18"
        );
        let unknown = StatusBar {
            details: Vec::new(),
            ..bar
        };
        assert_eq!(unknown.label(), "本地终端 · UTF-8 · xterm-256color · 96×18");
        let lost = StatusBar {
            state: "已断开".into(),
            tone: StatusTone::Bad,
            details: vec!["root@10.0.0.1:22".into()],
            reason: Some("连接已断开".into()),
            ..unknown
        };
        assert_eq!(
            lost.label(),
            "已断开 · root@10.0.0.1:22 · 连接已断开 · UTF-8 · xterm-256color · 96×18"
        );
    }
}
