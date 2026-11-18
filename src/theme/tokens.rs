//! 颜色 token：取自 Ayu 主题（<https://ayutheme.com/>）的 Light / Dark 两个变体，
//! UI 色按 `ayu-theme/vscode-ayu` 的三个主题 JSON，语法色按官网色板。界面颜色不跟终端主题走。
//!
//! 分层不靠阴影，靠**底色分档**加 1px 细线：侧栏 `side` 是 Ayu 的 Sidebar（深色下比内容亮一档），
//! 内容区 `content` 是 Editor，标签栏和状态栏 `tabbar` 是 Chrome（Dark 下接近纯黑）。
//! **没有强调色**：主按钮是 `sel` 灰底，红色只用于危险操作和出错，绿色只用于已连接。
//!
//! 浅色下有几处不是 Ayu 的原值：Ayu 的语法色是给编辑器设计的，直接当界面文字对比不够
//! （Number 黄 `#f2ae49` 只有 1.87:1），所以 `warn`、`ok` / `ok_text`、`err`、`st_dim`、`line`
//! 都压暗过，值后面的注释写着实际对比度；文件末尾的测试钉住这些下限。

use gpui_kit::{App, Background, Hsla, Rgba, linear_color_stop, linear_gradient, rgb, rgba};

use gpui_kit::component::ActiveTheme as _;

use crate::settings::WindowBackdrop;

/// 开了 Mica 时侧栏底色的不透明度：0 是纯 Mica，1 是看不见 Mica。
pub const MICA_TINT_OPACITY: f32 = 0.5;

/// 一组颜色 token。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    /// 侧栏底（Ayu 的 Sidebar），也是窗口底色。
    pub side: Hsla,
    /// 内容区底（Ayu 的 Editor）：终端、编辑器、表单、设置的底，终端背景也用它。
    pub content: Hsla,
    /// 标签栏、状态栏、右侧图标栏的底（Ayu 的 Chrome）：Dark 下接近纯黑。
    pub tabbar: Hsla,
    /// 区块之间的 1px 线；弹出层外圈、输入框描边用的深一档。
    pub line: Hsla,
    pub line_strong: Hsla,
    /// 文字：正文、侧栏和列表的条目、次要、很淡的提示和占位、非焦点窗格的状态栏。
    pub text: Hsla,
    pub label: Hsla,
    pub text2: Hsla,
    pub faint: Hsla,
    pub st_dim: Hsla,
    /// 图标。
    pub icon: Hsla,
    /// 悬停底、选中底和主按钮底。
    pub hover: Hsla,
    pub sel: Hsla,
    /// 已连接圆点和绿字、出错和危险红、提醒橙。
    pub ok: Hsla,
    pub ok_text: Hsla,
    pub err: Hsla,
    pub warn: Hsla,

    // ───────── 浮层与杂项 ─────────
    /// 右键菜单、弹窗、浮卡的底。
    pub popup: Hsla,
    /// 输入框、下拉框、主题预览的底。
    pub input: Hsla,
    /// 查找：匹配底、匹配下划线、当前匹配底和字。Ayu 的 Number 金，当前项保持亮金、字用深色。
    pub find_match: Hsla,
    pub find_match_line: Hsla,
    pub find_current: Hsla,
    pub find_current_text: Hsla,
    /// 开 Mica 时侧栏的渐变两端（GPUI 的渐变只有两个色标）：终点混一点 Ayu 的橙，透出来偏暖。
    pub mica_start: Hsla,
    pub mica_end: Hsla,
    /// 滚动条、提示框底和字、窗口「关闭」按钮悬停的底和字。
    pub scrollbar: Hsla,
    pub tooltip: Hsla,
    pub tooltip_text: Hsla,
    pub close_hover: Hsla,
    pub close_hover_text: Hsla,
}

fn c(color: Rgba) -> Hsla {
    color.into()
}

/// 一套颜色的原值。
struct Palette {
    side: Rgba,
    content: Rgba,
    tabbar: Rgba,
    line: Rgba,
    line_strong: Rgba,
    text: Rgba,
    label: Rgba,
    text2: Rgba,
    icon: Rgba,
    faint: Rgba,
    st_dim: Rgba,
    hover: Rgba,
    sel: Rgba,
    ok: Rgba,
    ok_text: Rgba,
    err: Rgba,
    warn: Rgba,
    popup: Rgba,
    input: Rgba,
    find_match: Rgba,
    find_match_line: Rgba,
    find_current: Rgba,
    find_current_text: Rgba,
    mica_start: Rgba,
    mica_end: Rgba,
    scrollbar: Rgba,
    tooltip: Rgba,
    tooltip_text: Rgba,
    close_hover: Rgba,
    close_hover_text: Rgba,
}

impl Tokens {
    /// 浅色：Ayu Light。
    pub fn light() -> Self {
        Self::from_palette(Palette {
            side: rgb(0xf8f9fa),     // Sidebar
            content: rgb(0xfcfcfc),  // Editor
            tabbar: rgb(0xebeef0),   // Chrome
            line: rgb(0xe0e4e8),     // 官方 Line 只有 1.18:1，压到 1.25
            line_strong: rgb(0xcfd3d8),
            text: rgb(0x5c6166),
            label: rgb(0x464b50),
            text2: rgb(0x6c7177),    // 官方 Subtle #6f7479 在侧栏上 4.48，微调到 4.67
            icon: rgb(0x84898f),
            faint: rgb(0xabb0b6),    // 注释色，占位和禁用
            st_dim: rgb(0x83888e),   // 官方 2.73，压到 3.48（标签栏上 3.07）
            hover: rgba(0x6b7d8f24), // 官方 list.hoverBackground
            sel: rgb(0xdcdfe3),
            ok: rgb(0x6b8a00),       // String 压暗：圆点 3.89
            ok_text: rgb(0x5b7a00),  // 「已连接」文字 4.84
            err: rgb(0xc44444),      // Markup 压暗：错误文字 4.81
            warn: rgb(0x9c6400),     // Number 压暗：警示文字 4.83（原值 1.87）
            popup: rgb(0xfcfcfc),
            input: rgb(0xf8f9fa),
            find_match: rgba(0xf2ae4938),      // 亮金 22%
            find_match_line: rgba(0xf2ae4999), // 亮金 60%
            find_current: rgb(0xf2ae49),
            find_current_text: rgb(0x464b50),  // 金底深字 4.59
            mica_start: rgb(0xf8f9fa),
            mica_end: rgb(0xf7eddf), // Sidebar 88% + 橙 12%
            scrollbar: rgba(0xabb0b68c),
            tooltip: rgb(0x5c6166),
            tooltip_text: rgb(0xfcfcfc),
            close_hover: rgb(0xc44444),
            close_hover_text: rgb(0xffffff),
        })
    }

    /// 深色：Ayu Dark。
    pub fn dark() -> Self {
        Self::from_palette(Palette {
            side: rgb(0x0d1017),     // Sidebar
            content: rgb(0x10141c),  // Editor
            tabbar: rgb(0x010102),   // Chrome，接近纯黑
            line: rgb(0x1b1f29),     // Panel border
            line_strong: rgb(0x252b38),
            text: rgb(0xd6d3cc),
            label: rgb(0xb3b1ad),
            text2: rgb(0x9c9a95),
            icon: rgb(0x8b9199),
            faint: rgb(0x626a73),    // 注释色
            st_dim: rgb(0x767d86),
            hover: rgba(0x47526640), // 官方 list.hoverBackground
            sel: rgba(0x47526666),   // 官方 list.activeSelectionBackground
            ok: rgb(0xaad94c),
            ok_text: rgb(0xaad94c),
            err: rgb(0xf07178),
            warn: rgb(0xe6b673),
            popup: rgb(0x202634),    // 官方浮层色与内容只差 1.06，抬到 1.22
            input: rgb(0x0b0f16),
            find_match: rgba(0xe6b67338),
            find_match_line: rgba(0xe6b67399),
            find_current: rgb(0xe6b673),
            find_current_text: rgb(0x010102),
            mica_start: rgb(0x0d1017),
            mica_end: rgb(0x27241e), // Sidebar 88% + 橙 12%
            scrollbar: rgba(0x626a738c),
            tooltip: rgb(0x252b38),
            tooltip_text: rgb(0xd6d3cc),
            close_hover: rgb(0xf07178),
            close_hover_text: rgb(0x010102),
        })
    }

    fn from_palette(p: Palette) -> Self {
        Self {
            side: c(p.side),
            content: c(p.content),
            tabbar: c(p.tabbar),
            line: c(p.line),
            line_strong: c(p.line_strong),
            text: c(p.text),
            label: c(p.label),
            text2: c(p.text2),
            faint: c(p.faint),
            st_dim: c(p.st_dim),
            icon: c(p.icon),
            hover: c(p.hover),
            sel: c(p.sel),
            ok: c(p.ok),
            ok_text: c(p.ok_text),
            err: c(p.err),
            warn: c(p.warn),
            popup: c(p.popup),
            input: c(p.input),
            find_match: c(p.find_match),
            find_match_line: c(p.find_match_line),
            find_current: c(p.find_current),
            find_current_text: c(p.find_current_text),
            mica_start: c(p.mica_start),
            mica_end: c(p.mica_end),
            scrollbar: c(p.scrollbar),
            tooltip: c(p.tooltip),
            tooltip_text: c(p.tooltip_text),
            close_hover: c(p.close_hover),
            close_hover_text: c(p.close_hover_text),
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

#[cfg(test)]
mod tests {
    use gpui_kit::Hsla;

    use super::Tokens;

    /// WCAG 的相对亮度。
    fn luminance(color: Hsla) -> f32 {
        let rgb = color.to_rgb();
        let channel = |value: f32| {
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(rgb.r) + 0.7152 * channel(rgb.g) + 0.0722 * channel(rgb.b)
    }

    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let (first, second) = (luminance(a), luminance(b));
        let (high, low) = if first > second {
            (first, second)
        } else {
            (second, first)
        };
        (high + 0.05) / (low + 0.05)
    }

    /// Ayu 的语法色直接当界面文字不够读，压暗后的值要守住下限：正文类 4.5，次要类 3。
    /// 阈值取 4.4，给 Hsla 换算留一点误差。
    #[test]
    fn text_and_status_colors_stay_readable_in_both_modes() {
        for (mode, tokens) in [("light", Tokens::light()), ("dark", Tokens::dark())] {
            for (name, color) in [
                ("text", tokens.text),
                ("label", tokens.label),
                ("text2", tokens.text2),
                ("ok_text", tokens.ok_text),
                ("err", tokens.err),
                ("warn", tokens.warn),
            ] {
                let ratio = contrast(color, tokens.content);
                assert!(ratio >= 4.4, "{mode} {name}: {ratio:.2}");
            }
            for (name, color) in [("st_dim", tokens.st_dim), ("icon", tokens.icon)] {
                let ratio = contrast(color, tokens.content);
                assert!(ratio >= 3., "{mode} {name}: {ratio:.2}");
            }
            assert!(
                contrast(tokens.st_dim, tokens.tabbar) >= 3.,
                "{mode} st_dim on tabbar: {:.2}",
                contrast(tokens.st_dim, tokens.tabbar)
            );
            assert!(
                contrast(tokens.text2, tokens.side) >= 4.4,
                "{mode} text2 on side: {:.2}",
                contrast(tokens.text2, tokens.side)
            );
            assert!(
                contrast(tokens.text, tokens.popup) >= 4.4,
                "{mode} text on popup: {:.2}",
                contrast(tokens.text, tokens.popup)
            );
            assert!(
                contrast(tokens.tooltip_text, tokens.tooltip) >= 4.4,
                "{mode} tooltip: {:.2}",
                contrast(tokens.tooltip_text, tokens.tooltip)
            );
            assert!(
                contrast(tokens.find_current_text, tokens.find_current) >= 4.4,
                "{mode} find current: {:.2}",
                contrast(tokens.find_current_text, tokens.find_current)
            );
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
