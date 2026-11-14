//! 设置的 action。

gpui_kit::actions!(
    micaterm,
    [
        /// 打开设置对话框（Ctrl+,）。
        OpenSettings,
        /// 标题栏的深浅切换：把「应用外观」改成和现在相反的固定浅色或深色（26 号，用户定）。
        /// 想回到跟随系统，去设置里改。
        ToggleAppearance,
    ]
);
