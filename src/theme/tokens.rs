//! 颜色 token：照抄 26 号逐屏原型（`.scratch/qishell-design/prototypes/26-screens.prototype.html`）
//! CSS 里 `.win.light` / `.win.dark` 两组变量，字段名对应原型的 `--xxx`；另有几种原型写死在组件上的颜色
//! （弹出层底、输入框底、查找高亮、Mica 渐变）也收在这里。界面颜色不跟终端主题走。
//!
//! 分层不靠阴影，靠**底色分档**加 1px 细线：侧栏 `side`、内容区 `content`、标签栏和状态栏比内容暗一档
//! `tabbar`。**没有强调色**：主按钮是 `sel` 浅灰底，红色只用于危险操作和出错，绿色只用于已连接。

use gpui_kit::{App, Background, Hsla, Rgba, linear_color_stop, linear_gradient, rgb, rgba};

use gpui_kit::component::ActiveTheme as _;

use crate::settings::WindowBackdrop;

/// 开了 Mica 时侧栏底色的不透明度：0 是纯 Mica，1 是看不见 Mica。
pub const MICA_TINT_OPACITY: f32 = 0.5;

/// 一组颜色 token。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    // ───────── 26 号 `.win.light` / `.win.dark` ─────────
    /// 侧栏底（`--side`），也是窗口底色。
    pub side: Hsla,
    /// 内容区底（`--content`）：终端、编辑器、表单、设置的底，终端背景也用它。
    pub content: Hsla,
    /// 标签栏、状态栏、右侧图标栏的底，比内容暗一档（`--tabbar`）。
    pub tabbar: Hsla,
    /// 区块之间的 1px 线（`--line`）；弹出层外圈、输入框描边用的深一档（`--line-strong`）。
    pub line: Hsla,
    pub line_strong: Hsla,
    /// 文字：正文（`--text`）、侧栏和列表的条目（`--label`）、次要（`--text2`）、
    /// 很淡的提示和占位（`--faint`）、非焦点窗格的状态栏（`--st-dim`）。
    pub text: Hsla,
    pub label: Hsla,
    pub text2: Hsla,
    pub faint: Hsla,
    pub st_dim: Hsla,
    /// 图标（`--icon`）。
    pub icon: Hsla,
    /// 悬停底（`--hover`）、选中底和主按钮底（`--sel`）。
    pub hover: Hsla,
    pub sel: Hsla,
    /// 已连接绿点（`--ok`）和绿字（`--ok-text`）、出错和危险红（`--err`）、提醒橙（`--warn`）。
    pub ok: Hsla,
    pub ok_text: Hsla,
    pub err: Hsla,
    pub warn: Hsla,

    // ───────── 原型写死在组件上的颜色 ─────────
    /// 右键菜单、弹窗、浮卡的底（浅色同 `--content`，深色 `#202124`）。
    pub popup: Hsla,
    /// 输入框、下拉框、主题预览的底（`.stg-in` / `.stg-dd`）。
    pub input: Hsla,
    /// 查找：匹配底、匹配下划线、当前匹配底和字（`--hl`、`--hl-line`、`--hl-cur`、`--hl-cur-fg`）。
    pub find_match: Hsla,
    pub find_match_line: Hsla,
    pub find_current: Hsla,
    pub find_current_text: Hsla,
    /// 开 Mica 时侧栏的渐变两端（`--mica` 取首尾两个色标，GPUI 的渐变只有两个色标）。
    pub mica_start: Hsla,
    pub mica_end: Hsla,
    /// 滚动条、提示框底和字、窗口「关闭」按钮悬停的红底白字（26 号没改，沿用第一版）。
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
}

impl Tokens {
    /// 浅色：带一点冷调的中性灰。
    pub fn light() -> Self {
        Self::from_palette(Palette {
            side: rgb(0xebecef),
            content: rgb(0xfafafb),
            tabbar: rgb(0xf2f3f5),
            line: rgb(0xdfe0e4),
            line_strong: rgb(0xcdd0d5),
            text: rgb(0x23252a),
            label: rgb(0x383b41),
            text2: rgb(0x565a62),
            icon: rgb(0x5c6068),
            faint: rgb(0xa3a7ae),
            st_dim: rgb(0x7e828a),
            hover: rgba(0x141e3209), // rgba(20,30,50,.035)
            sel: rgb(0xdcdee2),
            ok: rgb(0x1f8152),
            ok_text: rgb(0x1b7a4c),
            err: rgb(0xc9392f),
            warn: rgb(0xc08b0e),
            popup: rgb(0xfafafb),
            input: rgb(0xf4f5f7),
            find_match: rgba(0xe8b2142e),      // rgba(232,178,20,.18)
            find_match_line: rgba(0xc48c008c), // rgba(196,140,0,.55)
            find_current: rgb(0xf2c443),
            find_current_text: rgb(0x23252a),
            mica_start: rgb(0xe6eaf2),
            mica_end: rgb(0xeeebed),
            scrollbar: rgba(0x0f172a38),
            tooltip: rgb(0x23252a),
            tooltip_text: rgb(0xf2f3f5),
        })
    }

    /// 深色：同一套结构，炭灰底。
    pub fn dark() -> Self {
        Self::from_palette(Palette {
            side: rgb(0x131416),
            content: rgb(0x18191b),
            tabbar: rgb(0x131417),
            line: rgb(0x2a2b2f),
            line_strong: rgb(0x3a3c41),
            text: rgb(0xe3e6eb),
            label: rgb(0xc3c7cd),
            text2: rgb(0x9a9ea6),
            icon: rgb(0xa9adb5),
            faint: rgb(0x62666e),
            st_dim: rgb(0x7f838b),
            hover: rgba(0xffffff0a), // rgba(255,255,255,.04)
            sel: rgba(0xffffff13),   // rgba(255,255,255,.075)
            ok: rgb(0x4cb782),
            ok_text: rgb(0x4cb782),
            err: rgb(0xeb5757),
            warn: rgb(0xf2c94c),
            popup: rgb(0x202124),
            input: rgb(0x131416),
            find_match: rgba(0xf2c94c1f),      // rgba(242,201,76,.12)
            find_match_line: rgba(0xf2c94c8c), // rgba(242,201,76,.55)
            find_current: rgb(0xe6bb3f),
            find_current_text: rgb(0x18191b),
            mica_start: rgb(0x171a20),
            mica_end: rgb(0x17141a),
            scrollbar: rgba(0xffffff38),
            tooltip: rgb(0x3a3c41),
            tooltip_text: rgb(0xe3e6eb),
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
            close_hover: c(rgb(0xc42b1c)),
            close_hover_text: c(rgb(0xffffff)),
        }
    }

    /// 窗口背景：一块平的侧栏底色。开了 Mica（工单 26）时降到 `MICA_TINT_OPACITY`，让系统的 Mica 透出来。
    /// Mica 只该透在侧栏（26 号），内容区、标签栏、状态栏各自有不透明的底。
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
