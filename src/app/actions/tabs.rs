//! 标签的 action（工单 04 补全切换、关闭等）。

gpui_kit::actions!(
    qishell,
    [
        /// 新建本地终端标签（Ctrl+Shift+T）：里面是一张本地卡片。
        NewLocalTerminalTab,
    ]
);
