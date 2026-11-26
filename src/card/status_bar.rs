//! 每张卡片底部的状态栏（26 号 `.status`，照官网紧凑状态栏）：高 22、和内容同底、上边一条细线、11px 次要色。
//! 样式照原型的底部状态栏：每一段左右各留 7，段与段之间是贯通整条高度的 1px 竖线
//! （不是悬空的小短线），图标和字都是次要色。
//! 名称、前台程序、分屏和关闭按钮在卡片顶行（`title_row`，2026-11-14 用户定）。
//!
//! - 最左：路径（小文件夹 + 目录），最多占四成宽，再长截断。
//! - 接着：系统（「Rocky Linux」），跟在路径后面，不和右边抢地方。
//! - 最右：连接状态（绿点「已连接」+ 延迟、橙点「正在连接」、红点「已断开」+ 原因）；本地卡片没有。
//! - 列数 × 行数不再显示（2026-11-22 用户定）。
//! - 单击状态栏把焦点给这张卡片；双击放大或还原这张卡片。
//! - 卡片窄时让位：先去掉延迟，再藏系统；路径自己截断，状态一直留着。
//! - 非焦点卡片的状态栏整条用淡色（`--st-dim`），和空心光标一起认出焦点卡片。

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::CardId;
use super::fit::text_width;
use super::title_row::{ICON_GAP, Paint, dot};
use crate::app::{CatalogIcon, ZoomCard};
use crate::i18n::t;
use crate::theme::size;
use crate::theme::space;

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
    /// 交给 Agent 了（Agent CLI 工单 06）：`Some(正在执行命令)`；没交出去是 `None`。
    pub agent: Option<bool>,
}

/// 每一段左右的内边距（对齐原型与 Muxy 紧凑状态栏）。
const SEGMENT_PAD: f32 = 7.;
/// 路径前面小文件夹图标的边长、它和路径之间的距离。
const PATH_ICON: Pixels = px(10.5);
const PATH_GAP: f32 = 4.;
/// 路径最多占整条的四成，再长截断。
const PATH_SHARE: f32 = 0.4;

/// 这么宽时显示哪些。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shown {
    /// 延迟、原因。
    detail: bool,
    /// 路径后面的系统。
    system: bool,
}

/// 各段排出来有多宽（不含内边距）。
#[derive(Clone, Copy, Debug, Default)]
struct Widths {
    /// 状态那一组：点、字；没有是 0。
    state: f32,
    /// 延迟或原因；没有是 0。
    detail: f32,
    /// 路径那一组：文件夹、字；没有是 0。
    path: f32,
    system: f32,
}

impl Shown {
    /// 宽 `bar` 的状态栏放得下哪些（还没量过时都放）。
    fn for_width(bar: f32, widths: Widths) -> Self {
        if bar <= 0. {
            return Self {
                detail: true,
                system: true,
            };
        }
        // 路径最长占四成；系统、状态各自左右留白，状态左边还有一条分隔线。
        let path = 2. * SEGMENT_PAD + widths.path.min(bar * PATH_SHARE);
        let system = 2. * SEGMENT_PAD + widths.system + f32::from(size::LINE);
        let state = 2. * SEGMENT_PAD + widths.state + f32::from(size::LINE);
        let detail = state + ICON_GAP + widths.detail;
        if path + system + detail <= bar {
            Self {
                detail: true,
                system: true,
            }
        } else if path + system + state <= bar {
            // 地方不够：先去延迟。
            Self {
                detail: false,
                system: true,
            }
        } else if path + detail <= bar {
            // 再去系统，延迟放得下就留着。
            Self {
                detail: true,
                system: false,
            }
        } else {
            Self {
                detail: false,
                system: false,
            }
        }
    }
}

impl StatusBar {
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

    /// 状态栏左边「Agent」的小标记。
    fn agent_text(&self) -> Option<SharedString> {
        self.agent.map(|running| {
            if running {
                t!("card.status.agent_running")
            } else {
                t!("card.status.agent")
            }
        })
    }

    /// 无障碍名称：状态栏上能显示的每一段（不管现在窄不窄），用「 · 」隔开：
    /// 连接状态、路径、系统。测试靠它断言。
    fn label(&self) -> String {
        let mut parts = Vec::new();
        parts.extend(self.agent_text().map(|text| text.to_string()));
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
        parts.join(" · ")
    }

    fn widths(&self, window: &Window, text_size: Pixels) -> Widths {
        let measure = |text: &str| text_width(window, text, text_size);
        Widths {
            state: self.state_text().map_or(0., |state| {
                f32::from(size::DOT) + ICON_GAP + measure(&state)
            }),
            detail: self.state_detail().map_or(0., |detail| measure(&detail)),
            // 路径那一组含前面那个小文件夹图标。
            path: measure(&self.directory)
                + if self.directory.is_empty() {
                    0.
                } else {
                    f32::from(PATH_ICON) + PATH_GAP
                },
            system: self.os.as_deref().map_or(0., measure),
        }
    }
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
    let shown = Shown::for_width(width, status.widths(window, scale.sm));
    // 状态栏文字与官网原型一致用次要色 `text2`（65%），非焦点窗格再收淡到 `st_dim`。
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
            .min_w_0()
            .items_center()
            .gap(px(ICON_GAP))
            .child(dot(dot_color))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(colored(state_color))
                    .child(state),
            )
            .when_some(
                status.state_detail().filter(|_| shown.detail),
                |group, detail| group.child(div().min_w_0().truncate().child(detail)),
            )
    });
    // 路径那一整段（图标 + 目录）最多占四成宽（还没量过时不限）。
    let path_limit = if width > 0. {
        px(width * PATH_SHARE)
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
        .border_t(size::LINE)
        .border_color(tokens.line)
        .bg(tokens.tabbar)
        .text_size(scale.sm)
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
        .children(status.agent_text().map(|text| {
            div()
                .id(id.element("card-status-agent"))
                .test_support()
                .aria_label(text.clone())
                .flex_none()
                .ml(px(SEGMENT_PAD))
                .mr(px(SEGMENT_PAD))
                .px(space::SPACE_5)
                .rounded(size::RADIUS_SM)
                .bg(tokens.sel)
                .text_color(colored(tokens.text))
                .child(text)
        }))
        // 最左：路径。放不下时自己截断，不挤状态那一组。
        .when(!status.directory.is_empty(), |bar| {
            bar.child(
                div()
                    .id(id.element("card-status-path-group"))
                    .test_support()
                    .flex()
                    .flex_shrink(1.)
                    .min_w_0()
                    .items_center()
                    .h_full()
                    .gap(px(PATH_GAP))
                    .px(px(SEGMENT_PAD))
                    .max_w(path_limit)
                    // 原型状态栏的路径前面有一个小文件夹。字有下伸部，行框里字的视觉中心比
                    // 几何中心高：图标往上挪半个像素，和旁边 11px 的字对齐（2026-11-24 用户定）。
                    .child(
                        div()
                            .flex_none()
                            .relative()
                            .top(px(-0.5))
                            .child(Icon::new(CatalogIcon::Folder).size(PATH_ICON)),
                    )
                    .child(
                        div()
                            .id(id.element("card-status-path"))
                            .test_support()
                            .min_w_0()
                            .truncate()
                            .child(status.directory.clone()),
                    ),
            )
        })
        // 接着：系统。
        .when_some(status.os.clone().filter(|_| shown.system), |bar, os| {
            bar.child(
                div()
                    .id(id.element("card-status-system"))
                    .test_support()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h_full()
                    .px(px(SEGMENT_PAD))
                    .border_l(size::LINE)
                    .border_color(tokens.line)
                    .child(os),
            )
        })
        // 中间留空，状态靠最右；本地卡片没有状态，也就不画那一段和它前面的线。
        .child(div().flex_1().min_w_0())
        .children(state.map(|state| {
            div()
                .id(id.element("card-status-state"))
                .test_support()
                .flex()
                .flex_shrink(1.)
                .min_w_0()
                .items_center()
                .h_full()
                .px(px(SEGMENT_PAD))
                .border_l(size::LINE)
                .border_color(tokens.line)
                .child(state)
        }))
}

#[cfg(test)]
mod tests {
    use super::{Shown, StatusBar, StatusState, Widths};

    fn bar(state: StatusState) -> StatusBar {
        StatusBar {
            directory: "~/app".into(),
            state,
            os: Some("Ubuntu".into()),
            agent: None,
        }
    }

    #[test]
    fn the_label_reads_the_bar_from_left_to_right() {
        let connected = bar(StatusState::Connected {
            latency: Some("12 ms".into()),
        });
        assert_eq!(connected.label(), "Connected 12 ms · ~/app · Ubuntu");
        let lost = bar(StatusState::Disconnected {
            reason: "连接被对方重置".into(),
        });
        assert_eq!(lost.label(), "Disconnected 连接被对方重置 · ~/app · Ubuntu");
        let local = StatusBar {
            directory: "~".into(),
            os: None,
            ..bar(StatusState::Local)
        };
        assert_eq!(local.label(), "~");
    }

    /// 交给 Agent 的卡片（Agent CLI 工单 06）：标记在最左边，执行命令时写「执行中」。
    #[test]
    fn a_card_handed_to_the_agent_says_so_first() {
        let handed = StatusBar {
            agent: Some(false),
            ..bar(StatusState::Connected { latency: None })
        };
        assert!(
            handed.label().starts_with("Agent · Connected"),
            "{}",
            handed.label()
        );
        let running = StatusBar {
            agent: Some(true),
            ..bar(StatusState::Connected { latency: None })
        };
        assert!(
            running.label().starts_with("Agent Running · "),
            "{}",
            running.label()
        );
    }

    const WIDTHS: Widths = Widths {
        state: 60.,
        detail: 40.,
        path: 60.,
        system: 120.,
    };

    #[test]
    fn a_narrow_card_drops_the_latency_then_the_system() {
        // 路径段 14+60=74；系统段 14+120+1=135；状态段 14+60+1=75，带延迟再加 6+40。
        // 都放要 74+135+121=330；去延迟要 74+135+75=284；再去系统要 74+121=195。
        let wide = Shown::for_width(600., WIDTHS);
        assert!(wide.system && wide.detail, "{wide:?}");
        let roomy = Shown::for_width(400., WIDTHS);
        assert!(roomy.system && roomy.detail, "{roomy:?}");
        let no_detail = Shown::for_width(320., WIDTHS);
        assert!(no_detail.system && !no_detail.detail, "{no_detail:?}");
        let no_system = Shown::for_width(250., WIDTHS);
        assert!(!no_system.system && no_system.detail, "{no_system:?}");
        let bare = Shown::for_width(150., WIDTHS);
        assert!(!bare.system && !bare.detail, "{bare:?}");
        // 还没量过：都放。
        let unknown = Shown::for_width(0., WIDTHS);
        assert!(unknown.system && unknown.detail);
    }
}
