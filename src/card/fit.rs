//! 卡片顶行正中那一段（前台程序名）的让位（26 号「窄」，2026-11-14 用户定）。
//!
//! 正中那一段按整条居中，不随两边的长短挪动；离两边不到 12 就整段藏起来，不截成半截。
//! 宽度在 prepaint 里量（上一帧的），文字宽度在 render 时用文字系统排一遍。

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::*;

/// 正中那一段离两边至少留这么多。
const CLEARANCE: f32 = 12.;

/// 宽 `middle` 的一段放在宽 `bar` 的一条正中，碰不碰得到左边 `left`、右边 `right` 的东西。
/// 还没量过宽度（`bar` 为 0）时算放得下。
pub(super) fn centered_fits(bar: f32, left: f32, right: f32, middle: f32) -> bool {
    if bar <= 0. {
        return true;
    }
    let start = (bar - middle) / 2.;
    start >= left + CLEARANCE && start + middle <= bar - right - CLEARANCE
}

/// `text` 用当前界面字体、`size` 字号排出来有多宽。
pub(super) fn text_width(window: &Window, text: &str, size: Pixels) -> f32 {
    if text.is_empty() {
        return 0.;
    }
    let run = TextRun {
        len: text.len(),
        font: window.text_style().font(),
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text.to_string().into(), size, &[run], None);
    f32::from(line.width)
}

/// 量一条的宽度：铺满父元素的透明画布，prepaint 时记下宽度；变了就下一帧让 `entity` 重画。
pub(super) fn measure_width(width: Rc<Cell<f32>>, entity: EntityId) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            let measured = f32::from(bounds.size.width);
            if (width.get() - measured).abs() > 0.5 {
                width.set(measured);
                window.on_next_frame(move |_, cx| cx.notify(entity));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::centered_fits;

    #[test]
    fn the_middle_fits_when_it_keeps_clear_of_both_sides() {
        // 宽 400：正中 100 宽的一段从 150 到 250。
        assert!(centered_fits(400., 100., 100., 100.));
        assert!(centered_fits(400., 138., 138., 100.));
        assert!(!centered_fits(400., 139., 0., 100.));
        assert!(!centered_fits(400., 0., 139., 100.));
        // 还没量过：先放着。
        assert!(centered_fits(0., 500., 500., 100.));
    }
}
