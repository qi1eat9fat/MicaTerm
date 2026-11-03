//! 随包附带的字体：终端默认的 JetBrains Mono（OFL，见 `assets/fonts/OFL.txt`）。
//! 本机多半没装它，所以启动时注册进 GPUI 的字体系统。

use std::borrow::Cow;

use gpui_kit::App;

const FONTS: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
];

/// 把随包的字体注册进字体系统。注册失败只记一笔，界面照样能用（回落到别的等宽字体）。
pub fn register(cx: &mut App) {
    let fonts = FONTS.iter().map(|font| Cow::Borrowed(*font)).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("qishell: cannot register the bundled fonts: {error}");
    }
}
