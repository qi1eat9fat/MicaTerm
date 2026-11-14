//! 终端的动作，命名空间 `micaterm_terminal`。
//!
//! 右键菜单和查找条派发这些动作；应用在快捷键表里绑键（工单 03），由窗口壳转发给焦点卡片里的
//! `TerminalView`（`copy_selection`、`paste_clipboard`、`open_find`……）。
//! `SendTab`、`SendBackTab` 由 `TerminalView` 自己处理，见 `terminal_key_bindings`。

gpui_kit::actions!(
    micaterm_terminal,
    [
        /// 复制焦点终端里选中的文字。
        CopyTerminal,
        /// 把剪贴板粘贴进焦点终端。
        PasteTerminal,
        /// 全选焦点终端的文字，连同回滚。
        SelectAllTerminal,
        /// 打开焦点终端的查找条，已打开就重新选中查询。
        FindInTerminal,
        /// 查找：下一项（更新的输出）。
        FindNextInTerminal,
        /// 查找：上一项（更早的输出）。
        FindPreviousInTerminal,
        /// 关掉查找条和它的高亮。
        DismissTerminalFind,
        /// 清屏：清掉屏幕和回滚，只留提示符那一行。
        ClearTerminal,
        /// 终端字号大一号。
        ZoomTerminalIn,
        /// 终端字号小一号。
        ZoomTerminalOut,
        /// 终端字号恢复默认。
        ZoomTerminalReset,
        /// 把 Tab 发给 shell。
        SendTab,
        /// 把 Shift+Tab 发给 shell。
        SendBackTab,
    ]
);
