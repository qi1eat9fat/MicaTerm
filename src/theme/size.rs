//! Fixed logical-pixel metrics aligned with `docs/ui-standard.md` and Muxy's `Metrics`.
//! Every constant here must be referenced by the UI; zero dead constants are allowed.

use gpui_kit::{Pixels, px};

// ── Border Radii (4 levels) ──────────────────────────────────────────────────

/// Small icon buttons, micro badges, OS tiles.
pub const RADIUS_SM: Pixels = px(4.);
/// Standard buttons, inputs, selects, menu rows, list rows, nav items, kbd badges, tooltips.
pub const RADIUS_MD: Pixels = px(6.);
/// Context menus, dropdown popovers, segmented frames, find cards.
pub const RADIUS_LG: Pixels = px(8.);
/// All modal dialogs and form cards.
pub const RADIUS_MODAL: Pixels = px(12.);

// ── Controls & Icons ─────────────────────────────────────────────────────────

/// Compact icon button & small control height (20px).
pub const CONTROL_SM: Pixels = px(20.);
/// Default icon button & compact toolbar button height (24px).
pub const CONTROL_MD: Pixels = px(24.);
/// Standard form/modal button height (28px).
pub const CONTROL_LG: Pixels = px(28.);
/// Small icon size (12px).
pub const ICON_SM: Pixels = px(12.);
/// Default icon size (14px).
pub const ICON_MD: Pixels = px(14.);
/// Large icon size (16px).
pub const ICON_LG: Pixels = px(16.);
/// Standard text input / select height (28px).
pub const INPUT: Pixels = px(28.);
/// Compact search input height (20px, aligned with `CONTROL_SM`).
pub const SEARCH_INPUT: Pixels = px(20.);
/// Segmented control item height (22px).
pub const SEGMENT_ITEM: Pixels = px(22.);
/// Checkbox square side (12px).
pub const CHECKBOX: Pixels = px(12.);

// ── Window Shell & Layout ────────────────────────────────────────────────────

/// Hairline divider thickness (1px).
pub const LINE: Pixels = px(1.);
/// Title bar and tab bar height (24px).
pub const TAB_BAR: Pixels = px(24.);
/// Reserved width for window caption buttons / macOS traffic lights (72px).
pub const TRAFFIC_LIGHTS_WIDTH: Pixels = px(72.);
/// Fixed tab width; overflows scroll horizontally (152px).
pub const TAB_WIDTH: Pixels = px(152.);
/// Split terminal / card header row height (22px).
pub const CARD_TITLE: Pixels = px(22.);
/// Bottom status bar height (22px).
pub const STATUS_BAR: Pixels = px(22.);
/// Left host sidebar width (expanded / collapsed).
pub const SIDEBAR_WIDTH: Pixels = px(240.);
pub const SIDEBAR_COLLAPSED: Pixels = px(32.);
/// Host sidebar list row height (30px).
pub const SIDEBAR_ROW: Pixels = px(30.);
/// Right dock panel list / tree row height (22px).
pub const DOCK_ROW: Pixels = px(22.);
/// Right dock panel two-line item height (Snippets, 36px).
pub const SNIPPET_ROW: Pixels = px(36.);
/// Right dock panel default width and header height (380px / 24px).
pub const DOCK_PANEL_WIDTH: Pixels = px(380.);
pub const DOCK_HEADER: Pixels = px(24.);
/// Settings page left nav width (aligned with `TAB_WIDTH`), item height, and max content width.
pub const SETTINGS_NAV_WIDTH: Pixels = TAB_WIDTH;
pub const SETTINGS_NAV_ITEM: Pixels = px(24.);
pub const SETTINGS_CONTENT_MAX: Pixels = px(680.);
/// New tab page column width and row height.
pub const NEW_TAB_COLUMN: Pixels = px(300.);
pub const NEW_TAB_ROW: Pixels = px(32.);
/// OS badge tile sizes: host sidebar (22px) and tab / card title / card picker (16px).
pub const HOST_BADGE: Pixels = px(22.);
pub const TAB_BADGE: Pixels = px(16.);
/// Glyph inside a `TAB_BADGE` tile (10px): tabs, card titles and the card picker.
pub const TAB_BADGE_GLYPH: Pixels = px(10.);
/// Glyph inside a `HOST_BADGE` tile (12px): the taller host-row badge keeps its logo
/// one step smaller than the tab tiles so it does not crowd the 22px square.
pub const HOST_BADGE_GLYPH: Pixels = px(12.);
/// Connected / tab status indicator dot diameter (5px).
pub const DOT: Pixels = px(5.);

// ── Overlays (Tooltips) ──────────────────────────────────────────────────────

/// Tooltip widest content box before text wraps, and the most lines it shows (320px / 4).
/// Long commands, paths and keys wrap here instead of stretching the tooltip across the window.
pub const TOOLTIP_MAX_WIDTH: Pixels = px(320.);
pub const TOOLTIP_MAX_LINES: usize = 4;

// ── Modals, Menus & Tables ───────────────────────────────────────────────────

/// Compact modal width (confirmations, prompts, credentials, snippets, SFTP) (400px).
pub const MODAL_SM: Pixels = px(400.);
/// Form modal width (Add/Edit Host) (480px).
pub const MODAL_MD: Pixels = px(480.);
/// Modal top offset from window edge (64px).
pub const MODAL_TOP: Pixels = px(64.);
/// Context menu geometry (min width 160px, row 20px, outer padding 4px).
pub const MENU_MIN_WIDTH: Pixels = px(160.);
pub const MENU_ROW: Pixels = px(20.);
pub const MENU_PADDING: Pixels = px(4.);
/// Data table header and row heights (24px / 26px).
pub const TABLE_HEADER: Pixels = px(24.);
pub const TABLE_ROW: Pixels = px(26.);
