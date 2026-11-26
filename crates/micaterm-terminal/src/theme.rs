// 来自 since2006/shell-rs @ b2a307a: src/terminal/theme.rs（裁剪：主题换成 MicaTerm 的 4 对）

//! 终端主题: the colors a terminal draws with — its text, background,
//! cursor, selection and the 16 ANSI colors. The settings choose one for the
//! light appearance and one for the dark and set the one in effect as a
//! global; every terminal view reads it, so a change reaches open tabs at
//! once. Also what a program is told when it asks for a color (OSC 4, 10,
//! 11, 12).

use alacritty_terminal::vte::ansi::{NamedColor, Rgb};
use gpui_kit::component::{ActiveTheme as _, ThemeMode};
use gpui_kit::*;

/// A built-in theme. Its colors are `0xrrggbb`: data, like a picture's, not
/// theme roles, so they are the same whatever the app's appearance.
#[derive(Debug, PartialEq, Eq)]
pub struct TerminalTheme {
    /// What the settings file stores. Never changes once released.
    key: &'static str,
    name: &'static str,
    /// The appearance it is made for: the column it is listed in.
    mode: ThemeMode,
    foreground: u32,
    background: u32,
    cursor: u32,
    /// Opaque: drawn under selected text in place of its background.
    selection: u32,
    /// Black, red, green, yellow, blue, magenta, cyan, white, then their
    /// bright versions.
    ansi: [u32; 16],
}

impl TerminalTheme {
    /// The built-in themes: the light ones, then the dark ones.
    pub fn all() -> &'static [TerminalTheme] {
        &THEMES
    }

    /// The themes made for `mode`, in the order they are listed.
    pub fn for_mode(mode: ThemeMode) -> impl Iterator<Item = &'static TerminalTheme> {
        THEMES.iter().filter(move |theme| theme.mode == mode)
    }

    pub fn find(key: &str) -> Option<&'static TerminalTheme> {
        THEMES.iter().find(|theme| theme.key == key)
    }

    /// What terminals use for `mode` until another is chosen: the look they
    /// had before themes could be chosen, the app's own background and text.
    pub fn default_for(mode: ThemeMode) -> &'static TerminalTheme {
        Self::for_mode(mode)
            .next()
            .expect("each appearance has its themes")
    }

    pub fn key(&self) -> &'static str {
        self.key
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn mode(&self) -> ThemeMode {
        self.mode
    }

    pub fn foreground(&self) -> Hsla {
        rgb(self.foreground).into()
    }

    pub fn background(&self) -> Hsla {
        rgb(self.background).into()
    }

    pub fn cursor(&self) -> Hsla {
        rgb(self.cursor).into()
    }

    pub fn selection(&self) -> Hsla {
        rgb(self.selection).into()
    }

    /// One of the 256 colors a program picks by number: the 16 ANSI colors
    /// are the theme's, the 6×6×6 cube and the gray ramp above them xterm's.
    pub fn indexed(&self, index: u8) -> Hsla {
        rgb(self.indexed_rgb(index)).into()
    }

    /// The answer to a program asking for color `index` it has not set
    /// itself, numbered as alacritty numbers its colors: 0–255 by number,
    /// then the named ones (foreground, background, cursor …).
    pub fn query_color(&self, index: usize) -> Rgb {
        let value = match index {
            0..=255 => self.indexed_rgb(index as u8),
            _ if index == NamedColor::Background as usize => self.background,
            _ if index == NamedColor::Cursor as usize => self.cursor,
            // The foreground, and the dim and bright versions of it and of
            // the ANSI colors, which programs cannot ask for by OSC.
            _ => self.foreground,
        };
        Rgb {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        }
    }

    fn indexed_rgb(&self, index: u8) -> u32 {
        match index {
            0..=15 => self.ansi[index as usize],
            16..=231 => {
                let index = index - 16;
                let component = |value: u8| {
                    if value == 0 {
                        0
                    } else {
                        55 + value as u32 * 40
                    }
                };
                (component(index / 36) << 16)
                    | (component((index % 36) / 6) << 8)
                    | component(index % 6)
            }
            232..=255 => {
                let value = 8 + (index as u32 - 232) * 10;
                (value << 16) | (value << 8) | value
            }
        }
    }
}

/// The theme terminals draw with now, set by the settings for the app's
/// current appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalColors {
    theme: &'static TerminalTheme,
    /// 盖掉主题自己的背景：MicaTerm 的终端背景跟着界面的内容底走，主题只管前景和 ANSI 色
    /// （26 号原型，用户定），标签栏、状态栏才不会撞色。
    background: Option<Hsla>,
}

impl Global for TerminalColors {}

impl TerminalColors {
    pub fn new(theme: &'static TerminalTheme) -> Self {
        Self {
            theme,
            background: None,
        }
    }

    /// 用 `background` 代替主题的背景。
    pub fn on_background(mut self, background: Hsla) -> Self {
        self.background = Some(background);
        self
    }

    /// The active terminal color configuration (theme plus optional background override).
    pub fn active(cx: &App) -> Self {
        cx.try_global::<Self>()
            .copied()
            .unwrap_or_else(|| Self::new(TerminalTheme::default_for(cx.theme().mode)))
    }

    /// The theme in effect; before the settings set one, the default for
    /// the app's appearance.
    pub fn current(cx: &App) -> &'static TerminalTheme {
        Self::active(cx).theme
    }

    /// The background terminals draw on now: the app's, when it set one,
    /// else the theme's.
    pub fn background(cx: &App) -> Hsla {
        let active = Self::active(cx);
        active
            .background
            .unwrap_or_else(|| active.theme.background())
    }

    /// Answer a program's color query (OSC 4 / 10 / 11 / 12): reports the app's
    /// overridden background for OSC 11 when set, otherwise delegates to the theme.
    pub fn query_color(&self, index: usize) -> Rgb {
        if index == NamedColor::Background as usize
            && let Some(background) = self.background
        {
            let rgba = background.to_rgb();
            let byte = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
            return Rgb {
                r: byte(rgba.r),
                g: byte(rgba.g),
                b: byte(rgba.b),
            };
        }
        self.theme.query_color(index)
    }

    pub fn theme(&self) -> &'static TerminalTheme {
        self.theme
    }
}

// 内置 4 对主题：Air Neutral Light / Dark（默认，与界面 Air Neutral 无缝一体）、
// Tokyo Day / Night、Catppuccin Latte / Mocha、Rosé Pine Dawn / Moon。
// key 写进 `settings.json`，发布后不能改：Rosé Pine Moon 的 key 沿用 `rose-pine`；
// 旧配置里存着已移除的 `ayu-light` / `ayu-dark` 或 `micaterm-*` 时会自动落回默认 Air Neutral。

/// Air Neutral Light：专为 Air Neutral 浅色纯白画布校准的清透 16 色调色板，
/// 普通色与亮色有清晰阶梯，全部核心 ANSI 色在 `#FFFFFF` 上可读。
const AIR_LIGHT: TerminalTheme = TerminalTheme {
    key: "air-light",
    name: "Air Neutral Light",
    mode: ThemeMode::Light,
    foreground: 0x14171f,
    background: 0xffffff,
    cursor: 0x2563eb,
    selection: 0xdbeafe,
    ansi: [
        0x1e222b, 0xcf303c, 0x157f49, 0x9e6007, 0x2563eb, 0x8c3fd1, 0x0e7c86, 0x5c6473, 0x6b7280,
        0xe0424e, 0x169454, 0xb56d05, 0x3b76f6, 0xa152e6, 0x12919c, 0x374151,
    ],
};

/// Air Neutral Dark：专为 Air Neutral 黑曜石底色 `#101216` 校准的清透 16 色调色板，
/// 补全纯正紫红通道（ANSI 5/13）与可读暗黑（ANSI 0），亮色阶梯鲜明。
const AIR_DARK: TerminalTheme = TerminalTheme {
    key: "air-dark",
    name: "Air Neutral Dark",
    mode: ThemeMode::Dark,
    foreground: 0xeceff4,
    background: 0x101216,
    cursor: 0x5b9bf8,
    selection: 0x24344d,
    ansi: [
        0x2a2f3a, 0xf05c65, 0x38c17a, 0xeab348, 0x5b9bf8, 0xc084fc, 0x3ec6d3, 0xcbd2dd, 0x6c7484,
        0xff7d85, 0x5ee09b, 0xf7cd6b, 0x82b7ff, 0xd8a8ff, 0x6ce0eb, 0xffffff,
    ],
};

/// 每一栏按列出的顺序，默认的在前；同一行是同一主题的浅色版和深色版。
static THEMES: [TerminalTheme; 8] = [
    AIR_LIGHT,
    TOKYO_DAY,
    CATPPUCCIN_LATTE,
    ROSE_PINE_DAWN,
    AIR_DARK,
    TOKYO_NIGHT,
    CATPPUCCIN_MOCHA,
    ROSE_PINE,
];

// 其余各取自己项目的终端配色，并确保 Catppuccin / Rosé Pine 的亮色（ANSI 9–14）与普通色（1–6）有清晰区分。

/// https://raw.githubusercontent.com/folke/tokyonight.nvim/main/extras/ghostty/tokyonight_day (Apache-2.0).
/// The project calls it Tokyo Night Day.
const TOKYO_DAY: TerminalTheme = TerminalTheme {
    key: "tokyo-day",
    name: "Tokyo Day",
    mode: ThemeMode::Light,
    foreground: 0x3760bf,
    background: 0xe1e2e7,
    cursor: 0x3760bf,
    selection: 0xb7c1e3,
    ansi: [
        0xb4b5b9, 0xf52a65, 0x587539, 0x8c6c3e, 0x2e7de9, 0x9854f1, 0x007197, 0x6172b0, 0xa1a6c5,
        0xff4774, 0x5c8524, 0xa27629, 0x358aff, 0xa463ff, 0x007ea8, 0x3760bf,
    ],
};

/// https://raw.githubusercontent.com/catppuccin/ghostty/main/themes/catppuccin-latte.conf (MIT).
/// 亮色 9–14 按同色相略微提亮提纯，避免与普通色 1–6 完全重合。
const CATPPUCCIN_LATTE: TerminalTheme = TerminalTheme {
    key: "catppuccin-latte",
    name: "Catppuccin Latte",
    mode: ThemeMode::Light,
    foreground: 0x4c4f69,
    background: 0xeff1f5,
    cursor: 0xdc8a78,
    selection: 0xd8dae1,
    ansi: [
        0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be, 0x6c6f85,
        0xe62950, 0x4fb336, 0xeb9e2f, 0x3878ff, 0xf28cd9, 0x22a8b0, 0xbcc0cc,
    ],
};

/// https://raw.githubusercontent.com/rose-pine/ghostty/main/dist/rose-pine-dawn (MIT).
/// 亮色 9–14 按同色相略微提亮，避免与普通色 1–6 完全重合。
const ROSE_PINE_DAWN: TerminalTheme = TerminalTheme {
    key: "rose-pine-dawn",
    name: "Rosé Pine Dawn",
    mode: ThemeMode::Light,
    foreground: 0x575279,
    background: 0xfaf4ed,
    cursor: 0x575279,
    selection: 0xdfdad9,
    ansi: [
        0xf2e9e1, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279, 0x9893a5,
        0xc9738c, 0x347d99, 0xf2ae4e, 0x67a8b3, 0xa58ec0, 0xe39490, 0x575279,
    ],
};

/// https://raw.githubusercontent.com/folke/tokyonight.nvim/main/extras/ghostty/tokyonight_night (Apache-2.0).
const TOKYO_NIGHT: TerminalTheme = TerminalTheme {
    key: "tokyo-night",
    name: "Tokyo Night",
    mode: ThemeMode::Dark,
    foreground: 0xc0caf5,
    background: 0x1a1b26,
    cursor: 0xc0caf5,
    selection: 0x283457,
    ansi: [
        0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6, 0x414868,
        0xff899d, 0x9fe044, 0xfaba4a, 0x8db0ff, 0xc7a9ff, 0xa4daff, 0xc0caf5,
    ],
};

/// https://raw.githubusercontent.com/catppuccin/ghostty/main/themes/catppuccin-mocha.conf (MIT).
/// 亮色 9–14 按同色相略微提亮，避免与普通色 1–6 完全重合。
const CATPPUCCIN_MOCHA: TerminalTheme = TerminalTheme {
    key: "catppuccin-mocha",
    name: "Catppuccin Mocha",
    mode: ThemeMode::Dark,
    foreground: 0xcdd6f4,
    background: 0x1e1e2e,
    cursor: 0xf5e0dc,
    selection: 0x353749,
    ansi: [
        0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8, 0x585b70,
        0xf8a5bc, 0xbeefba, 0xfcedc8, 0xa8c8ff, 0xfad4f0, 0xafeee4, 0xbac2de,
    ],
};

/// https://raw.githubusercontent.com/rose-pine/ghostty/main/dist/rose-pine-moon (MIT).
/// 深色用 Moon；key 沿用 `rose-pine`，亮色 9–14 按同色相略微提亮。
const ROSE_PINE: TerminalTheme = TerminalTheme {
    key: "rose-pine",
    name: "Rosé Pine Moon",
    mode: ThemeMode::Dark,
    foreground: 0xe0def4,
    background: 0x232136,
    cursor: 0xe0def4,
    selection: 0x44415a,
    ansi: [
        0x393552, 0xeb6f92, 0x3e8fb0, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xea9a97, 0xe0def4, 0x6e6a86,
        0xf489a8, 0x52a6c7, 0xfad296, 0xb5dfe6, 0xd6bef2, 0xf3b3b0, 0xe0def4,
    ],
};

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use alacritty_terminal::vte::ansi::{NamedColor, Rgb};
    use gpui_kit::component::ThemeMode;

    use super::TerminalTheme;

    /// WCAG relative luminance.
    fn luminance(color: u32) -> f32 {
        let channel = |shift: u32| {
            let value = ((color >> shift) & 0xff) as f32 / 255.;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(a: u32, b: u32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// 内置 4 对主题，浅色栏和深色栏按行配对，Air Neutral 在前（默认），Ayu 已移除。
    #[test]
    fn four_pairs_are_built_in_with_air_neutral_first() {
        let names = |mode| {
            TerminalTheme::for_mode(mode)
                .map(|theme| (theme.key, theme.name))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(ThemeMode::Light),
            [
                ("air-light", "Air Neutral Light"),
                ("tokyo-day", "Tokyo Day"),
                ("catppuccin-latte", "Catppuccin Latte"),
                ("rose-pine-dawn", "Rosé Pine Dawn"),
            ]
        );
        assert_eq!(
            names(ThemeMode::Dark),
            [
                ("air-dark", "Air Neutral Dark"),
                ("tokyo-night", "Tokyo Night"),
                ("catppuccin-mocha", "Catppuccin Mocha"),
                ("rose-pine", "Rosé Pine Moon"),
            ]
        );
        assert_eq!(TerminalTheme::all().len(), 8);
        assert_eq!(TerminalTheme::find("ayu-light"), None);
        assert_eq!(TerminalTheme::find("ayu-dark"), None);
        assert_eq!(
            TerminalTheme::default_for(ThemeMode::Light).key,
            "air-light"
        );
        assert_eq!(TerminalTheme::default_for(ThemeMode::Dark).key, "air-dark");
    }

    /// Air Neutral 这一对与界面底色/前景一致，且深浅 16 色均具备清晰紫红通道与普通/亮色阶梯。
    #[test]
    fn the_air_neutral_pair_matches_the_air_neutral_palette() {
        let light = TerminalTheme::find("air-light").unwrap();
        assert_eq!((light.background, light.foreground), (0xffffff, 0x14171f));
        assert_eq!(light.ansi[4], 0x2563eb);
        assert_eq!(light.ansi[5], 0x8c3fd1);
        let dark = TerminalTheme::find("air-dark").unwrap();
        assert_eq!((dark.background, dark.foreground), (0x101216, 0xeceff4));
        assert_eq!(dark.ansi[4], 0x5b9bf8);
        assert_eq!(dark.ansi[5], 0xc084fc);
    }

    /// 每一套内置主题的亮色（ANSI 9–14）都必须与普通色（ANSI 1–6）有区分，避免高亮层级丢失。
    #[test]
    fn every_theme_distinguishes_bright_from_normal_ansi_colors() {
        for theme in TerminalTheme::all() {
            for index in 1..=6 {
                assert_ne!(
                    theme.ansi[index],
                    theme.ansi[index + 8],
                    "{} bright ANSI {} duplicates normal ANSI {index}",
                    theme.name,
                    index + 8
                );
            }
        }
    }

    #[test]
    fn keys_are_unique_and_each_theme_suits_its_column() {
        let mut keys = HashSet::new();
        for theme in TerminalTheme::all() {
            assert!(keys.insert(theme.key), "{} twice", theme.key);
            assert_eq!(TerminalTheme::find(theme.key), Some(theme));
            assert_eq!(
                luminance(theme.background) > 0.5,
                theme.mode == ThemeMode::Light,
                "{} has a background of the other appearance",
                theme.name
            );
            // Solarized is soft by design, about 4:1; this catches a text
            // color swapped for another.
            assert!(
                contrast(theme.foreground, theme.background) >= 3.,
                "{}'s text is hard to read",
                theme.name
            );
        }
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let default = TerminalTheme::default_for(mode);
            assert_eq!(default.mode, mode);
            assert_eq!(TerminalTheme::for_mode(mode).next(), Some(default));
        }
    }

    #[test]
    fn the_default_themes_colors_read_on_their_background() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let theme = TerminalTheme::default_for(mode);
            // Black, white and their bright versions are meant to fade into
            // one background or the other.
            for index in (1..=6).chain(9..=14) {
                let ratio = contrast(theme.ansi[index], theme.background);
                assert!(ratio >= 3., "{} color {index}: {ratio:.2}", theme.name);
            }
            // zsh's suggestions and many prompts' quiet parts are bright
            // black.
            assert!(contrast(theme.ansi[8], theme.background) >= 3.);
        }
    }

    #[test]
    fn colors_above_the_sixteen_are_xterms() {
        let theme = TerminalTheme::default_for(ThemeMode::Light);
        assert_eq!(theme.indexed_rgb(16), 0x000000);
        assert_eq!(theme.indexed_rgb(21), 0x0000ff);
        assert_eq!(theme.indexed_rgb(196), 0xff0000);
        assert_eq!(theme.indexed_rgb(231), 0xffffff);
        assert_eq!(theme.indexed_rgb(232), 0x080808);
        assert_eq!(theme.indexed_rgb(255), 0xeeeeee);
        assert_eq!(theme.indexed_rgb(4), theme.ansi[4]);
    }

    #[test]
    fn programs_asking_for_colors_are_told_the_themes() {
        let light = TerminalTheme::default_for(ThemeMode::Light);
        let rgb = |value: u32| Rgb {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        };
        // vim asks for the background to choose its light or dark colors.
        assert_eq!(
            light.query_color(NamedColor::Background as usize),
            rgb(0xffffff)
        );
        assert_eq!(
            light.query_color(NamedColor::Foreground as usize),
            rgb(light.foreground)
        );
        assert_eq!(
            light.query_color(NamedColor::Cursor as usize),
            rgb(light.cursor)
        );
        assert_eq!(light.query_color(1), rgb(light.ansi[1]));
        assert_eq!(light.query_color(232), rgb(0x080808));
        let dark = TerminalTheme::default_for(ThemeMode::Dark);
        assert_eq!(
            dark.query_color(NamedColor::Background as usize),
            rgb(0x101216)
        );
    }

    #[test]
    fn overridden_background_is_reported_to_osc_11() {
        let colors = super::TerminalColors::new(TerminalTheme::default_for(ThemeMode::Dark))
            .on_background(gpui_kit::rgb(0x1a1b26).into());
        assert_eq!(
            colors.query_color(NamedColor::Background as usize),
            Rgb {
                r: 0x1a,
                g: 0x1b,
                b: 0x26,
            }
        );
        // Foreground and ANSI queries still come from the terminal theme itself.
        assert_eq!(
            colors.query_color(NamedColor::Foreground as usize),
            colors.theme().query_color(NamedColor::Foreground as usize)
        );
        assert_eq!(colors.query_color(1), colors.theme().query_color(1));
    }
}
