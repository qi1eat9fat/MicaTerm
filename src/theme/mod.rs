//! Visual design tokens: color (`Tokens`), dimensions (`size`, `space`), interaction
//! opacity (`opacity`), typography (`TypeScale`), elevation shadows (`Elevation`), and fonts.
//!
//! See `docs/ui-standard.md` for the full specification. Feature modules must consume values
//! from here instead of hardcoding colors, font sizes, radii, or shadows.

pub mod code_colors;
mod component;
pub mod elevation;
pub mod opacity;
pub mod size;
pub mod space;
mod tokens;
mod type_scale;

pub use component::{DARK_THEME_NAME, LIGHT_THEME_NAME, component_theme, component_theme_name};
pub use elevation::Elevation;
pub use tokens::Tokens;
pub(crate) use tokens::{dark_ink_on, on_color};
pub use type_scale::{INTERFACE_FONT_SIZE, TypeScale, ui_text};

use gpui_kit::{Font, FontFallbacks, FontWeight, Pixels, font};

/// Primary UI font family: Instrument Sans (bundled 400 / 600).
pub const UI_FONT_FAMILY: &str = "Instrument Sans";

/// Explicit CJK fallback for user data containing Chinese characters.
///
/// Microsoft YaHei UI has no 500 face and DirectWrite resolves a 500 request to Semibold 600
/// (unlike CSS, which falls back to 400), so every medium-weight Chinese label looked bold.
/// The UI therefore only uses two weights (400 / 600): see `WEIGHT_NORMAL` / `WEIGHT_SEMIBOLD`.
pub const UI_FONT_FALLBACK: &str = "Microsoft YaHei UI";

/// Monospace font family: Maple Mono Normal NL NF CN (terminal, editor, shortcuts, paths, IPs).
pub const MONO_FONT_FAMILY: &str = micaterm_terminal::DEFAULT_FONT_FAMILY;

/// Font weights (2 tiers: 400 / 600).
///
/// There is deliberately no 500 tier: DirectWrite maps a 500 request to YaHei UI's Semibold face
/// for Chinese, which looks much heavier than Instrument Sans Medium. Emphasis comes from size
/// and color instead.
pub const WEIGHT_NORMAL: FontWeight = FontWeight::NORMAL;
pub const WEIGHT_SEMIBOLD: FontWeight = FontWeight::SEMIBOLD;

/// Root UI font with explicit CJK fallback attached.
pub fn ui_font() -> Font {
    Font {
        fallbacks: Some(FontFallbacks::from_fonts(vec![UI_FONT_FALLBACK.into()])),
        ..font(UI_FONT_FAMILY)
    }
}

/// Computes the vertical offset needed to align the baseline of `to` to `from` within a shared
/// `line_height` box.
pub fn baseline_shift(
    window: &gpui_kit::Window,
    from: (&'static str, Pixels),
    to: (&'static str, Pixels),
    line_height: Pixels,
) -> Pixels {
    let text = window.text_system();
    let baseline = |(family, size): (&'static str, Pixels)| {
        let id = text.resolve_font(&font(family));
        text.baseline_offset(id, size, line_height)
    };
    baseline(from) - baseline(to)
}
