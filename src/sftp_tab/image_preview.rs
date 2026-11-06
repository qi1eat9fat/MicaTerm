// 参考 since2006/shell-rs @ b2a307a: src/explorer/image_preview.rs 改写
// 改造：不是单独的实体，缩放的状态归 `PreviewDialog`；工具放头部（原型 `previewDlg`），「适合窗口」「原图」是分段按钮；
// 底下是棋盘格（原型 `.pv .canvas`）。
//! 预览里的图片：放进画框居中，或者按倍数缩放，比画框大就能滚动。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::preview::PreviewDialog;
use crate::i18n::t;
use crate::theme::Tokens;

/// 按钮和按键走的倍数，相对图片自己的大小（100% 时一个像素一个点）。
const STEPS: [f32; 14] = [
    0.1, 0.25, 0.33, 0.5, 0.67, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0, 5.0, 8.0,
];
const MIN_SCALE: f32 = 0.1;
const MAX_SCALE: f32 = 8.0;

/// 棋盘格一格的边长（原型 `16px 16px` 是两格一个周期）。
const CHECKER: Pixels = px(8.);

/// 图片怎么摆：放进画框（不超过自己的大小），或者自己的倍数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Zoom {
    Fit,
    Scale(f32),
}

/// 整张图片放得进画框的倍数；小图片保持原样。
pub(super) fn fit_scale(frame: Size<Pixels>, image: Size<Pixels>) -> f32 {
    if image.width <= px(0.) || image.height <= px(0.) {
        return 1.0;
    }
    (frame.width / image.width)
        .min(frame.height / image.height)
        .min(1.0)
}

/// 比 `scale` 大的下一档。
pub(super) fn zoom_in(scale: f32) -> f32 {
    STEPS
        .into_iter()
        .find(|step| *step > scale + 0.001)
        .unwrap_or(MAX_SCALE)
}

/// 比 `scale` 小的下一档。
pub(super) fn zoom_out(scale: f32) -> f32 {
    STEPS
        .into_iter()
        .rev()
        .find(|step| *step < scale - 0.001)
        .unwrap_or(MIN_SCALE)
}

/// 滚动的范围：图片，图片比画框小时是画框，这样它留在正中。
pub(super) fn scroll_area(frame: Size<Pixels>, shown: Size<Pixels>) -> Size<Pixels> {
    size(shown.width.max(frame.width), shown.height.max(frame.height))
}

/// 范围变了（缩放）以后的滚动位置：画框正中的那一点还在正中。
pub(super) fn recenter(
    frame: Size<Pixels>,
    old_area: Size<Pixels>,
    new_area: Size<Pixels>,
    offset: Point<Pixels>,
) -> Point<Pixels> {
    let axis = |offset: Pixels, frame: Pixels, old: Pixels, new: Pixels| {
        let center = (-offset + frame / 2.) / old;
        let start = new * center - frame / 2.;
        -start.clamp(px(0.), (new - frame).max(px(0.)))
    };
    point(
        axis(offset.x, frame.width, old_area.width, new_area.width),
        axis(offset.y, frame.height, old_area.height, new_area.height),
    )
}

/// 「37%」。
pub(super) fn percent(scale: f32) -> String {
    format!("{}%", (scale * 100.).round())
}

/// 预览里图片的状态。
pub(super) struct ImageView {
    /// 解码的结果：解码时是 `None`。
    pub(super) decoded: Option<Result<Arc<RenderImage>, String>>,
    zoom: Zoom,
    scroll: ScrollHandle,
    /// 上次排版时画框的大小：放进画框、居中和滚动范围都按它算。
    frame: Rc<Cell<Option<Size<Pixels>>>>,
}

impl ImageView {
    pub(super) fn new() -> Self {
        Self {
            decoded: None,
            zoom: Zoom::Fit,
            scroll: ScrollHandle::new(),
            frame: Rc::new(Cell::new(None)),
        }
    }

    /// 图片自己的大小，一个像素一个点。还没解码或者解不开是 `None`。
    pub(super) fn pixels(&self) -> Option<Size<Pixels>> {
        let Some(Ok(decoded)) = &self.decoded else {
            return None;
        };
        let pixels = decoded.size(0);
        Some(size(
            px(u32::from(pixels.width) as f32),
            px(u32::from(pixels.height) as f32),
        ))
    }

    fn scale_in(&self, frame: Size<Pixels>, image: Size<Pixels>) -> f32 {
        match self.zoom {
            Zoom::Fit => fit_scale(frame, image),
            Zoom::Scale(scale) => scale,
        }
    }

    /// 现在的倍数；还没量过画框或者还没解码是 `None`。
    pub(super) fn scale(&self) -> Option<f32> {
        Some(self.scale_in(self.frame.get()?, self.pixels()?))
    }

    /// 分段按钮选中第几个：放进画框是 0，100% 是 1，别的倍数都不选。
    pub(super) fn mode(&self) -> usize {
        match self.zoom {
            Zoom::Fit => 0,
            Zoom::Scale(scale) if scale == 1.0 => 1,
            Zoom::Scale(_) => usize::MAX,
        }
    }

    /// 缩放，画框正中的东西留在正中。
    pub(super) fn set_zoom(&mut self, zoom: Zoom) {
        if let (Some(image), Some(frame)) = (self.pixels(), self.frame.get()) {
            let old = self.scale_in(frame, image);
            self.zoom = zoom;
            let new = self.scale_in(frame, image);
            let area =
                |scale: f32| scroll_area(frame, size(image.width * scale, image.height * scale));
            self.scroll
                .set_offset(recenter(frame, area(old), area(new), self.scroll.offset()));
        } else {
            self.zoom = zoom;
        }
    }

    pub(super) fn step_in(&mut self) {
        if let Some(scale) = self.scale() {
            self.set_zoom(Zoom::Scale(zoom_in(scale)));
        }
    }

    pub(super) fn step_out(&mut self) {
        if let Some(scale) = self.scale() {
            self.set_zoom(Zoom::Scale(zoom_out(scale)));
        }
    }

    /// 双击图片：在放进画框和 100% 之间切。
    fn toggle(&mut self) {
        self.set_zoom(if self.zoom == Zoom::Fit {
            Zoom::Scale(1.0)
        } else {
            Zoom::Fit
        });
    }

    /// Ctrl 加滚轮：按滚动的量缩放。
    fn zoom_by_wheel(&mut self, delta: Pixels) {
        if let Some(scale) = self.scale() {
            let scale = (scale * (1. + f32::from(delta) / 200.)).clamp(MIN_SCALE, MAX_SCALE);
            self.set_zoom(Zoom::Scale(scale));
        }
    }

    /// 画框和里面的图片。
    pub(super) fn render_frame(
        &self,
        tokens: &Tokens,
        window: &mut Window,
        cx: &mut Context<PreviewDialog>,
    ) -> impl IntoElement + use<> {
        let viewport = window.viewport_size();
        let pixels = self.pixels();
        // 第一次排版之前估一个画框，下一帧就量到了。
        let frame = self
            .frame
            .get()
            .unwrap_or_else(|| size(viewport.width - px(120.), viewport.height - px(126.)));
        let measured = self.frame.clone();
        let view = cx.entity().entity_id();
        let picture = pixels
            .map(|image| (image, self.scale_in(frame, image)))
            .zip(match &self.decoded {
                Some(Ok(decoded)) => Some(decoded.clone()),
                _ => None,
            })
            .map(|((image, scale), decoded)| {
                let shown = size(image.width * scale, image.height * scale);
                let area = scroll_area(frame, shown);
                div()
                    .w(area.width)
                    .h(area.height)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(ImageSource::Render(decoded))
                            .id("preview-picture")
                            .test_support()
                            .w(shown.width)
                            .h(shown.height)
                            .flex_none()
                            .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                                if event.click_count() == 2 {
                                    this.with_image(cx, ImageView::toggle);
                                }
                            })),
                    )
            });
        let checker = tokens.field;
        div()
            .id("preview-image")
            .test_support()
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .w_full()
            .rounded_b(px(14.))
            .overflow_hidden()
            // 画框的大小决定怎么放，量到新的大小下一帧生效；棋盘格只画这一个画框。
            .child(
                canvas(
                    move |bounds, window, _| {
                        if measured.get() != Some(bounds.size) {
                            measured.set(Some(bounds.size));
                            window.on_next_frame(move |_, cx| cx.notify(view));
                        }
                    },
                    move |bounds, _, window, _| paint_checker(bounds, checker, window),
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(
                div()
                    .id("preview-scroll")
                    .size_full()
                    .overflow_scroll()
                    .track_scroll(&self.scroll)
                    // Ctrl 加滚轮是缩放，不是滚动。
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        if !event.modifiers.secondary() {
                            return;
                        }
                        cx.stop_propagation();
                        let delta = event.delta.pixel_delta(px(20.)).y;
                        this.with_image(cx, |image| image.zoom_by_wheel(delta));
                    }))
                    .children(picture)
                    .when(pixels.is_none(), |scroll| {
                        let note = match &self.decoded {
                            Some(Err(_)) => t!("preview.undecodable"),
                            _ => t!("preview.decoding"),
                        };
                        scroll.child(
                            div()
                                .id("preview-note")
                                .test_support()
                                .aria_label(note.clone())
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(12.5))
                                .text_color(tokens.muted)
                                .child(note),
                        )
                    }),
            )
            .vertical_scrollbar(&self.scroll)
            .horizontal_scrollbar(&self.scroll)
    }
}

/// 棋盘格（原型 `.pv .canvas`）：看得出图片哪里透明。
fn paint_checker(bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let columns = (bounds.size.width / CHECKER).ceil() as usize;
    let rows = (bounds.size.height / CHECKER).ceil() as usize;
    for row in 0..rows {
        for column in (row % 2..columns).step_by(2) {
            let origin = bounds.origin + point(CHECKER * column as f32, CHECKER * row as f32);
            let cell = Bounds::new(origin, size(CHECKER, CHECKER)).intersect(&bounds);
            window.paint_quad(fill(cell, color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fit_scale, percent, recenter, scroll_area, zoom_in, zoom_out};
    use gpui_kit::{point, px, size};

    #[test]
    fn a_large_image_fits_and_a_small_one_keeps_its_size() {
        let frame = size(px(800.), px(600.));
        assert_eq!(fit_scale(frame, size(px(4000.), px(1000.))), 0.2);
        assert_eq!(fit_scale(frame, size(px(100.), px(3000.))), 0.2);
        assert_eq!(fit_scale(frame, size(px(10.), px(10.))), 1.0);
        assert_eq!(fit_scale(frame, size(px(0.), px(0.))), 1.0);
    }

    #[test]
    fn zooming_steps_up_and_down_within_bounds() {
        assert_eq!(zoom_in(0.2), 0.25);
        assert_eq!(zoom_in(1.0), 1.25);
        assert_eq!(zoom_in(8.0), 8.0);
        assert_eq!(zoom_out(1.0), 0.75);
        assert_eq!(zoom_out(0.3), 0.25);
        assert_eq!(zoom_out(0.1), 0.1);
        assert_eq!(percent(0.333), "33%");
    }

    #[test]
    fn the_middle_stays_in_the_middle_when_zooming() {
        let frame = size(px(100.), px(100.));
        // 小图片居中：没东西可滚。
        let small = scroll_area(frame, size(px(50.), px(20.)));
        assert_eq!(small, frame);
        // 从放进画框到两倍画框：范围的正中。
        let large = scroll_area(frame, size(px(200.), px(400.)));
        assert_eq!(
            recenter(frame, small, large, point(px(0.), px(0.))),
            point(px(-50.), px(-150.))
        );
        // 滚到底再缩小：位置不出界。
        assert_eq!(
            recenter(frame, large, small, point(px(-100.), px(-300.))),
            point(px(0.), px(0.))
        );
    }
}
