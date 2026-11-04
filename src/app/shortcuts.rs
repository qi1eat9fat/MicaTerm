//! 快捷键表（spec 第 8 节）：v1 固定，不能改。
//!
//! 按领域拆成几张静态表，各工单只往自己那张里加：
//! - `WINDOW_KEYS`：窗口壳（主机栏、右侧栏、新建主机、快速连接、设置、退出）；
//! - `TAB_KEYS`：标签（工单 04）；
//! - `CARD_KEYS`：卡片（工单 05）；
//! - `TERMINAL_KEYS`：终端（工单 03，键上下文是终端 crate 的 `Terminal` / `TerminalFind`）。
//!
//! 规则：Windows 和 Linux 用 Ctrl+Shift 系，macOS 用 ⌘；全局绑定里不能出现只按 Ctrl 加字母的组合
//! （留给终端里的 shell）。绑定一律经 `KeyBinding::load` 加平台的 keyboard mapper 建，
//! 否则 Windows 上 Ctrl+Shift+数字这类绑定永远匹配不上（无头测试测不出，只能真机验证）。

use std::rc::Rc;

use gpui_kit::{Action, App, KeyBinding, KeyBindingContextPredicate};

use super::actions::*;

/// 快捷键表里的一项。
pub struct Shortcut {
    /// Windows 和 Linux 上的按键，GPUI 的写法（`"ctrl-shift-b"`）。
    pub keys: &'static str,
    /// macOS 上的按键（`"cmd-b"`）。
    pub mac: &'static str,
    /// 派发的 action。
    pub action: fn() -> Box<dyn Action>,
    /// 只在这个键上下文里生效；`None` 是全局。
    pub context: Option<&'static str>,
}

impl Shortcut {
    /// 本平台上的按键。
    pub fn platform_keys(&self) -> &'static str {
        if cfg!(target_os = "macos") {
            self.mac
        } else {
            self.keys
        }
    }
}

/// 窗口壳的快捷键。
pub const WINDOW_KEYS: &[Shortcut] = &[
    Shortcut {
        keys: "ctrl-shift-b",
        mac: "cmd-b",
        action: || Box::new(ToggleHostSidebar),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-n",
        mac: "cmd-n",
        action: || Box::new(NewHost),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-o",
        mac: "cmd-shift-o",
        action: || Box::new(QuickConnect),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-u",
        mac: "cmd-shift-u",
        action: || Box::new(OpenSftp),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-s",
        mac: "cmd-shift-s",
        action: || Box::new(ShowSnippets),
        context: None,
    },
    Shortcut {
        keys: "ctrl-,",
        mac: "cmd-,",
        action: || Box::new(OpenSettings),
        context: None,
    },
    // Windows 和 Linux 的退出是系统自带的 Alt+F4，这里只给 macOS 绑。
    Shortcut {
        keys: "",
        mac: "cmd-q",
        action: || Box::new(Quit),
        context: None,
    },
];

/// 标签的快捷键（工单 02、04）。
pub const TAB_KEYS: &[Shortcut] = &[
    Shortcut {
        keys: "ctrl-shift-t",
        mac: "cmd-t",
        action: || Box::new(NewLocalTerminalTab),
        context: None,
    },
    // 切换标签是全局绑定：焦点在终端里也生效（绑定先于终端的按键处理）。
    Shortcut {
        keys: "ctrl-tab",
        mac: "ctrl-tab",
        action: || Box::new(NextTab),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-tab",
        mac: "ctrl-shift-tab",
        action: || Box::new(PreviousTab),
        context: None,
    },
    Shortcut {
        keys: "ctrl-pagedown",
        mac: "cmd-shift-]",
        action: || Box::new(NextTab),
        context: None,
    },
    Shortcut {
        keys: "ctrl-pageup",
        mac: "cmd-shift-[",
        action: || Box::new(PreviousTab),
        context: None,
    },
    // Windows 上 Shift+数字报成符号键，靠 `KeyBinding::load` 的 keyboard mapper 才匹配得上。
    Shortcut {
        keys: "ctrl-shift-1",
        mac: "cmd-1",
        action: || Box::new(SwitchToTab(1)),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-2",
        mac: "cmd-2",
        action: || Box::new(SwitchToTab(2)),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-3",
        mac: "cmd-3",
        action: || Box::new(SwitchToTab(3)),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-4",
        mac: "cmd-4",
        action: || Box::new(SwitchToTab(4)),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-5",
        mac: "cmd-5",
        action: || Box::new(SwitchToTab(5)),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-6",
        mac: "cmd-6",
        action: || Box::new(SwitchToTab(6)),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-7",
        mac: "cmd-7",
        action: || Box::new(SwitchToTab(7)),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-8",
        mac: "cmd-8",
        action: || Box::new(SwitchToTab(8)),
        context: None,
    },
    // 9 是最后一个。
    Shortcut {
        keys: "ctrl-shift-9",
        mac: "cmd-9",
        action: || Box::new(SwitchToTab(9)),
        context: None,
    },
];

/// 卡片的快捷键（工单 05 填写）。
pub const CARD_KEYS: &[Shortcut] = &[];

/// 终端的快捷键（工单 03 填写）。
pub const TERMINAL_KEYS: &[Shortcut] = &[];

/// 全部快捷键。全局的在前、带上下文的在后：同一个键在上下文里后写的胜出。
pub fn all() -> impl Iterator<Item = &'static Shortcut> {
    let tables = [WINDOW_KEYS, TAB_KEYS, CARD_KEYS, TERMINAL_KEYS];
    let global = tables
        .into_iter()
        .flatten()
        .filter(|shortcut| shortcut.context.is_none());
    let contextual = tables
        .into_iter()
        .flatten()
        .filter(|shortcut| shortcut.context.is_some());
    global.chain(contextual)
}

/// 把整张表绑上。在 `gpui_kit::init` 之后调用，gpui-kit 自己的绑定保留在前面。
pub fn bind(cx: &mut App) {
    let mapper = cx.keyboard_mapper().clone();
    let bindings = all()
        .filter(|shortcut| !shortcut.platform_keys().is_empty())
        .map(|shortcut| {
            let context = shortcut.context.map(|context| {
                Rc::new(KeyBindingContextPredicate::parse(context).expect("valid key context"))
            });
            KeyBinding::load(
                shortcut.platform_keys(),
                (shortcut.action)(),
                context,
                false,
                None,
                mapper.as_ref(),
            )
            .expect("valid keystroke")
        })
        .collect::<Vec<_>>();
    cx.bind_keys(bindings);
    // 终端里的 Tab 和 Shift+Tab 发给 shell，不让焦点跳走（终端 crate 自带的绑定）。
    cx.bind_keys(qishell_terminal::terminal_key_bindings());
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use gpui_kit::Keystroke;

    use super::all;

    #[test]
    fn every_key_parses() {
        for shortcut in all() {
            for keys in [shortcut.keys, shortcut.mac] {
                for key in keys.split_whitespace() {
                    assert!(Keystroke::parse(key).is_ok(), "{key}");
                }
            }
        }
    }

    #[test]
    fn no_key_is_bound_twice_in_one_context() {
        let mut seen = HashSet::new();
        for shortcut in all() {
            // 按平台分开查：同一个键在两个平台上都用（如 Ctrl+Tab）不算重复。
            for (mac, keys) in [(false, shortcut.keys), (true, shortcut.mac)] {
                if !keys.is_empty() {
                    assert!(seen.insert((mac, keys, shortcut.context)), "{keys} twice");
                }
            }
        }
    }

    /// 只按 Ctrl 加字母的组合全部留给终端里的 shell。
    #[test]
    fn ctrl_with_a_letter_alone_is_left_to_the_shell() {
        for shortcut in all().filter(|shortcut| shortcut.context.is_none()) {
            for key in shortcut.keys.split_whitespace() {
                let keystroke = Keystroke::parse(key).unwrap();
                let modifiers = keystroke.modifiers;
                let letter = keystroke.key.len() == 1
                    && keystroke.key.chars().all(|c| c.is_ascii_alphabetic());
                assert!(
                    !(letter && modifiers.control && !modifiers.shift && !modifiers.alt),
                    "{key}"
                );
            }
        }
    }
}
