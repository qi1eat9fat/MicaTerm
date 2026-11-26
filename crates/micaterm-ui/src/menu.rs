//! 右键菜单（照 Muxy 空壳的右键菜单）：最小宽 200、四周留 5、圆角 8、外圈 1px 深一档的线；
//! 每项高 21、圆角 5、左右留 6，文字和右边的快捷键之间至少 16；悬停或键盘选中是强调色底、白字；
//! 快捷键用等宽字体、淡色，按基线和文字对齐再上移 0.5（用户定的全局规则）；
//! 置灰项文字更淡，危险项（删除、断开连接）是红字；分隔是一条 1px 线，上下各留 3。
//! 勾选项（「显示隐藏文件」）开着时右边画一个对勾，在快捷键的位置。
//!
//! 行为照 gpui-component 的 `PopupMenu`，调用方几乎不用改：`Menu::build` 建，`action_context`
//! 指定 action 送到哪个焦点；点一项（或 ↑↓ 选、回车）先把焦点交回 `action_context` 再派发，
//! 然后发 `DismissEvent`；Esc、点菜单外面也发 `DismissEvent`。菜单怎么弹出、什么时候放掉由调用方管
//! （`anchored().position(鼠标位置)`）。
//!
//! 测试靠的约定也照旧：根元素 id 是 `popup-menu`，第 i 项（分隔也算一项）的 id 是 `i`、
//! 无障碍名称是文字。

use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// 右键菜单、链接提示这类浮层与窗口边缘之间至少留的空白（调用方传给 `snap_to_window_with_margin`）。
pub const WINDOW_MARGIN: Pixels = px(8.);

/// 菜单画在什么颜色、什么字上。应用按自己的 token 设成全局量；没设时用一套中性灰。
#[derive(Clone, Debug, PartialEq)]
pub struct MenuStyle {
    pub background: Hsla,
    /// 外圈的线（`--line-strong`）。
    pub border: Hsla,
    /// 菜单底下的阴影。
    pub shadows: Vec<BoxShadow>,
    /// 分隔线（`--line-strong`，和菜单外圈同色；`--line` 是按内容底混的，在面板上几乎看不见）。
    pub separator: Hsla,
    pub text: Hsla,
    /// 快捷键（`--st-dim`）。
    pub hint: Hsla,
    /// 置灰项的文字和快捷键（`--faint`）。
    pub disabled: Hsla,
    /// 危险项（`--err`）。
    pub danger: Hsla,
    /// 悬停、键盘选中的底（强调色）。
    pub selected: Hsla,
    /// 悬停、键盘选中时的文字（强调色上的字）。
    pub selected_text: Hsla,
    /// 文字的字体、字号；快捷键的字体、字号。
    pub font: SharedString,
    pub text_size: Pixels,
    pub hint_font: SharedString,
    pub hint_size: Pixels,
    /// 菜单几何尺寸（由应用主题统一注入）。
    pub min_width: Pixels,
    pub row_height: Pixels,
    pub padding: Pixels,
    pub radius: Pixels,
    pub item_radius: Pixels,
}

impl Global for MenuStyle {}

impl Default for MenuStyle {
    fn default() -> Self {
        let gray = crate::default_gray;
        Self {
            background: gray(0.98),
            border: gray(0.82),
            // 阴影由应用在启动时按主题注入（`src/settings/apply.rs`），这里不内置。
            shadows: Vec::new(),
            separator: gray(0.88),
            text: gray(0.15),
            hint: gray(0.5),
            disabled: gray(0.66),
            danger: hsla(0., 0.6, 0.48, 1.),
            selected: gray(0.87),
            selected_text: gray(0.15),
            font: "".into(),
            text_size: px(11.),
            hint_font: "".into(),
            hint_size: px(11.),
            min_width: px(160.),
            row_height: px(20.),
            padding: px(4.),
            radius: px(8.),
            item_radius: px(4.),
        }
    }
}

impl MenuStyle {
    pub fn current(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }
}

/// 文字和快捷键之间至少这么宽。
const HINT_GAP: Pixels = px(14.);

enum MenuItem {
    Separator,
    Action {
        label: SharedString,
        action: Box<dyn Action>,
        /// 快捷键按哪个 action 的绑定写；`None` 是 `action` 自己的。
        keys_of: Option<Box<dyn Action>>,
        /// 直接写出来的快捷键（键不是绑在 action 上、而是列表自己处理的，比如主机列表的回车）。
        keys_text: Option<SharedString>,
        disabled: bool,
        danger: bool,
        /// 勾选项：`Some(开着没有)`。
        checked: Option<bool>,
    },
}

impl MenuItem {
    fn clickable(&self) -> bool {
        matches!(
            self,
            Self::Action {
                disabled: false,
                ..
            }
        )
    }
}

/// 一个右键菜单。
pub struct Menu {
    focus_handle: FocusHandle,
    action_context: Option<FocusHandle>,
    items: Vec<MenuItem>,
    /// 悬停或键盘选中的那一项。
    selected: Option<usize>,
}

impl EventEmitter<DismissEvent> for Menu {}

/// `.when(条件, |menu| …)` 这样按条件加项。
impl FluentBuilder for Menu {}

impl Focusable for Menu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Menu {
    /// 建一个菜单：`build` 往里加项。
    pub fn build(
        window: &mut Window,
        cx: &mut App,
        build: impl FnOnce(Self, &mut Window, &mut Context<Self>) -> Self,
    ) -> Entity<Self> {
        cx.new(|cx| {
            let menu = Self {
                focus_handle: cx.focus_handle(),
                action_context: None,
                items: Vec::new(),
                selected: None,
            };
            build(menu, window, cx)
        })
    }

    /// 项派发的 action 送到 `handle` 所在的焦点路径（窗口壳、终端……），关菜单后焦点也回到它。
    pub fn action_context(mut self, handle: FocusHandle) -> Self {
        self.action_context = Some(handle);
        self
    }

    /// 一项。
    pub fn menu(self, label: impl Into<SharedString>, action: Box<dyn Action>) -> Self {
        self.push(label, action, false, false)
    }

    /// 一项，`disabled` 时置灰、点不了。
    pub fn menu_with_disabled(
        self,
        label: impl Into<SharedString>,
        action: Box<dyn Action>,
        disabled: bool,
    ) -> Self {
        self.push(label, action, disabled, false)
    }

    /// 危险的一项（删除、断开连接）：红字。`disabled` 时照样置灰。
    pub fn danger(
        self,
        label: impl Into<SharedString>,
        action: Box<dyn Action>,
        disabled: bool,
    ) -> Self {
        self.push(label, action, disabled, true)
    }

    /// 勾选项：`checked` 时右边画对勾，点它派发 `action`（由 action 自己切换）。
    pub fn check(
        mut self,
        label: impl Into<SharedString>,
        checked: bool,
        action: Box<dyn Action>,
    ) -> Self {
        self = self.push(label, action, false, false);
        if let Some(MenuItem::Action { checked: slot, .. }) = self.items.last_mut() {
            *slot = Some(checked);
        }
        self
    }

    /// 上一项的快捷键按 `action` 的绑定写：项派发的是带参数的 action（「关闭这张卡片」），
    /// 键绑在不带参数的那个上（「关闭焦点卡片」 Ctrl+Shift+W），两者在这里是一回事。
    pub fn keys_of(mut self, action: Box<dyn Action>) -> Self {
        if let Some(MenuItem::Action { keys_of, .. }) = self.items.last_mut() {
            *keys_of = Some(action);
        }
        self
    }

    /// 上一项右边直接写 `keys`（键由别处自己处理、查不到绑定时）。
    pub fn keys(mut self, keys: impl Into<SharedString>) -> Self {
        if let Some(MenuItem::Action { keys_text, .. }) = self.items.last_mut() {
            *keys_text = Some(keys.into());
        }
        self
    }

    /// 分隔线。开头、结尾和连着的分隔不画。
    pub fn separator(mut self) -> Self {
        if !matches!(self.items.last(), None | Some(MenuItem::Separator)) {
            self.items.push(MenuItem::Separator);
        }
        self
    }

    /// 一项都没有。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn push(
        mut self,
        label: impl Into<SharedString>,
        action: Box<dyn Action>,
        disabled: bool,
        danger: bool,
    ) -> Self {
        self.items.push(MenuItem::Action {
            label: label.into(),
            action,
            keys_of: None,
            keys_text: None,
            disabled,
            danger,
            checked: None,
        });
        self
    }

    /// 执行第 `index` 项：焦点交回 `action_context`，派发，然后关菜单。
    fn confirm(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(MenuItem::Action {
            action,
            disabled: false,
            ..
        }) = self.items.get(index)
        else {
            return;
        };
        let action = action.boxed_clone();
        if let Some(context) = &self.action_context {
            context.focus(window, cx);
        }
        window.dispatch_action(action, cx);
        self.dismiss(window, cx);
    }

    /// 关菜单：发 `DismissEvent`；项没有把焦点挪到别处时，焦点回到 `action_context`。
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = None;
        cx.emit(DismissEvent);
        let moved_away =
            window.focused(cx).is_some() && !self.focus_handle.contains_focused(window, cx);
        if !moved_away && let Some(context) = &self.action_context {
            window.focus(context, cx);
        }
    }

    /// ↑↓ 在能点的项之间移动，到头了绕回去。
    fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let clickable: Vec<usize> = (0..self.items.len())
            .filter(|index| self.items[*index].clickable())
            .collect();
        let Some(&first) = clickable.first() else {
            return;
        };
        let last = *clickable.last().unwrap_or(&first);
        let next = match self.selected {
            None if down => first,
            None => last,
            Some(current) if down => clickable
                .iter()
                .copied()
                .find(|index| *index > current)
                .unwrap_or(first),
            Some(current) => clickable
                .iter()
                .rev()
                .copied()
                .find(|index| *index < current)
                .unwrap_or(last),
        };
        self.selected = Some(next);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        match event.keystroke.key.as_str() {
            "up" => self.step(false, cx),
            "down" => self.step(true, cx),
            "enter" => {
                if let Some(index) = self.selected {
                    self.confirm(index, window, cx);
                }
            }
            "escape" => self.dismiss(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// 快捷键：在 `action_context` 的焦点路径上找绑定，写成 `Ctrl+Shift+C`。
    fn hint(&self, action: &dyn Action, window: &Window) -> Option<SharedString> {
        let binding = match &self.action_context {
            Some(context) => window.highest_precedence_binding_for_action_in(action, context),
            None => window.highest_precedence_binding_for_action(action),
        }?;
        let stroke = binding.keystrokes().first()?;
        Some(gpui_kit::component::kbd::Kbd::format(stroke.as_keystroke()).into())
    }
}

/// 把快捷键挪到和文字同一条基线上要移多少（两段都是行高 `ROW`、盒子居中），再上移 0.5（用户定）。
fn hint_shift(style: &MenuStyle, window: &Window) -> Pixels {
    let text = window.text_system();
    let baseline = |family: &SharedString, size: Pixels| {
        let id = text.resolve_font(&font(family.clone()));
        text.baseline_offset(id, size, style.row_height)
    };
    baseline(&style.font, style.text_size) - baseline(&style.hint_font, style.hint_size) - px(0.5)
}

impl Render for Menu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = MenuStyle::current(cx);
        let row_height = style.row_height;
        let shift = hint_shift(&style, window);
        let rows: Vec<AnyElement> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| match item {
                MenuItem::Separator => div()
                    .id(index)
                    .test_support()
                    .flex_none()
                    .h(px(1.))
                    .my(px(2.5))
                    .mx(px(4.))
                    .bg(style.separator)
                    .into_any_element(),
                MenuItem::Action {
                    label,
                    action,
                    keys_of,
                    keys_text,
                    disabled,
                    danger,
                    checked,
                } => {
                    let hint = keys_text
                        .clone()
                        .or_else(|| self.hint(keys_of.as_ref().unwrap_or(action).as_ref(), window));
                    let color = if *disabled {
                        style.disabled
                    } else if *danger {
                        style.danger
                    } else {
                        style.text
                    };
                    let hint_color = if *disabled {
                        style.disabled
                    } else {
                        style.hint
                    };
                    let disabled = *disabled;
                    let selected = self.selected == Some(index) && !disabled;
                    let text_color = if selected { style.selected_text } else { color };
                    div()
                        .id(index)
                        .test_support()
                        .aria_label(label.clone())
                        .when_some(*checked, |row, checked| row.aria_selected(checked))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(HINT_GAP)
                        .h(row_height)
                        .px(px(6.))
                        .rounded(style.item_radius)
                        .text_color(text_color)
                        .whitespace_nowrap()
                        .when(selected, |row| row.bg(style.selected))
                        .when(!disabled, |row| {
                            row.cursor_pointer()
                                .on_hover(cx.listener(move |menu, hovered: &bool, _, cx| {
                                    if *hovered {
                                        menu.selected = Some(index);
                                    } else if menu.selected == Some(index) {
                                        menu.selected = None;
                                    }
                                    cx.notify();
                                }))
                                .on_click(cx.listener(move |menu, _, window, cx| {
                                    cx.stop_propagation();
                                    menu.confirm(index, window, cx);
                                }))
                        })
                        .child(div().flex_1().line_height(row_height).child(label.clone()))
                        .children(hint.map(|hint| {
                            div()
                                .flex_none()
                                .relative()
                                .top(shift)
                                .line_height(row_height)
                                .font_family(style.hint_font.clone())
                                .text_size(style.hint_size)
                                .text_color(if selected {
                                    style.selected_text.opacity(0.7)
                                } else {
                                    hint_color
                                })
                                .child(hint)
                        }))
                        .when(*checked == Some(true), |row| {
                            row.child(
                                Icon::new(IconName::Check)
                                    .size(px(11.))
                                    .flex_none()
                                    .text_color(text_color),
                            )
                        })
                        .into_any_element()
                }
            })
            .collect();
        // 结尾的分隔不画（中间的已经在 `separator` 里去重了）。
        let rows = {
            let mut rows = rows;
            if matches!(self.items.last(), Some(MenuItem::Separator)) {
                rows.pop();
            }
            rows
        };
        div()
            .id("popup-menu")
            .test_support()
            .key_context("MicaTermMenu")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down_out(cx.listener(|menu, _, window, cx| menu.dismiss(window, cx)))
            .occlude()
            .flex()
            .flex_col()
            .min_w(style.min_width)
            .p(style.padding)
            .rounded(style.radius)
            .bg(style.background)
            .border_1()
            .border_color(style.border)
            .shadow(style.shadows)
            .font_family(style.font.clone())
            .text_size(style.text_size)
            .children(rows)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, actions};

    use super::{Menu, MenuItem, WINDOW_MARGIN};

    actions!(menu_test, [First, Second, Third]);

    /// 菜单贴边留白仍是 8px。
    #[test]
    fn window_margin_stays_8px() {
        assert_eq!(WINDOW_MARGIN, gpui_kit::px(8.));
    }

    #[gpui_kit::test]
    fn separators_never_lead_or_double(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let menu = cx.update(|window, cx| {
            Menu::build(window, cx, |menu, _, _| {
                menu.separator()
                    .menu("一", Box::new(First))
                    .separator()
                    .separator()
                    .menu("二", Box::new(Second))
                    .separator()
            })
        });
        let separators: Vec<bool> = cx.update(|_, cx| {
            menu.read(cx)
                .items
                .iter()
                .map(|item| matches!(item, MenuItem::Separator))
                .collect()
        });
        // 结尾那条在 render 时去掉。
        assert_eq!(separators, [false, true, false, true]);
    }

    #[gpui_kit::test]
    fn arrows_skip_separators_and_disabled_items_and_wrap(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let menu = cx.update(|window, cx| {
            Menu::build(window, cx, |menu, _, _| {
                menu.menu("一", Box::new(First))
                    .separator()
                    .menu_with_disabled("二", Box::new(Second), true)
                    .danger("三", Box::new(Third), false)
            })
        });
        let mut step = |down: bool| {
            menu.update(cx, |menu, cx| {
                menu.step(down, cx);
                menu.selected
            })
        };
        assert_eq!(step(true), Some(0));
        assert_eq!(step(true), Some(3));
        assert_eq!(step(true), Some(0));
        assert_eq!(step(false), Some(3));
    }
}
