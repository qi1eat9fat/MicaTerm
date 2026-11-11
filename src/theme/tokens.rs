//! 颜色 token：照抄全部界面原型（`.scratch/qishell-design/prototypes/17-all-screens.prototype.html`）
//! CSS 里 `.win.light` / `.win.dark` 两组变量，名字对应原型的 `--xxx`。
//! 界面颜色不跟终端主题走。

use gpui_kit::{App, Background, Hsla, Rgba, linear_color_stop, linear_gradient, rgb, rgba};

use gpui_kit::component::ActiveTheme as _;

/// 开了 Mica 时窗口背景渐变的不透明度：0 是纯 Mica，1 是看不见 Mica。
pub const MICA_TINT_OPACITY: f32 = 0.5;

/// 一组颜色 token。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    /// 窗口背景渐变的上、下两端（`--bg-top`、`--bg-bottom`）。GPUI 的渐变只有两个色标。
    pub bg_top: Hsla,
    pub bg_bottom: Hsla,
    /// 卡片底色、描边；聚焦卡片的底色、描边（`--card`、`--card-b`、`--card-f`、`--card-bf`）。
    pub card: Hsla,
    pub card_border: Hsla,
    pub card_focused: Hsla,
    pub card_focused_border: Hsla,
    /// 阴影；聚焦时的阴影（`--shadow`、`--shadow-f`）。
    pub shadow: Hsla,
    pub shadow_focused: Hsla,
    /// 三级文字（`--text`、`--muted`、`--faint`）。
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    /// 图标按钮的图标色、悬停底、按下底（`--icon`、`--hover`、`--pressed`）。
    pub icon: Hsla,
    pub hover: Hsla,
    pub pressed: Hsla,
    /// 标签轨道底色、活动标签底色、分隔竖线（`--track`、`--tab-active`、`--sep`）。
    pub track: Hsla,
    pub tab_active: Hsla,
    pub separator: Hsla,
    /// 程序图块的外圈（`--rim`）。
    pub rim: Hsla,
    /// 浮层面板底色、描边（`--panel`、`--panel-b`）。
    pub panel: Hsla,
    pub panel_border: Hsla,
    /// 强调色和强调色上的字（`--accent`、`--on-accent`）。
    pub accent: Hsla,
    pub on_accent: Hsla,
    /// 选中底（`--sel`、`--sel-solid`）。
    pub selection: Hsla,
    pub selection_solid: Hsla,
    /// 状态点：有输出（绿）、需要注意（橙）；离线灰（`--busy`、`--attn`、`--off`）。
    pub busy: Hsla,
    pub attention: Hsla,
    pub offline: Hsla,
    /// 终端前景（`--term`）、细分隔线（`--line`）、错误红（`--bad`）。
    pub terminal: Hsla,
    pub line: Hsla,
    pub bad: Hsla,
    /// 输入框底、描边、聚焦底（`--field`、`--field-b`、`--field-f`）。
    pub field: Hsla,
    pub field_border: Hsla,
    pub field_focused: Hsla,
    /// 分段按钮底（`--seg`）、对话框遮罩（`--scrim`）、分组底（`--group`）。
    pub segment: Hsla,
    pub scrim: Hsla,
    pub group: Hsla,
    /// 查找命中、当前命中（`--match`、`--match-on`）。
    pub find_match: Hsla,
    pub find_match_current: Hsla,
    /// 滚动条（`--scroll`）、提示框底和字（`--tip`、`--tip-t`）。
    pub scrollbar: Hsla,
    pub tooltip: Hsla,
    pub tooltip_text: Hsla,
    /// 窗口「关闭」按钮悬停时的红底和白字（`.winbtns .ib.close:hover`）。
    pub close_hover: Hsla,
    pub close_hover_text: Hsla,
}

fn c(color: Rgba) -> Hsla {
    color.into()
}

impl Tokens {
    /// 浅色：中性冷灰底 + 白卡片。
    pub fn light() -> Self {
        Self {
            bg_top: c(rgb(0xf4f5f7)),
            bg_bottom: c(rgb(0xeceef1)),
            card: c(rgb(0xffffff)),
            card_border: c(rgb(0xe3e6ea)),
            card_focused: c(rgb(0xffffff)),
            card_focused_border: c(rgba(0x4f5bd58c)),
            shadow: c(rgba(0x1018280a)),
            shadow_focused: c(rgba(0x10182817)),
            text: c(rgb(0x1c2024)),
            muted: c(rgb(0x525a65)),
            faint: c(rgb(0x6b7280)),
            icon: c(rgb(0x6b7280)),
            hover: c(rgba(0x0f172a0d)),
            pressed: c(rgba(0x0f172a17)),
            track: c(rgb(0xe5e7eb)),
            tab_active: c(rgb(0xffffff)),
            separator: c(rgb(0xd3d7dd)),
            rim: c(rgb(0xffffff)),
            panel: c(rgb(0xffffff)),
            panel_border: c(rgb(0xdfe2e7)),
            accent: c(rgb(0x4f5bd5)),
            on_accent: c(rgb(0xffffff)),
            selection: c(rgba(0x4f5bd51a)),
            selection_solid: c(rgb(0xedeefb)),
            busy: c(rgb(0x1f883d)),
            attention: c(rgb(0xc27c0e)),
            offline: c(rgb(0x8c939d)),
            terminal: c(rgb(0x1f2328)),
            line: c(rgb(0xeaecef)),
            bad: c(rgb(0xcf222e)),
            field: c(rgb(0xf3f4f6)),
            field_border: c(rgb(0xe2e5e9)),
            field_focused: c(rgb(0xffffff)),
            segment: c(rgb(0xeceef1)),
            scrim: c(rgba(0x0f172a42)),
            group: c(rgb(0xf7f8fa)),
            find_match: c(rgba(0xd4a72c52)),
            find_match_current: c(rgb(0xf2cc60)),
            scrollbar: c(rgba(0x0f172a38)),
            tooltip: c(rgb(0x1f2328)),
            tooltip_text: c(rgb(0xf5f6f8)),
            close_hover: c(rgb(0xc42b1c)),
            close_hover_text: c(rgb(0xffffff)),
        }
    }

    /// 深色：炭灰底，卡片比底亮一级；强调色用浅一号的靛蓝。
    pub fn dark() -> Self {
        Self {
            bg_top: c(rgb(0x18191c)),
            bg_bottom: c(rgb(0x131416)),
            card: c(rgb(0x1d1f23)),
            card_border: c(rgb(0x2b2e34)),
            card_focused: c(rgb(0x212328)),
            card_focused_border: c(rgba(0x8b93ff99)),
            shadow: c(rgba(0x00000038)),
            shadow_focused: c(rgba(0x00000057)),
            text: c(rgb(0xe8eaed)),
            muted: c(rgb(0xaab0b8)),
            faint: c(rgb(0x8f96a0)),
            icon: c(rgb(0x979ea8)),
            hover: c(rgba(0xffffff0f)),
            pressed: c(rgba(0xffffff1a)),
            track: c(rgb(0x232529)),
            tab_active: c(rgb(0x34373e)),
            separator: c(rgb(0x34373d)),
            rim: c(rgb(0x1d1f23)),
            panel: c(rgb(0x25272c)),
            panel_border: c(rgb(0x363940)),
            accent: c(rgb(0x8b93ff)),
            on_accent: c(rgb(0x101228)),
            selection: c(rgba(0x8b93ff29)),
            selection_solid: c(rgb(0x2a2c44)),
            busy: c(rgb(0x3fb950)),
            attention: c(rgb(0xe3a82b)),
            offline: c(rgb(0x5f656e)),
            terminal: c(rgb(0xe6e8eb)),
            line: c(rgb(0x2b2e33)),
            bad: c(rgb(0xff6b6b)),
            field: c(rgb(0x17191c)),
            field_border: c(rgb(0x30333a)),
            field_focused: c(rgb(0x131417)),
            segment: c(rgb(0x17191c)),
            scrim: c(rgba(0x00000080)),
            group: c(rgb(0x1a1c20)),
            find_match: c(rgba(0xe3b34140)),
            find_match_current: c(rgb(0xe3b341)),
            scrollbar: c(rgba(0xffffff38)),
            tooltip: c(rgb(0x3a3d45)),
            tooltip_text: c(rgb(0xf2f3f5)),
            close_hover: c(rgb(0xc42b1c)),
            close_hover_text: c(rgb(0xffffff)),
        }
    }

    /// 窗口背景：从 `bg_top` 到 `bg_bottom` 的竖直渐变。`translucent`（开了 Mica，工单 26）时
    /// 两端都降到 `MICA_TINT_OPACITY`，让系统的 Mica 透出来，又压住 Mica 和应用外观不一致的色调
    /// （Mica 跟系统的深浅色走，应用外观可以单独设）。
    pub fn window_gradient(&self, translucent: bool) -> Background {
        let tint = |color: Hsla| {
            if translucent {
                color.opacity(MICA_TINT_OPACITY)
            } else {
                color
            }
        };
        linear_gradient(
            180.,
            linear_color_stop(tint(self.bg_top), 0.),
            linear_color_stop(tint(self.bg_bottom), 1.),
        )
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
