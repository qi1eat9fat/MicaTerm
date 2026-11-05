//! 窗口壳的 action：主机栏、右侧栏、新建主机、快速连接、退出。

gpui_kit::actions!(
    qishell,
    [
        /// 显示或隐藏主机栏（Ctrl+Shift+B）。
        ToggleHostSidebar,
        /// 新建主机（Ctrl+Shift+N）。主机对话框在 M2（工单 11）接上。
        NewHost,
        /// 快速连接（Ctrl+Shift+O）。在 M2（工单 18）接上。
        QuickConnect,
        /// 右侧栏「文件」：打开或切换到 SFTP 标签（Ctrl+Shift+U）。在 M3 接上。
        OpenSftp,
        /// 展开或收起右侧栏的「命令片段」面板（Ctrl+Shift+S）。
        ShowSnippets,
        /// 右侧栏「凭据」面板。在 M2（工单 14）接上。
        ShowCredentials,
        /// 退出（macOS 的 ⌘Q；Windows 和 Linux 用系统自带的 Alt+F4）。
        Quit,
    ]
);
