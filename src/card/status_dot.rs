//! 状态点：标签上名称、目录后面的那个点。
//!
//! - 绿点（有光晕，不做动画）：前台是 shell 以外的程序，且 1.5 秒内有输出。在提示符下打字的回显不算：
//!   那时前台是 shell 自己。
//! - 橙点：程序在看不见的地方结束，或者在看不见的地方响铃；卡片重新看得见时消失。
//! - 两个都有时以橙点为准。
// 参考 gorex 的模型重写：view.go 的 `statusOf`、`activityDot`，tabs.go 的标签上的点。

use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use crate::programs::Program;
use crate::theme::{Elevation, Tokens, size};

/// 有输出之后，绿点亮多久。
pub const ACTIVE_FOR: Duration = Duration::from_millis(1500);

/// 状态点的样子。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusDot {
    /// 不画点：在提示符下，或者程序没出声。
    #[default]
    None,
    /// 绿点：程序在输出。
    Busy,
    /// 橙点：在看不见的地方结束或响铃了。
    Attention,
}

impl StatusDot {
    /// 卡片的点。`attention`：卡片在看不见的地方结束或响铃过；`program`：前台程序（`None` 是 shell 在提示符下）；
    /// `output_at`：最近一次有输出的时候。
    pub fn of(
        attention: bool,
        program: Option<&Program>,
        output_at: Option<Instant>,
        now: Instant,
    ) -> Self {
        if attention {
            return Self::Attention;
        }
        match (program, output_at) {
            (Some(program), Some(at))
                if !program.shell && now.saturating_duration_since(at) < ACTIVE_FOR =>
            {
                Self::Busy
            }
            _ => Self::None,
        }
    }

    /// 标签的点：有一张卡片要注意就是橙点，否则有一张在输出就是绿点。
    pub fn of_tab(cards: impl IntoIterator<Item = Self>) -> Self {
        cards
            .into_iter()
            .fold(Self::None, |tab, card| match (tab, card) {
                (Self::Attention, _) | (_, Self::Attention) => Self::Attention,
                (Self::Busy, _) | (_, Self::Busy) => Self::Busy,
                _ => Self::None,
            })
    }
}

/// 画状态点（原型 `.dot.busy`、`.dot.attn`）；`None` 什么都不画。`id` 给测试查，无障碍名称写明是哪种。
pub fn render_status_dot(id: ElementId, dot: StatusDot, tokens: &Tokens) -> Option<AnyElement> {
    let (dot_size, color, label, halo) = match dot {
        StatusDot::None => return None,
        StatusDot::Busy => (size::DOT, tokens.ok, t!("card.dot.busy"), true),
        StatusDot::Attention => (size::DOT, tokens.warn, t!("card.dot.attention"), false),
    };
    Some(
        crate::ui::dot(color, dot_size)
            .when(halo, |dot| dot.shadow(Elevation::dot_halo(color)))
            .id(id)
            .test_support()
            .aria_label(label)
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::StatusDot;
    use crate::programs::program_of;

    #[test]
    fn a_program_printing_lately_shows_the_busy_dot() {
        let start = Instant::now();
        let vim = program_of("vim");
        let dot = |now| StatusDot::of(false, Some(&vim), Some(start), now);
        assert_eq!(dot(start), StatusDot::Busy);
        assert_eq!(dot(start + Duration::from_millis(1499)), StatusDot::Busy);
        // 1.5 秒没输出就熄灭。
        assert_eq!(dot(start + Duration::from_millis(1500)), StatusDot::None);
        // 一直没有输出过：不亮。
        assert_eq!(
            StatusDot::of(false, Some(&vim), None, start),
            StatusDot::None
        );
    }

    #[test]
    fn echo_at_the_prompt_is_no_output() {
        let now = Instant::now();
        // shell 自己在前台（提示符下打字）：有回显也不亮。
        assert_eq!(StatusDot::of(false, None, Some(now), now), StatusDot::None);
        let pwsh = program_of("pwsh");
        assert_eq!(
            StatusDot::of(false, Some(&pwsh), Some(now), now),
            StatusDot::None
        );
    }

    #[test]
    fn attention_wins_over_output() {
        let now = Instant::now();
        let vim = program_of("vim");
        assert_eq!(
            StatusDot::of(true, Some(&vim), Some(now), now),
            StatusDot::Attention
        );
        assert_eq!(StatusDot::of(true, None, None, now), StatusDot::Attention);
    }

    #[test]
    fn a_tab_shows_the_dot_its_cards_need_most() {
        use StatusDot::{Attention, Busy, None};
        assert_eq!(StatusDot::of_tab([None, None]), None);
        assert_eq!(StatusDot::of_tab([None, Busy]), Busy);
        assert_eq!(StatusDot::of_tab([Busy, Attention, None]), Attention);
        assert_eq!(StatusDot::of_tab([]), None);
    }
}
