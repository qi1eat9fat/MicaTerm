//! Floating surface elevation shadows (aligned with `muxy-ui/src/theme/elevation.rs`).
//! Feature modules must use `Elevation::shadow` instead of constructing `BoxShadow` directly.

use gpui_kit::{BoxShadow, Hsla, hsla, point, px};

/// Elevation levels for floating UI layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// Context menus, dropdowns, tooltips, and find cards.
    Elevated,
    /// Full-window modal dialogs and form cards.
    Modal,
}

impl Elevation {
    /// Returns layered drop shadows adapted to the surface background lightness.
    pub fn shadow(self, background: Hsla) -> Vec<BoxShadow> {
        let light = background.l >= 0.5;
        match self {
            Self::Elevated => vec![
                layer(2.0, 3.0, if light { 0.08 } else { 0.16 }),
                layer(6.0, 16.0, if light { 0.10 } else { 0.30 }),
            ],
            Self::Modal => vec![
                layer(2.0, 4.0, if light { 0.06 } else { 0.14 }),
                layer(8.0, 20.0, if light { 0.10 } else { 0.24 }),
                layer(16.0, 40.0, if light { 0.12 } else { 0.32 }),
            ],
        }
    }

    /// Subtle halo ring around an active status dot.
    pub fn dot_halo(color: Hsla) -> Vec<BoxShadow> {
        vec![BoxShadow {
            color: color.opacity(0.22),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(2.5),
            inset: false,
        }]
    }
}

fn layer(offset: f32, blur: f32, opacity: f32) -> BoxShadow {
    BoxShadow {
        color: hsla(0.0, 0.0, 0.0, opacity),
        offset: point(px(0.0), px(offset)),
        blur_radius: px(blur),
        spread_radius: px(0.0),
        inset: false,
    }
}
