//! Font registration at startup:
//! - Bundled UI font: Instrument Sans (400 / 600, OFL).
//! - Bundled monospace fallback: JetBrains Mono (400 / 700 / Italic / BoldItalic, OFL).
//! - Optional external Maple Mono CN `.ttf` files placed in `assets/fonts/` next to the binary
//!   (or workspace `assets/fonts/` during development) are loaded from disk without bloating the
//!   compiled executable.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use gpui_kit::App;

const BUNDLED_FONTS: [&[u8]; 6] = [
    include_bytes!("../../assets/fonts/InstrumentSans-400.ttf"),
    include_bytes!("../../assets/fonts/InstrumentSans-600.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
];

/// Registers bundled fonts and any external `MapleMono*.ttf` files found in `assets/fonts/`.
pub fn register(cx: &mut App) {
    let mut fonts: Vec<Cow<'static, [u8]>> = BUNDLED_FONTS
        .iter()
        .map(|font| Cow::Borrowed(*font))
        .collect();

    for dir in external_font_dirs() {
        load_maple_fonts_from(&dir, &mut fonts);
    }

    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("micaterm: cannot register fonts: {error}");
    }
}

fn external_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        dirs.push(parent.join("assets").join("fonts"));
    }
    let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("fonts");
    if !dirs.contains(&dev_dir) {
        dirs.push(dev_dir);
    }
    dirs
}

fn load_maple_fonts_from(dir: &Path, out: &mut Vec<Cow<'static, [u8]>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("MapleMono")
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf"))
            && let Ok(bytes) = std::fs::read(&path)
        {
            out.push(Cow::Owned(bytes));
        }
    }
}
