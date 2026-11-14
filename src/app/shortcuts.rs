//! 快捷键表（spec 第 8 节）：v1 固定，不能改。
//!
//! 按领域拆成几张静态表，各工单只往自己那张里加：
//! - `WINDOW_KEYS`：窗口壳（主机栏、右侧栏、新建主机、快速连接、设置、退出）；
//! - `TAB_KEYS`：标签（工单 04）；
//! - `CARD_KEYS`：卡片（工单 05）；
//! - `TERMINAL_KEYS`：终端（工单 03，键上下文是终端 crate 的 `Terminal` / `TerminalFind`）；
//! - `LOCAL_FILE_KEYS`、`REMOTE_FILE_KEYS`：SFTP 标签的文件列表（工单 20，一次写全）；
//! - `PREVIEW_KEYS`：预览里的图片（工单 24）。
//!
//! 规则：Windows 和 Linux 用 Ctrl+Shift 系，macOS 用 ⌘；全局绑定里不能出现只按 Ctrl 加字母的组合
//! （留给终端里的 shell）。绑定一律经 `KeyBinding::load` 加平台的 keyboard mapper 建，
//! 否则 Windows 上 Ctrl+Shift+数字这类绑定永远匹配不上（无头测试测不出，只能真机验证）。

use std::rc::Rc;

use gpui_kit::{Action, App, KeyBinding, KeyBindingContextPredicate, NoAction};

use micaterm_terminal::actions::*;

use super::actions::*;
use crate::sftp_tab::{CardSide, CursorMotion, NewEntryKind};
use crate::split::Direction;

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
    // 聚焦主机搜索（工单 11）。
    Shortcut {
        keys: "ctrl-shift-h",
        mac: "cmd-shift-h",
        action: || Box::new(FocusHostSearch),
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

/// 卡片的快捷键（工单 05）：分屏、关闭、放大、焦点移动、移动分隔线。
pub const CARD_KEYS: &[Shortcut] = &[
    Shortcut {
        keys: "ctrl-shift-d",
        mac: "cmd-d",
        action: || Box::new(SplitRight),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-e",
        mac: "cmd-shift-d",
        action: || Box::new(SplitDown),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-w",
        mac: "cmd-w",
        action: || Box::new(CloseFocusedCard),
        context: None,
    },
    // 关闭整个标签：Windows 和 Linux 没有快捷键（用标签上的 × 或右键菜单），只给 macOS 绑。
    Shortcut {
        keys: "",
        mac: "cmd-shift-w",
        action: || Box::new(CloseActiveTab),
        context: None,
    },
    Shortcut {
        keys: "ctrl-shift-enter",
        mac: "cmd-shift-enter",
        action: || Box::new(ToggleZoomCard),
        context: None,
    },
    Shortcut {
        keys: "alt-left",
        mac: "alt-cmd-left",
        action: || Box::new(FocusCard(Direction::Left)),
        context: None,
    },
    Shortcut {
        keys: "alt-right",
        mac: "alt-cmd-right",
        action: || Box::new(FocusCard(Direction::Right)),
        context: None,
    },
    Shortcut {
        keys: "alt-up",
        mac: "alt-cmd-up",
        action: || Box::new(FocusCard(Direction::Up)),
        context: None,
    },
    Shortcut {
        keys: "alt-down",
        mac: "alt-cmd-down",
        action: || Box::new(FocusCard(Direction::Down)),
        context: None,
    },
    Shortcut {
        keys: "alt-shift-left",
        mac: "ctrl-cmd-left",
        action: || Box::new(MoveDivider(Direction::Left)),
        context: None,
    },
    Shortcut {
        keys: "alt-shift-right",
        mac: "ctrl-cmd-right",
        action: || Box::new(MoveDivider(Direction::Right)),
        context: None,
    },
    Shortcut {
        keys: "alt-shift-up",
        mac: "ctrl-cmd-up",
        action: || Box::new(MoveDivider(Direction::Up)),
        context: None,
    },
    Shortcut {
        keys: "alt-shift-down",
        mac: "ctrl-cmd-down",
        action: || Box::new(MoveDivider(Direction::Down)),
        context: None,
    },
];

/// 只在终端里生效。
const IN_TERMINAL: Option<&str> = Some(micaterm_terminal::TERMINAL_KEY_CONTEXT);
/// 在终端里和它的查找条里都生效：查找条和终端是兄弟节点，不在同一条键上下文路径上。
const IN_TERMINAL_OR_FIND: Option<&str> = Some("Terminal || TerminalFind");

/// 终端的快捷键（工单 03）。转发给焦点卡片的终端，见 `workspace/handlers/terminal.rs`。
/// 字号的 Ctrl+= / Ctrl+- / Ctrl+0 是 Ctrl 加符号，本来就是终端的功能，只绑在终端里。
pub const TERMINAL_KEYS: &[Shortcut] = &[
    Shortcut {
        keys: "ctrl-shift-c",
        mac: "cmd-c",
        action: || Box::new(CopyTerminal),
        context: IN_TERMINAL,
    },
    Shortcut {
        keys: "ctrl-shift-v",
        mac: "cmd-v",
        action: || Box::new(PasteTerminal),
        context: IN_TERMINAL,
    },
    Shortcut {
        keys: "ctrl-shift-a",
        mac: "cmd-a",
        action: || Box::new(SelectAllTerminal),
        context: IN_TERMINAL,
    },
    Shortcut {
        keys: "ctrl-shift-k",
        mac: "cmd-k",
        action: || Box::new(ClearTerminal),
        context: IN_TERMINAL,
    },
    Shortcut {
        keys: "ctrl-shift-f",
        mac: "cmd-f",
        action: || Box::new(FindInTerminal),
        context: IN_TERMINAL_OR_FIND,
    },
    Shortcut {
        keys: "f3",
        mac: "cmd-g",
        action: || Box::new(FindNextInTerminal),
        context: IN_TERMINAL_OR_FIND,
    },
    Shortcut {
        keys: "shift-f3",
        mac: "cmd-shift-g",
        action: || Box::new(FindPreviousInTerminal),
        context: IN_TERMINAL_OR_FIND,
    },
    Shortcut {
        keys: "escape",
        mac: "escape",
        action: || Box::new(DismissTerminalFind),
        context: Some(micaterm_terminal::TERMINAL_FIND_KEY_CONTEXT),
    },
    Shortcut {
        keys: "ctrl-=",
        mac: "cmd-=",
        action: || Box::new(ZoomTerminalIn),
        context: IN_TERMINAL_OR_FIND,
    },
    Shortcut {
        keys: "ctrl--",
        mac: "cmd--",
        action: || Box::new(ZoomTerminalOut),
        context: IN_TERMINAL_OR_FIND,
    },
    Shortcut {
        keys: "ctrl-0",
        mac: "cmd-0",
        action: || Box::new(ZoomTerminalReset),
        context: IN_TERMINAL_OR_FIND,
    },
];

/// 本地文件列表的键上下文（文件卡片的列表外面那一层）。
pub const LOCAL_FILE_LIST_CONTEXT: &str = "LocalFileList";
/// 远程文件列表的键上下文。
pub const REMOTE_FILE_LIST_CONTEXT: &str = "RemoteFileList";

/// 一侧文件列表的按键（spec 第 8 节，照 shell-rs，去掉 Ctrl+H、Ctrl+F、Ctrl+D、Alt+方向键、Ctrl+\）。
// 参考 since2006/shell-rs @ b2a307a: src/app/shortcuts.rs 的 `explorer_shortcuts`
macro_rules! file_list_keys {
    ($side:expr, $list:literal) => {
        &[
            file_list_key!("enter", "enter", $list, Open { side: $side }),
            file_list_key!("f4", "f4", $list, Edit { side: $side, path: None }),
            file_list_key!("f5", "f5", $list, Transfer { side: $side }),
            file_list_key!("f2", "f2", $list, Rename { side: $side }),
            file_list_key!("f7", "f7", $list, New { side: $side, kind: NewEntryKind::Folder }),
            // 后写的优先：菜单上要写 Delete（原型），所以它排在 F8 后面。
            file_list_key!("f8", "f8", $list, Delete { side: $side }),
            file_list_key!("delete", "cmd-backspace", $list, Delete { side: $side }),
            file_list_key!("f9", "cmd-i", $list, Properties { side: $side }),
            file_list_key!("backspace", "backspace", $list, Up { side: $side }),
            file_list_key!("ctrl-r", "cmd-r", $list, Refresh { side: $side }),
            file_list_key!("ctrl-o", "cmd-o", $list, EditPath { side: $side }),
            file_list_key!("ctrl-a", "cmd-a", $list, SelectAll { side: $side }),
            file_list_key!("space", "space", $list, ToggleSelection { side: $side }),
            file_list_key!("insert", "insert", $list, ToggleSelection { side: $side }),
            file_list_key!("tab", "tab", $list, FocusCard { side: $side.other() }),
            file_list_key!("shift-tab", "shift-tab", $list, FocusCard { side: $side.other() }),
            file_list_key!("up", "up", $list, MoveCursor { side: $side, motion: CursorMotion::Up, extend: false }),
            file_list_key!("down", "down", $list, MoveCursor { side: $side, motion: CursorMotion::Down, extend: false }),
            file_list_key!("pageup", "pageup", $list, MoveCursor { side: $side, motion: CursorMotion::PageUp, extend: false }),
            file_list_key!("pagedown", "pagedown", $list, MoveCursor { side: $side, motion: CursorMotion::PageDown, extend: false }),
            file_list_key!("home", "home", $list, MoveCursor { side: $side, motion: CursorMotion::Home, extend: false }),
            file_list_key!("end", "end", $list, MoveCursor { side: $side, motion: CursorMotion::End, extend: false }),
            file_list_key!("shift-up", "shift-up", $list, MoveCursor { side: $side, motion: CursorMotion::Up, extend: true }),
            file_list_key!("shift-down", "shift-down", $list, MoveCursor { side: $side, motion: CursorMotion::Down, extend: true }),
            file_list_key!("shift-pageup", "shift-pageup", $list, MoveCursor { side: $side, motion: CursorMotion::PageUp, extend: true }),
            file_list_key!("shift-pagedown", "shift-pagedown", $list, MoveCursor { side: $side, motion: CursorMotion::PageDown, extend: true }),
            file_list_key!("shift-home", "shift-home", $list, MoveCursor { side: $side, motion: CursorMotion::Home, extend: true }),
            file_list_key!("shift-end", "shift-end", $list, MoveCursor { side: $side, motion: CursorMotion::End, extend: true }),
            // 显示隐藏文件：WinSCP 的 Ctrl+Alt+H，访达的 ⌘⇧.（macOS 报成 ⌘>）。
            Shortcut {
                keys: "ctrl-alt-h",
                mac: "cmd->",
                action: || Box::new(ToggleHiddenFiles($side)),
                context: Some($list),
            },
        ]
    };
}

macro_rules! file_list_key {
    ($keys:literal, $mac:literal, $context:literal, $($command:tt)*) => {
        Shortcut {
            keys: $keys,
            mac: $mac,
            action: || Box::new(SftpShortcut(SftpTabCommand::$($command)*)),
            context: Some($context),
        }
    };
}

/// 本地文件列表的按键。
pub const LOCAL_FILE_KEYS: &[Shortcut] = file_list_keys!(CardSide::Local, "LocalFileList");
/// 远程文件列表的按键。
pub const REMOTE_FILE_KEYS: &[Shortcut] = file_list_keys!(CardSide::Remote, "RemoteFileList");

/// 编辑器标签的键上下文。
pub const EDITOR_CONTEXT: &str = "EditorTab";

/// 编辑器查找浮卡的键上下文（UI 重做工单 11）。
pub const EDITOR_FIND_CONTEXT: &str = "EditorFind";

/// 编辑器标签的按键（工单 23）。Ctrl+F、Ctrl+H 是编辑器自带的绑定（`Search`、`Replace`），
/// 编辑器标签截下来开自己的查找浮卡；Enter / Shift+Enter 由浮卡的输入框自己处理。
pub const EDITOR_KEYS: &[Shortcut] = &[
    Shortcut {
        keys: "ctrl-s",
        mac: "cmd-s",
        action: || Box::new(SaveEditor),
        context: Some(EDITOR_CONTEXT),
    },
    Shortcut {
        keys: "f3",
        mac: "cmd-g",
        action: || Box::new(FindNextInEditor),
        context: Some(EDITOR_CONTEXT),
    },
    Shortcut {
        keys: "shift-f3",
        mac: "cmd-shift-g",
        action: || Box::new(FindPreviousInEditor),
        context: Some(EDITOR_CONTEXT),
    },
    Shortcut {
        keys: "escape",
        mac: "escape",
        action: || Box::new(DismissEditorFind),
        context: Some(EDITOR_FIND_CONTEXT),
    },
    Shortcut {
        keys: "ctrl-alt-enter",
        mac: "cmd-alt-enter",
        action: || Box::new(ReplaceAllInEditor),
        context: Some(EDITOR_FIND_CONTEXT),
    },
];

/// 预览里图片的键上下文。
pub const IMAGE_PREVIEW_CONTEXT: &str = "ImagePreview";

/// 预览里图片的按键（工单 24，照 shell-rs）：和终端字号一样的键，焦点在预览里时缩放图片；
/// 0 是原图，9 是适合窗口（同 macOS 的「预览」）。
pub const PREVIEW_KEYS: &[Shortcut] = &[
    Shortcut {
        keys: "ctrl-=",
        mac: "cmd-=",
        action: || Box::new(ZoomPreviewIn),
        context: Some(IMAGE_PREVIEW_CONTEXT),
    },
    Shortcut {
        keys: "ctrl--",
        mac: "cmd--",
        action: || Box::new(ZoomPreviewOut),
        context: Some(IMAGE_PREVIEW_CONTEXT),
    },
    Shortcut {
        keys: "ctrl-0",
        mac: "cmd-0",
        action: || Box::new(ActualSizePreview),
        context: Some(IMAGE_PREVIEW_CONTEXT),
    },
    Shortcut {
        keys: "ctrl-9",
        mac: "cmd-9",
        action: || Box::new(FitPreview),
        context: Some(IMAGE_PREVIEW_CONTEXT),
    },
];

/// 全部快捷键。全局的在前、带上下文的在后：同一个键在上下文里后写的胜出。
pub fn all() -> impl Iterator<Item = &'static Shortcut> {
    let tables = [
        WINDOW_KEYS,
        TAB_KEYS,
        CARD_KEYS,
        TERMINAL_KEYS,
        LOCAL_FILE_KEYS,
        REMOTE_FILE_KEYS,
        EDITOR_KEYS,
        PREVIEW_KEYS,
    ];
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
    cx.bind_keys(micaterm_terminal::terminal_key_bindings());
    cx.bind_keys(shell_keys_in_terminal());
}

/// 在终端里，只按 Ctrl 加字母全部留给 shell（Ctrl+C 中断、tmux 的 Ctrl+B、readline 的 Ctrl+W）。
/// 按键先过绑定、再到终端，而 gpui-kit 的根节点把 Ctrl+C 绑成了复制：界面里有选中文字时，
/// 它就会吞掉中断。终端上下文里的 `NoAction` 比外层的绑定优先，把这些键让回给终端。
// 参考 since2006/shell-rs @ b2a307a: src/app/shortcuts.rs 的 `shell_keys_in_terminal`
fn shell_keys_in_terminal() -> Vec<KeyBinding> {
    ('a'..='z')
        .map(|letter| {
            KeyBinding::new(
                &format!("ctrl-{letter}"),
                NoAction {},
                Some(micaterm_terminal::TERMINAL_KEY_CONTEXT),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use gpui_kit::Keystroke;

    use super::{CARD_KEYS, all};

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

    /// 关闭整个标签只有 macOS 有快捷键 ⇧⌘W；Windows 和 Linux 用标签上的 × 或右键菜单（spec §8）。
    #[test]
    fn cmd_shift_w_closes_the_whole_tab_on_macos_only() {
        let close_tab = CARD_KEYS
            .iter()
            .find(|shortcut| shortcut.mac == "cmd-shift-w")
            .expect("⇧⌘W is bound");
        assert_eq!((close_tab.action)().name(), "micaterm::CloseActiveTab");
        assert_eq!(close_tab.keys, "");
        assert_eq!(close_tab.context, None);
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
