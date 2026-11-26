//! Fixed integer typography scale (see `docs/ui-standard.md`).
//!
//! Six steps anchored at `INTERFACE_FONT_SIZE = 12.0`:
//! - `xs` (10px): OS badge initial, micro badges.
//! - `sm` (11px): status bar, shortcut hints, validation notes, uppercase section headers, table headers.
//! - `body` (12px): default UI text (sidebar host names, inputs, buttons, menus, tabs, descriptions).
//! - `emphasis` (13px): settings/toggle row titles, dock panel titles.
//! - `title` (15px): all modal dialog titles.
//! - `page_title` (18px): settings page titles.
//!
//! Terminal and code editor font sizes are configured independently in terminal settings.

use std::ops::RangeInclusive;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Pixels, Rems, px, rems};

/// Base UI font size in logical pixels.
pub const INTERFACE_FONT_SIZE: f32 = 12.0;

const STEPS: RangeInclusive<f32> = 12.0..=20.0;

/// The 6-step UI typography scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeScale {
    /// Micro badges and OS tile initials (10px).
    pub xs: Pixels,
    /// Shortcut hints, validation errors, uppercase section headers, table headers (11px).
    pub sm: Pixels,
    /// Global UI body default: inputs, buttons, menus, tabs, status bar, descriptions (12px).
    pub body: Pixels,
    /// Sidebar host names, settings nav, settings/toggle row titles (13px).
    pub emphasis: Pixels,
    /// Modal dialog titles (15px).
    pub title: Pixels,
    /// Settings page titles (18px).
    pub page_title: Pixels,
}

impl TypeScale {
    /// Computes the scale from a base body size clamped into 12–20px.
    pub fn from_base(base: f32) -> Self {
        let base = if base.is_finite() {
            base.round().clamp(*STEPS.start(), *STEPS.end())
        } else {
            INTERFACE_FONT_SIZE
        };
        let delta = base - INTERFACE_FONT_SIZE;
        Self {
            xs: px(10. + delta),
            sm: px(11. + delta),
            body: px(base),
            emphasis: px(13. + delta),
            title: px(15. + delta),
            page_title: px((base * 1.5).round()),
        }
    }

    /// Active scale derived from `gpui-component`'s theme font size.
    pub fn current(cx: &App) -> Self {
        Self::from_base(f32::from(cx.theme().font_size))
    }
}

/// Body text size as `1rem` (matches `TypeScale::body` set on `Root`).
pub fn ui_text() -> Rems {
    rems(1.)
}

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::TypeScale;

    #[test]
    fn default_step_matches_standard() {
        let scale = TypeScale::from_base(12.);
        assert_eq!(scale.xs, px(10.));
        assert_eq!(scale.sm, px(11.));
        assert_eq!(scale.body, px(12.));
        assert_eq!(scale.emphasis, px(13.));
        assert_eq!(scale.title, px(15.));
        assert_eq!(scale.page_title, px(18.));
    }

    #[test]
    fn bases_outside_the_steps_are_brought_back() {
        assert_eq!(TypeScale::from_base(24.), TypeScale::from_base(20.));
        assert_eq!(TypeScale::from_base(9.), TypeScale::from_base(12.));
        assert_eq!(TypeScale::from_base(13.4), TypeScale::from_base(13.));
    }
}
