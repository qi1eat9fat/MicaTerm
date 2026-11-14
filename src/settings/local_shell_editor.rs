//! 终端 › 本地终端 › Shell：一个输入框写程序和参数，末尾的下拉列出本机装了的常用 shell。
//! 每次改动立刻写进设置；本地终端在新开时用它。留空是自动检测。
// 来自 since2006/shell-rs @ b2a307a: src/settings/local_shell_editor.rs（裁剪：去掉「选择文件…」和切换语言）

use gpui_kit::component::{
    Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui_kit::*;

use micaterm_terminal::{default_shell_name, installed_shells};

use super::SettingsStore;
use super::controls::field_input;
use super::pages::WIDE_WIDTH;
use crate::i18n::t;
use crate::theme::Tokens;

/// 输入框的文字（设置在它下面变时也保住光标），和下拉里列的 shell。
pub struct LocalShellEditor {
    store: Entity<SettingsStore>,
    input: Entity<InputState>,
    /// 自动检测会用哪个 shell，和本机装了的；后台找回来之前是 `None`。
    found: Option<FoundShells>,
    _subscriptions: [Subscription; 2],
    _scan: Task<()>,
}

struct FoundShells {
    default: String,
    installed: Vec<String>,
}

impl LocalShellEditor {
    pub fn new(store: Entity<SettingsStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shell = stored(&store, cx).unwrap_or_default();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("settings.local_terminal.automatic_unknown"))
                .default_value(shell)
        });
        let subscriptions = [
            cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let shell = input.read(cx).value().to_string();
                    this.write(Some(shell), cx);
                }
            }),
            // 输入框自己的改动回来时一样（只差两头的空白）；别处的改动显示到输入框里。
            cx.observe_in(&store, window, |this, store, window, cx| {
                let shell = stored(&store, cx).unwrap_or_default();
                if this.input.read(cx).value().trim() != shell {
                    this.input
                        .update(cx, |input, cx| input.set_value(shell, window, cx));
                }
            }),
        ];
        let scan = cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async {
                    FoundShells {
                        default: default_shell_name(),
                        installed: installed_shells(),
                    }
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.found = Some(found);
                let placeholder = this.automatic_label();
                this.input.update(cx, |input, cx| {
                    input.set_placeholder(placeholder, window, cx)
                });
                cx.notify();
            })
            .ok();
        });
        Self {
            store,
            input,
            found: None,
            _subscriptions: subscriptions,
            _scan: scan,
        }
    }

    /// 「自动检测（pwsh.exe）」：知道默认 shell 以后带上它的名字。
    fn automatic_label(&self) -> SharedString {
        match &self.found {
            Some(found) => t!(
                "settings.local_terminal.automatic",
                shell = found.default.as_str()
            ),
            None => t!("settings.local_terminal.automatic_unknown"),
        }
    }

    /// 从下拉里选：`None` 是自动检测。
    fn choose(&mut self, shell: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_value(shell.clone().unwrap_or_default(), window, cx)
        });
        self.write(shell, cx);
    }

    fn write(&self, shell: Option<String>, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            store.update(|settings| settings.local_terminal.shell = shell, cx)
        });
    }
}

/// 设置里存的 shell，`None` 是自动检测。
fn stored(store: &Entity<SettingsStore>, cx: &App) -> Option<String> {
    store.read(cx).settings().local_terminal.shell
}

impl Render for LocalShellEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = stored(&self.store, cx);
        let automatic = self.automatic_label();
        let installed = self
            .found
            .as_ref()
            .map(|found| found.installed.clone())
            .unwrap_or_default();
        let editor = cx.entity().downgrade();
        let tokens = Tokens::current(cx);
        let menu = Button::new("local-shell-menu")
            .ghost()
            .xsmall()
            .icon(
                Icon::new(IconName::ChevronDown)
                    .size(px(12.))
                    .text_color(tokens.st_dim),
            )
            .tooltip(t!("settings.local_terminal.installed"))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let item = |label: SharedString, shell: Option<String>| {
                    let editor = editor.clone();
                    PopupMenuItem::new(label)
                        .checked(current == shell)
                        .on_click(move |_, window, cx| {
                            editor
                                .update(cx, |editor, cx| editor.choose(shell.clone(), window, cx))
                                .ok();
                        })
                };
                let mut menu = menu.item(item(automatic.clone(), None));
                if !installed.is_empty() {
                    menu = menu.separator();
                }
                installed.iter().fold(menu, |menu, shell| {
                    menu.item(item(shell.clone().into(), Some(shell.clone())))
                })
            });
        // 26 号 `.stg-in`：界面字体（等宽字体会把中文的占位字拉开）。
        div().w(WIDE_WIDTH).child(field_input(
            Input::new(&self.input)
                .id("local-shell")
                .aria_label(t!("settings.local_terminal.shell"))
                .suffix(menu),
            &tokens,
        ))
    }
}
