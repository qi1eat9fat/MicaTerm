//! 颜色 token：Air Neutral（简约通透）明暗配色体系，所有界面语义色均由 `Scheme` 单一数据源推导。
//!
//! 浅色以纯白 `#FFFFFF` 为内容画布、极轻冷雪灰（前景 4.2%）为侧栏；深色以清冽黑曜石 `#101216` 为内容底、
//! 前景 6.5% 为侧栏。强调色采用清澈静谧蓝（深色 `#5B9BF8`、浅色 `#2563EB`），只用在主按钮、开关打开、
//! 分段选中、菜单悬停、激活标签下划线和焦点环；列表选中与标签激活保持轻中性色。
//!
//! 细线与浮层描边在明暗下分别采用轻量化比例（浅色 7% / 14%、深色 8% / 16%），
//! 浮层（菜单、对话框）深色用 `#1B1F27` 抬升一级、浅色用纯白配柔和阴影，输入框用前景 3.5% / 4.5% 混进面板。

use gpui_kit::{App, Background, Hsla, Rgba, linear_color_stop, linear_gradient, rgb, rgba};

use gpui_kit::component::ActiveTheme as _;

use crate::settings::WindowBackdrop;

/// 开了 Mica 时侧栏底色的不透明度：0 是纯 Mica，1 是看不见 Mica。
pub const MICA_TINT_OPACITY: f32 = 0.5;

/// 一组颜色 token。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    /// 侧栏底（Muxy 的 `raised`），也是窗口底色。
    pub side: Hsla,
    /// 内容区底（Muxy 的 `background`）：终端、编辑器、表单、设置的底，终端背景也用它。
    pub content: Hsla,
    /// 标签栏、状态栏、右侧图标栏的底：和窗口同底。
    pub tabbar: Hsla,
    /// 区块之间的 1px 线；弹出层外圈、输入框描边用的深一档。
    pub line: Hsla,
    pub line_strong: Hsla,
    /// 文字：正文、侧栏和列表的条目、次要、很淡的提示和占位、非焦点窗格的状态栏。
    pub text: Hsla,
    pub label: Hsla,
    pub text2: Hsla,
    /// 状态栏这类次要、但要看清楚的文字：前景 80%（比 `text2` 深一档）。
    pub text_soft: Hsla,
    pub faint: Hsla,
    pub st_dim: Hsla,
    /// 图标。
    pub icon: Hsla,
    /// 悬停底、选中底、当前标签的底。
    pub hover: Hsla,
    pub sel: Hsla,
    pub tab_active: Hsla,
    /// 已连接圆点和绿字、出错和危险红、提醒橙。
    pub ok: Hsla,
    pub ok_soft: Hsla,
    pub ok_text: Hsla,
    pub err: Hsla,
    /// 状态色的淡底和描边：失败/警告/成功的行、徽章和提示条共用。
    pub err_soft: Hsla,
    pub err_line: Hsla,
    pub warn: Hsla,
    pub warn_soft: Hsla,
    pub warn_line: Hsla,
    /// 强调色（Muxy 的 `accent`）：主按钮、开关打开、分段选中、菜单悬停。
    pub brand: Hsla,
    /// 强调色悬停：强调色混 12% 黑。
    pub brand_hover: Hsla,
    /// 强调色的淡底（10%）：选中的小标签、聚焦行。
    pub brand_soft: Hsla,
    /// 强调色淡底悬停（18%）：例如重连徽章的悬停底。
    pub brand_soft_hover: Hsla,
    /// 强调色焦点环（25%）：开关等控件获得键盘焦点时的外圈底。
    pub brand_focus_ring: Hsla,
    /// 强调色 45% 的描边：拖放悬停的方框、框选矩形。
    pub brand_ring: Hsla,
    /// 强调色 80% 的次级进度条填充（如传输队列的子文件条）。
    pub brand_progress: Hsla,
    /// 强调色上的字：按强调色的亮度取黑或白（Muxy 的 `accent_foreground`）。
    pub on_brand: Hsla,
    /// 强调色上的淡字（70%）：主按钮里的快捷键提示。
    pub on_brand_muted: Hsla,
    /// 开关滑块在两种外观下均为白色，不随品牌文字色变化。
    pub switch_knob: Hsla,

    // ───────── 浮层与杂项 ─────────
    /// 右键菜单、弹窗、浮卡的底（Air Neutral：深色 `#1B1F27`、浅色纯白）。
    pub popup: Hsla,
    /// 右键菜单外圈描边和内部分隔线（前景 11% / 8%）。
    pub menu_border: Hsla,
    pub menu_separator: Hsla,
    /// 输入框、下拉框、主题预览的底（前景 3.5% 浅色 / 4.5% 深色混进面板）。
    pub input: Hsla,
    /// 查找：匹配底、匹配下划线、当前匹配底和字。
    pub find_match: Hsla,
    pub find_match_line: Hsla,
    pub find_current: Hsla,
    pub find_current_text: Hsla,
    /// 开 Mica 时侧栏的渐变两端（GPUI 的渐变只有两个色标）：终点微混静谧蓝，保持冷净通透。
    mica_start: Hsla,
    mica_end: Hsla,
    /// 滚动条、提示框底和字。
    pub scrollbar: Hsla,
    pub tooltip: Hsla,
    pub tooltip_text: Hsla,
    /// 窗口控制按钮圆点（最小化、最大化、关闭）。
    pub window_min: Hsla,
    pub window_max: Hsla,
    pub window_close: Hsla,
    /// 对话框后面的遮罩：深色 35%、浅色 22%。
    pub backdrop: Hsla,
}

/// 一套配色的原值（Air Neutral）：底色、前景、强调色、状态色、浮层面板色与窗口圆点色。
struct Scheme {
    background: u32,
    foreground: u32,
    accent: u32,
    warning: u32,
    danger: u32,
    diff_add: u32,
    panel: u32,
    window_min: u32,
    window_max: u32,
    window_close: u32,
}

/// 深色：Air Neutral（清冽黑曜石 + 冰感静谧蓝）。
const DARK: Scheme = Scheme {
    background: 0x101216,
    foreground: 0xeceff4,
    accent: 0x5b9bf8,
    warning: 0xeab348,
    danger: 0xf05c65,
    diff_add: 0x38c17a,
    panel: 0x1b1f27,
    window_min: 0xfebc2e,
    window_max: 0x28c840,
    window_close: 0xff5f57,
};

/// 浅色：Air Neutral（极净冷白 + 清澈静谧蓝）。
const LIGHT: Scheme = Scheme {
    background: 0xffffff,
    foreground: 0x14171f,
    accent: 0x2563eb,
    warning: 0xa16207,
    danger: 0xd1343f,
    diff_add: 0x15804a,
    panel: 0xffffff,
    window_min: 0xfebc2e,
    window_max: 0x28c840,
    window_close: 0xff5f57,
};

impl Tokens {
    /// 浅色：Air Neutral Light。
    pub fn light() -> Self {
        Self::from_scheme(LIGHT)
    }

    /// 深色：Air Neutral Dark。
    pub fn dark() -> Self {
        Self::from_scheme(DARK)
    }

    fn from_scheme(scheme: Scheme) -> Self {
        let content: Hsla = rgb(scheme.background).into();
        let fg: Hsla = rgb(scheme.foreground).into();
        let accent: Hsla = rgb(scheme.accent).into();
        let warning: Hsla = rgb(scheme.warning).into();
        let danger: Hsla = rgb(scheme.danger).into();
        let black: Hsla = rgb(0x000000).into();
        let dark = content.l < 0.5;
        let side = blend(fg.opacity(if dark { 0.065 } else { 0.042 }), content);
        // 浮层（菜单、对话框）：深色比内容抬升一级，浅色纯白配柔和阴影。
        let popup: Hsla = rgb(scheme.panel).into();
        // 输入框、下拉框：极轻中性底混进面板，保持通透层次。
        let input = blend(fg.opacity(if dark { 0.045 } else { 0.035 }), popup);
        let add: Hsla = rgb(scheme.diff_add).into();
        let on_brand = on_color(accent);
        Self {
            side,
            content,
            tabbar: content,
            line: blend(fg.opacity(if dark { 0.08 } else { 0.07 }), content),
            line_strong: blend(fg.opacity(if dark { 0.16 } else { 0.14 }), content),
            text: fg,
            label: fg,
            text2: fg.opacity(0.65),
            text_soft: fg.opacity(0.8),
            faint: fg.opacity(0.4),
            st_dim: fg.opacity(0.5),
            icon: fg.opacity(0.65),
            hover: fg.opacity(if dark { 0.055 } else { 0.045 }),
            sel: fg.opacity(if dark { 0.085 } else { 0.065 }),
            tab_active: fg.opacity(if dark { 0.045 } else { 0.035 }),
            ok: add,
            ok_soft: add.opacity(0.12),
            ok_text: add,
            err: danger,
            err_soft: danger.opacity(0.12),
            err_line: danger.opacity(0.4),
            warn: warning,
            warn_soft: warning.opacity(0.14),
            warn_line: warning.opacity(0.45),
            brand: accent,
            brand_hover: blend(black.opacity(0.12), accent),
            brand_soft: accent.opacity(0.1),
            brand_soft_hover: accent.opacity(0.18),
            brand_focus_ring: accent.opacity(0.25),
            brand_ring: accent.opacity(0.45),
            brand_progress: accent.opacity(0.8),
            on_brand,
            on_brand_muted: on_brand.opacity(0.7),
            switch_knob: rgb(0xffffff).into(),
            popup,
            menu_border: fg.opacity(0.11),
            menu_separator: fg.opacity(0.08),
            input,
            find_match: warning.opacity(0.22),
            find_match_line: warning.opacity(0.6),
            find_current: warning,
            find_current_text: on_color(warning),
            mica_start: side,
            mica_end: blend(accent.opacity(0.08), side),
            scrollbar: fg.opacity(0.25),
            tooltip: side,
            tooltip_text: fg,
            window_min: rgb(scheme.window_min).into(),
            window_max: rgb(scheme.window_max).into(),
            window_close: rgb(scheme.window_close).into(),
            backdrop: if dark {
                rgba(0x00000059).into()
            } else {
                rgba(0x00000038).into()
            },
        }
    }

    /// 窗口背景：一块平的侧栏底色。开了 Mica（工单 26）时降到 `MICA_TINT_OPACITY`，让系统的 Mica 透出来。
    /// Mica 只该透在侧栏，内容区、标签栏、状态栏各自有不透明的底。
    pub fn window_background(&self, backdrop: WindowBackdrop) -> Background {
        match backdrop {
            WindowBackdrop::Opaque => self.side.into(),
            WindowBackdrop::Mica => linear_gradient(
                165.,
                linear_color_stop(self.mica_start.opacity(MICA_TINT_OPACITY), 0.),
                linear_color_stop(self.mica_end.opacity(MICA_TINT_OPACITY), 1.),
            ),
        }
    }

    /// 当前主题模式（浅色或深色）的那组 token。
    pub fn current(cx: &App) -> Self {
        if cx.theme().is_dark() {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

/// `top` 按自身透明度混进 `bottom`，返回不透明的颜色（Muxy 的 `blend`）。
fn blend(top: Hsla, bottom: Hsla) -> Hsla {
    let top: Rgba = top.into();
    let bottom: Rgba = bottom.into();
    let mix = |t: f32, b: f32| t * top.a + b * (1.0 - top.a);
    Rgba {
        r: mix(top.r, bottom.r),
        g: mix(top.g, bottom.g),
        b: mix(top.b, bottom.b),
        a: 1.0,
    }
    .into()
}

/// WCAG 相对亮度先将 sRGB 转为线性光，再按通道加权。
fn luminance(color: Hsla) -> f32 {
    let rgb = color.to_rgb();
    let channel = |value: f32| {
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb.r) + 0.7152 * channel(rgb.g) + 0.0722 * channel(rgb.b)
}

/// 在不透明底色上选取对比度更高的黑字或白字。
pub(crate) fn dark_ink_on(background: Hsla) -> bool {
    let level = luminance(background) + 0.05;
    level / 0.05 >= 1.05 / level
}

pub(crate) fn on_color(background: Hsla) -> Hsla {
    if dark_ink_on(background) {
        rgb(0x000000).into()
    } else {
        rgb(0xffffff).into()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::Hsla;

    use super::{Tokens, blend, luminance};

    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let (first, second) = (luminance(a), luminance(b));
        let (high, low) = if first > second {
            (first, second)
        } else {
            (second, first)
        };
        (high + 0.05) / (low + 0.05)
    }

    /// 半透明的字要先按背景合成再量对比度，不然量的是它不透明时的样子。
    fn over(color: Hsla, background: Hsla) -> Hsla {
        blend(color, background)
    }

    #[test]
    fn primary_label_meets_aa_in_both_modes() {
        for tokens in [Tokens::light(), Tokens::dark()] {
            assert!(contrast(tokens.on_brand, tokens.brand) >= 4.5);
        }
        assert_eq!(
            super::on_color(gpui_kit::rgb(0).into()),
            gpui_kit::rgb(0xffffff).into()
        );
    }

    /// Air Neutral 的正文、品牌链接与状态色在明暗双模式下均守住 WCAG AA 下限（4.5）；
    /// 次要/图标类守住 3.0。半透明的字按合成到背景后的颜色量。
    #[test]
    fn text_and_status_colors_stay_readable_in_both_modes() {
        for (mode, tokens) in [("light", Tokens::light()), ("dark", Tokens::dark())] {
            let content = tokens.content;
            for (name, color) in [
                ("text", tokens.text),
                ("label", tokens.label),
                ("text2", tokens.text2),
                ("text_soft", tokens.text_soft),
                ("brand", tokens.brand),
                ("ok_text", tokens.ok_text),
                ("warn", tokens.warn),
                ("err", tokens.err),
            ] {
                let ratio = contrast(over(color, content), content);
                assert!(ratio >= 4.5, "{mode} {name}: {ratio:.2}");
            }
            for (name, color) in [("st_dim", tokens.st_dim), ("icon", tokens.icon)] {
                let ratio = contrast(over(color, content), content);
                assert!(ratio >= 3., "{mode} {name}: {ratio:.2}");
            }
            let st_dim = contrast(over(tokens.st_dim, tokens.tabbar), tokens.tabbar);
            assert!(st_dim >= 3., "{mode} st_dim on tabbar: {st_dim:.2}");
            let text2 = contrast(over(tokens.text2, tokens.side), tokens.side);
            assert!(text2 >= 4., "{mode} text2 on side: {text2:.2}");
            let text = contrast(over(tokens.text, tokens.popup), tokens.popup);
            assert!(text >= 4.4, "{mode} text on popup: {text:.2}");
            let tooltip = contrast(over(tokens.tooltip_text, tokens.tooltip), tokens.tooltip);
            assert!(tooltip >= 4.4, "{mode} tooltip: {tooltip:.2}");
            let find = contrast(
                over(tokens.find_current_text, tokens.find_current),
                tokens.find_current,
            );
            assert!(find >= 4.3, "{mode} find current: {find:.2}");
        }
    }

    /// 高亮描边 token 仍是强调色的 45%，两种外观都不变。
    #[test]
    fn status_tints_are_derived_once() {
        for tokens in [Tokens::light(), Tokens::dark()] {
            assert_eq!(tokens.err_soft, tokens.err.opacity(0.12));
            assert_eq!(tokens.err_line, tokens.err.opacity(0.4));
            assert_eq!(tokens.ok_soft, tokens.ok.opacity(0.12));
            assert_eq!(tokens.warn_soft, tokens.warn.opacity(0.14));
            assert_eq!(tokens.warn_line, tokens.warn.opacity(0.45));
        }
    }

    #[test]
    fn brand_ring_stays_brand_at_45_percent() {
        for tokens in [Tokens::light(), Tokens::dark()] {
            assert_eq!(tokens.brand_ring, tokens.brand.opacity(0.45));
        }
    }

    /// 深色的浮层要比内容看得出浮起（浅色靠边框区分，底可以同色）。
    #[test]
    fn dark_popups_stand_out_from_the_content() {
        let tokens = Tokens::dark();
        let ratio = contrast(tokens.popup, tokens.content);
        assert!(ratio >= 1.1, "popup: {ratio:.2}");
    }
}
