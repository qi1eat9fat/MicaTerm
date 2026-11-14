//! 每张卡片底部的状态栏（26 号 `.status`，照 Zed）：高 28、标签栏同色的底、上边一条细线、12px。
//! 名称、前台程序、分屏和关闭按钮在卡片顶行（`title_row`，2026-11-14 用户定）。
//!
//! - 左边：连接状态（绿点「已连接」+ 延迟、橙点「正在连接」、红点「已断开」+ 原因）；本地卡片没有。
//! - 正中：路径，不带图标，按整条居中；最多占四成宽，再长截断。
//! - 右边：系统、尺寸（列数 × 行数），用短竖线隔开。编码不再常驻（用户定）。
//! - 单击状态栏把焦点给这张卡片；双击放大或还原这张卡片。
//! - 卡片窄时让位，不让每段一起被截断：先藏系统，还碰就藏路径；再窄就去掉延迟、原因。
//!   连接状态和尺寸一直留着。
//! - 非焦点卡片的状态栏整条用淡色（`--st-dim`），和空心光标一起认出焦点卡片。

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::CardId;
use super::fit::{centered_fits, path_width, text_width};
use super::title_row::{ICON_GAP, Paint, ROW_LEFT, dot};
use crate::app::ZoomCard;
use crate::i18n::t;
use crate::theme::{Tokens, size};

/// 连接状态那一组。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum StatusState {
    /// 本地卡片：没有这一组。
    Local,
    /// 绿点「已连接」，后面是延迟。
    Connected { latency: Option<SharedString> },
    /// 橙点，写正在做什么：「正在连接」「等待解锁保险库」。
    Pending(SharedString),
    /// 红点「已断开」，后面是原因。
    Disconnected { reason: SharedString },
}

/// 状态栏要显示的东西，render 前从卡片取好。
pub(super) struct StatusBar {
    /// 当前目录：远程是远程报告的，本地是 shell 所在的（主目录写成 `~`）。
    pub directory: SharedString,
    pub state: StatusState,
    /// 远程主机的系统；本地卡片和还没探测出时是 `None`。
    pub os: Option<SharedString>,
    pub columns: usize,
    pub rows: usize,
}

/// 右边距。
const BAR_RIGHT: f32 = 10.;
/// 一组之间的距离（`.status` gap 10）、竖线的宽度。
const GROUP_GAP: f32 = 10.;

/// 这么宽时显示哪些。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shown {
    /// 延迟、原因。
    detail: bool,
    path: bool,
    system: bool,
}

/// 各段排出来有多宽（不含内边距）。
#[derive(Clone, Copy, Debug, Default)]
struct Widths {
    /// 状态那一组：点、字；没有是 0。
    state: f32,
    /// 延迟或原因；没有是 0。
    detail: f32,
    path: f32,
    system: f32,
    size: f32,
}

impl Shown {
    /// 宽 `bar` 的状态栏放得下哪些（还没量过时都放）。
    fn for_width(bar: f32, widths: Widths) -> Self {
        let left = |detail: bool| {
            ROW_LEFT
                + widths.state
                + if detail && widths.detail > 0. {
                    ICON_GAP + widths.detail
                } else {
                    0.
                }
        };
        let right = |system: bool| {
            BAR_RIGHT
                + widths.size
                + if system && widths.system > 0. {
                    GROUP_GAP * 2. + 1. + widths.system
                } else {
                    0.
                }
        };
        let detail = bar <= 0. || left(true) + right(false) + GROUP_GAP <= bar;
        let path = path_width(bar, widths.path);
        let fits = |system: bool| {
            widths.path > 0. && centered_fits(bar, left(detail), right(system), path)
        };
        if fits(true) {
            return Self {
                detail,
                path: true,
                system: true,
            };
        }
        if fits(false) {
            return Self {
                detail,
                path: true,
                system: false,
            };
        }
        // 没有路径（或放不下）：系统放得下就留着。
        let system = bar <= 0. || left(detail) + right(true) + GROUP_GAP <= bar;
        Self {
            detail,
            path: false,
            system: system && widths.path <= 0.,
        }
    }
}

impl StatusBar {
    /// 右边的尺寸：「96×18」。
    fn size(&self) -> String {
        format!("{}×{}", self.columns, self.rows)
    }

    /// 状态那一组的字：「已连接」「已断开」，本地卡片没有。
    fn state_text(&self) -> Option<SharedString> {
        match &self.state {
            StatusState::Local => None,
            StatusState::Connected { .. } => Some(t!("card.remote.connected")),
            StatusState::Pending(text) => Some(text.clone()),
            StatusState::Disconnected { .. } => Some(t!("card.remote.disconnected")),
        }
    }

    /// 状态后面的那一段：延迟或原因。
    fn state_detail(&self) -> Option<SharedString> {
        match &self.state {
            StatusState::Connected { latency } => latency.clone(),
            StatusState::Disconnected { reason } => Some(reason.clone()),
            _ => None,
        }
    }

    /// 无障碍名称：状态栏上能显示的每一段（不管现在窄不窄），用「 · 」隔开：
    /// 连接状态、路径、系统、尺寸。测试靠它断言。
    fn label(&self) -> String {
        let mut parts = Vec::new();
        if let Some(state) = self.state_text() {
            parts.push(match self.state_detail() {
                Some(detail) => format!("{state} {detail}"),
                None => state.to_string(),
            });
        }
        if !self.directory.is_empty() {
            parts.push(self.directory.to_string());
        }
        parts.extend(self.os.as_ref().map(ToString::to_string));
        parts.push(self.size());
        parts.join(" · ")
    }

    fn widths(&self, window: &Window, text_size: Pixels) -> Widths {
        let measure = |text: &str| text_width(window, text, text_size);
        Widths {
            state: self.state_text().map_or(0., |state| {
                f32::from(size::DOT) + ICON_GAP + measure(&state)
            }),
            detail: self.state_detail().map_or(0., |detail| measure(&detail)),
            path: measure(&self.directory),
            system: self.os.as_deref().map_or(0., measure),
            size: measure(&self.size()),
        }
    }
}

/// 一组之间的短竖线（`.status .vr`）。
fn rule(tokens: &Tokens) -> Div {
    div().flex_none().w(size::LINE).h(px(12.)).bg(tokens.line)
}

/// 画状态栏。`width` 是上一帧量到的卡片宽度（还没量过是 0）。
pub(super) fn render_status_bar(
    id: CardId,
    status: &StatusBar,
    focused: bool,
    width: f32,
    paint: &Paint,
) -> impl IntoElement {
    let Paint {
        target,
        window,
        tokens,
        scale,
    } = *paint;
    let shown = Shown::for_width(width, status.widths(window, scale.ui));
    // 非焦点卡片整条淡色；焦点卡片正文是次要色，状态用自己的颜色。
    let dim = !focused;
    let text = if dim { tokens.st_dim } else { tokens.text2 };
    let colored = |color: Hsla| if dim { tokens.st_dim } else { color };
    let state = status.state_text().map(|state| {
        let (dot_color, state_color) = match &status.state {
            StatusState::Connected { .. } => (tokens.ok, tokens.ok_text),
            StatusState::Disconnected { .. } => (tokens.err, tokens.err),
            _ => (tokens.warn, text),
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(ICON_GAP))
            .child(dot(dot_color))
            .child(div().text_color(colored(state_color)).child(state))
            .when_some(
                status.state_detail().filter(|_| shown.detail),
                |group, detail| group.child(detail),
            )
    });
    let mut right: Vec<SharedString> = Vec::new();
    if shown.system {
        right.extend(status.os.clone());
    }
    right.push(status.size().into());
    // 正中的路径最多占四成宽（还没量过时不限）。
    let path_limit = if width > 0. {
        px(width * 0.4)
    } else {
        Pixels::MAX
    };
    div()
        .id(id.element("card-status"))
        .test_support()
        .aria_label(status.label())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .h(size::STATUS_BAR)
        .pl(px(ROW_LEFT))
        .pr(px(BAR_RIGHT))
        .border_t(size::LINE)
        .border_color(tokens.line)
        .bg(tokens.tabbar)
        .text_size(scale.ui)
        .text_color(text)
        .whitespace_nowrap()
        .overflow_hidden()
        // 双击放大或还原这张卡片（标签里只有它时没反应）。
        .on_click({
            let target = target.clone();
            move |event, window, cx| {
                if event.click_count() == 2 {
                    target.dispatch_action(&ZoomCard(id), window, cx);
                }
            }
        })
        .child(
            div()
                .id(id.element("card-status-state"))
                .test_support()
                .flex()
                .min_w_0()
                .items_center()
                .children(state),
        )
        .when(shown.path && !status.directory.is_empty(), |bar| {
            bar.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .justify_center()
                    .items_center()
                    .child(
                        div()
                            .id(id.element("card-status-path"))
                            .test_support()
                            .min_w_0()
                            .max_w(path_limit)
                            .truncate()
                            .child(status.directory.clone()),
                    ),
            )
        })
        .child(
            div()
                .id(id.element("card-status-terminal"))
                .test_support()
                .ml_auto()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(GROUP_GAP))
                .children(right.into_iter().enumerate().flat_map(|(index, part)| {
                    let rule = (index > 0).then(|| rule(tokens).into_any_element());
                    rule.into_iter()
                        .chain(std::iter::once(div().child(part).into_any_element()))
                })),
        )
}

#[cfg(test)]
mod tests {
    use super::{Shown, StatusBar, StatusState, Widths};

    fn bar(state: StatusState) -> StatusBar {
        StatusBar {
            directory: "~/app".into(),
            state,
            os: Some("Ubuntu".into()),
            columns: 96,
            rows: 18,
        }
    }

    #[test]
    fn the_label_reads_the_bar_from_left_to_right() {
        let connected = bar(StatusState::Connected {
            latency: Some("12 ms".into()),
        });
        assert_eq!(connected.label(), "已连接 12 ms · ~/app · Ubuntu · 96×18");
        let lost = bar(StatusState::Disconnected {
            reason: "连接被对方重置".into(),
        });
        assert_eq!(
            lost.label(),
            "已断开 连接被对方重置 · ~/app · Ubuntu · 96×18"
        );
        let local = StatusBar {
            directory: "~".into(),
            os: None,
            ..bar(StatusState::Local)
        };
        assert_eq!(local.label(), "~ · 96×18");
    }

    const WIDTHS: Widths = Widths {
        state: 60.,
        detail: 40.,
        path: 60.,
        system: 120.,
        size: 40.,
    };

    #[test]
    fn a_narrow_card_hides_the_system_first_then_the_path_then_the_details() {
        // 左边 20+60+6+40=126；右边带系统 10+40+21+120=191，不带 50。
        // 路径 60 放正中：带系统要 2×(191+12)+60=466 宽，不带系统看左边要 2×(126+12)+60=336 宽。
        let wide = Shown::for_width(600., WIDTHS);
        assert!(wide.system && wide.path && wide.detail, "{wide:?}");
        let no_system = Shown::for_width(400., WIDTHS);
        assert!(!no_system.system && no_system.path && no_system.detail);
        let no_path = Shown::for_width(300., WIDTHS);
        assert!(!no_path.system && !no_path.path && no_path.detail);
        let bare = Shown::for_width(150., WIDTHS);
        assert!(!bare.detail && !bare.path && !bare.system);
        // 还没量过：都放。
        let unknown = Shown::for_width(0., WIDTHS);
        assert!(unknown.system && unknown.path && unknown.detail);
    }
}
