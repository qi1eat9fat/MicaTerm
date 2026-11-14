//! 查找浮卡（26 号「查找条」，照 Zed `buffer_search`）：浮在焦点窗格右上角（上 8、右 12），
//! 不推动下面的内容；宽 300、高 32、圆角 8，浅色是内容底、深色是弹出层底，外圈 1px `--line-strong`
//! 加一层小阴影。从左到右：淡色放大镜（14）· 输入框 · 「当前/总数」（等宽小字、淡色）· 竖线 ·
//! 上一个 / 下一个 / 关闭（24 的图标按钮，图标 14）。智能大小写，所以没有 Aa、正则开关。
//!
//! 编辑器多一行「替换」（26 号 `.find.rep`，[`ReplaceRow`]）：卡片宽 360、两行各 32，中间一条线；
//! 第二行是替换图标 · 输入框「替换为」· 「替换」「全部替换」两个文字按钮（[`FindTextButton`]）。
//!
//! 终端和编辑器共用这一张卡；匹配的高亮颜色也在 [`FindStyle`] 里：
//! 其余匹配是很淡的暖黄底 + 1px 黄边（字保持原色），当前项是实心暖黄底 + 深色字。
//!
//! 组件不认识应用的颜色 token：应用在设置变化时把 [`FindStyle`] 设成全局量（`settings::apply`）。

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 卡片和匹配高亮画在什么颜色、什么字上。应用按自己的 token 设成全局量；没设时用一套中性灰。
#[derive(Clone, Debug, PartialEq)]
pub struct FindStyle {
    /// 卡片底（浅色 `--content`、深色 `--popup`）。
    pub background: Hsla,
    /// 外圈的线（`--line-strong`）。
    pub border: Hsla,
    pub shadow: Hsla,
    /// 输入的字（`--text`）。
    pub text: Hsla,
    /// 放大镜和计数（`--st-dim`）。
    pub dim: Hsla,
    /// 按钮图标（`--icon`），悬停时变正文色、加 `hover` 底。
    pub icon: Hsla,
    pub hover: Hsla,
    /// 没有匹配时上下按钮的颜色（`--faint`）。
    pub disabled: Hsla,
    /// 计数和按钮之间的竖线（`--line`）。
    pub separator: Hsla,
    /// 悬停提示的底和字。
    pub tooltip: Hsla,
    pub tooltip_text: Hsla,
    /// 界面字体、字号；计数和提示里快捷键的等宽字体、字号。
    pub font: SharedString,
    pub text_size: Pixels,
    pub mono_font: SharedString,
    pub mono_size: Pixels,
    /// 其余匹配：底和 1px 边（`find_match` / `find_match_line`）。
    pub matched: Hsla,
    pub matched_line: Hsla,
    /// 当前项：底和字（`find_current` / `find_current_text`）。
    pub current: Hsla,
    pub current_text: Hsla,
}

impl Global for FindStyle {}

impl Default for FindStyle {
    fn default() -> Self {
        let gray = |lightness: f32| hsla(0., 0., lightness, 1.);
        Self {
            background: gray(0.98),
            border: gray(0.82),
            shadow: hsla(0., 0., 0., 0.08),
            text: gray(0.15),
            dim: gray(0.5),
            icon: gray(0.4),
            hover: gray(0.92),
            disabled: gray(0.66),
            separator: gray(0.88),
            tooltip: gray(0.15),
            tooltip_text: gray(0.98),
            font: "".into(),
            text_size: px(12.),
            mono_font: "".into(),
            mono_size: px(11.5),
            matched: hsla(45. / 360., 0.84, 0.49, 0.18),
            matched_line: hsla(43. / 360., 1., 0.38, 0.55),
            current: hsla(44. / 360., 0.87, 0.61, 1.),
            current_text: gray(0.15),
        }
    }
}

impl FindStyle {
    pub fn current(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }
}

/// 卡片的位置和大小（26 号 `.find`）。
const TOP: Pixels = px(8.);
const RIGHT: Pixels = px(12.);
const WIDTH: Pixels = px(300.);
/// 有替换那一行时的宽（26 号 `.find.rep`）。
const REPLACE_WIDTH: Pixels = px(360.);
const HEIGHT: Pixels = px(32.);
const RADIUS: Pixels = px(8.);
/// 按钮边长、图标边长、按钮圆角。
const BUTTON: Pixels = px(24.);
const ICON: Pixels = px(14.);
const BUTTON_RADIUS: Pixels = px(6.);

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// 卡片上的一个按钮：图标、悬停提示（文字 + 快捷键）、点了做什么。
pub struct FindButton {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    keys: Option<SharedString>,
    enabled: bool,
    on_click: OnClick,
}

impl FindButton {
    /// `label` 是无障碍名称，也是悬停提示的文字；`keys` 跟在后面，用等宽字。
    pub fn new(
        id: impl Into<ElementId>,
        icon: IconName,
        label: impl Into<SharedString>,
        keys: Option<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            icon,
            label: label.into(),
            keys,
            enabled: true,
            on_click: Box::new(on_click),
        }
    }

    /// 现在点不了（没有匹配时的上下）：变淡，不响应。
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn render(self, style: &FindStyle) -> impl IntoElement + use<> {
        let hover = style.hover;
        let hover_text = style.text;
        let tooltip = FindTooltip {
            label: self.label.clone(),
            keys: self.keys,
            style: style.clone(),
        };
        let on_click = self.on_click;
        div()
            .id(self.id)
            .test_support()
            .aria_label(self.label)
            .role(Role::Button)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(BUTTON)
            .rounded(BUTTON_RADIUS)
            .text_color(if self.enabled {
                style.icon
            } else {
                style.disabled
            })
            .when(self.enabled, |button| {
                button
                    .hover(move |button| button.bg(hover).text_color(hover_text))
                    .on_click(move |event, window, cx| on_click(event, window, cx))
            })
            .tooltip(move |_, cx| cx.new(|_| tooltip.clone()).into())
            .child(Icon::new(self.icon).size(ICON))
    }
}

/// 替换那一行的文字按钮（26 号 `.find.rep .qb`）：高 24、左右 8、圆角 6，悬停有底色。
pub struct FindTextButton {
    id: ElementId,
    label: SharedString,
    keys: Option<SharedString>,
    enabled: bool,
    on_click: OnClick,
}

impl FindTextButton {
    /// `label` 是按钮上的字，也是无障碍名称；悬停提示是它和 `keys`。
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        keys: Option<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            keys,
            enabled: true,
            on_click: Box::new(on_click),
        }
    }

    /// 现在点不了（没有匹配）：变淡，不响应。
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn render(self, style: &FindStyle) -> impl IntoElement + use<> {
        let hover = style.hover;
        let tooltip = FindTooltip {
            label: self.label.clone(),
            keys: self.keys,
            style: style.clone(),
        };
        let on_click = self.on_click;
        div()
            .id(self.id)
            .test_support()
            .aria_label(self.label.clone())
            .role(Role::Button)
            .flex()
            .flex_none()
            .items_center()
            .h(BUTTON)
            .px(px(8.))
            .rounded(BUTTON_RADIUS)
            .whitespace_nowrap()
            .text_color(if self.enabled {
                style.text
            } else {
                style.disabled
            })
            .when(self.enabled, |button| {
                button
                    .hover(move |button| button.bg(hover))
                    .on_click(move |event, window, cx| on_click(event, window, cx))
            })
            .tooltip(move |_, cx| cx.new(|_| tooltip.clone()).into())
            .child(self.label)
    }
}

/// 卡片的第二行「替换」：替换图标、输入框、文字按钮。
pub struct ReplaceRow {
    input: Entity<InputState>,
    input_id: ElementId,
    input_label: SharedString,
    icon: Icon,
    buttons: Vec<FindTextButton>,
}

impl ReplaceRow {
    /// `icon` 由应用给（组件不带替换图标）。
    pub fn new(
        input: &Entity<InputState>,
        input_id: impl Into<ElementId>,
        input_label: impl Into<SharedString>,
        icon: Icon,
    ) -> Self {
        Self {
            input: input.clone(),
            input_id: input_id.into(),
            input_label: input_label.into(),
            icon,
            buttons: Vec::new(),
        }
    }

    pub fn button(mut self, button: FindTextButton) -> Self {
        self.buttons.push(button);
        self
    }
}

/// 卡片里的一行：左 10 右 4，前面一个淡色的 14 图标。高 `height`：卡片外圈 1px 边算在 32 里，
/// 只有一行时是 30，两行时各 31（第二行的上边线也算在里面），整张卡还是 32 / 64。
fn card_row(icon: Icon, height: Pixels, style: &FindStyle) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(height)
        .gap(px(2.))
        .pl(px(10.))
        .pr(px(4.))
        .child(
            div()
                .flex_none()
                .mr(px(6.))
                .text_color(style.dim)
                .child(icon.size(ICON)),
        )
}

/// 卡片里的输入框：没有边框和底，字号跟卡片。
fn card_input(
    input: &Entity<InputState>,
    id: ElementId,
    label: SharedString,
    style: &FindStyle,
) -> Div {
    div().flex_1().min_w_0().child(
        Input::new(input)
            .id(id)
            .aria_label(label)
            .appearance(false)
            .small()
            .px_0()
            .text_size(style.text_size),
    )
}

/// 按钮的悬停提示：文字，后面是快捷键（等宽、淡一点）。
#[derive(Clone)]
struct FindTooltip {
    label: SharedString,
    keys: Option<SharedString>,
    style: FindStyle,
}

impl Render for FindTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let style = &self.style;
        div()
            .id("find-tooltip")
            .test_support()
            .aria_label(self.label.clone())
            .flex()
            .items_baseline()
            .gap(px(8.))
            .px(px(8.))
            .py(px(4.))
            .rounded(BUTTON_RADIUS)
            .bg(style.tooltip)
            .text_color(style.tooltip_text)
            .font_family(style.font.clone())
            .text_size(style.text_size)
            .child(self.label.clone())
            .when_some(self.keys.clone(), |tip, keys| {
                tip.child(
                    div()
                        .font_family(style.mono_font.clone())
                        .text_size(style.mono_size)
                        .opacity(0.7)
                        .child(keys),
                )
            })
    }
}

/// 一张查找卡：一个输入框、计数、上下和关闭。调用方拿 [`FindCard::build`] 的结果再加 key context 之类。
pub struct FindCard {
    input: Entity<InputState>,
    input_id: ElementId,
    input_label: SharedString,
    /// 「当前/总数」：元素 id 和文字；`None` 时不显示（还没有查询、还在搜）。
    count: Option<(ElementId, SharedString)>,
    buttons: Vec<FindButton>,
    /// 摆卡片的那个盒子在窗格边里面多远（上、右）：卡片往外挪这么多，离窗格边还是 8 / 12。
    offset: (Pixels, Pixels),
    /// 第二行「替换」（编辑器）。
    replace: Option<ReplaceRow>,
}

impl FindCard {
    pub fn new(
        input: &Entity<InputState>,
        input_id: impl Into<ElementId>,
        input_label: impl Into<SharedString>,
    ) -> Self {
        Self {
            input: input.clone(),
            input_id: input_id.into(),
            input_label: input_label.into(),
            count: None,
            buttons: Vec::new(),
            offset: (px(0.), px(0.)),
            replace: None,
        }
    }

    /// 加第二行「替换」：卡片变宽到 360。
    pub fn replace(mut self, row: Option<ReplaceRow>) -> Self {
        self.replace = row;
        self
    }

    pub fn count(mut self, id: impl Into<ElementId>, count: Option<SharedString>) -> Self {
        let id = id.into();
        self.count = count.map(|count| (id, count));
        self
    }

    /// 竖线后面的按钮，从左到右。
    pub fn button(mut self, button: FindButton) -> Self {
        self.buttons.push(button);
        self
    }

    /// 卡片放在一个比窗格小一圈的盒子里时（终端四周留了边），盒子离窗格上边 `top`、右边 `right`。
    pub fn offset(mut self, top: Pixels, right: Pixels) -> Self {
        self.offset = (top, right);
        self
    }

    pub fn build(self, cx: &App) -> Div {
        let style = FindStyle::current(cx);
        let row_height = if self.replace.is_some() {
            HEIGHT - px(1.)
        } else {
            HEIGHT - px(2.)
        };
        let find_row = card_row(Icon::new(IconName::Search), row_height, &style)
            .child(card_input(
                &self.input,
                self.input_id,
                self.input_label,
                &style,
            ))
            .when_some(self.count, |row, (id, count)| {
                row.child(
                    div()
                        .id(id)
                        .test_support()
                        .aria_label(count.clone())
                        .flex_none()
                        .mx(px(6.))
                        .font_family(style.mono_font.clone())
                        .text_size(style.mono_size)
                        .text_color(style.dim)
                        .child(count),
                )
            })
            .child(
                div()
                    .flex_none()
                    .w(px(1.))
                    .h(px(16.))
                    .mx(px(4.))
                    .bg(style.separator),
            )
            .children(self.buttons.into_iter().map(|button| button.render(&style)));
        let replace_row = self.replace.map(|row| {
            card_row(row.icon, row_height, &style)
                .border_t_1()
                .border_color(style.separator)
                .child(card_input(
                    &row.input,
                    row.input_id,
                    row.input_label,
                    &style,
                ))
                .children(row.buttons.into_iter().map(|button| button.render(&style)))
        });
        div()
            // 下面的终端、编辑器不能接到卡片上的点击（会挪焦点、开始选择）。
            .occlude()
            .absolute()
            .top(TOP - self.offset.0)
            .right(RIGHT - self.offset.1)
            .w(if replace_row.is_some() {
                REPLACE_WIDTH
            } else {
                WIDTH
            })
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(RADIUS)
            .bg(style.background)
            .border_1()
            .border_color(style.border)
            .shadow(vec![BoxShadow {
                color: style.shadow,
                offset: point(px(0.), px(4.)),
                blur_radius: px(14.),
                spread_radius: px(0.),
                inset: false,
            }])
            .font_family(style.font.clone())
            .text_size(style.text_size)
            .text_color(style.text)
            .child(find_row)
            .children(replace_row)
    }
}
