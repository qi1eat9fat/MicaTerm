//! 保险库的 action（工单 13）：主机栏底部的锁按钮、主机对话框里「解锁…」、设置 › 安全都派发它们，
//! 由窗口壳的处理器（`workspace/handlers/vault.rs`）交给 `crate::vault`。

gpui_kit::actions!(
    micaterm,
    [
        /// 立即锁定保险库（主机栏底部按钮、设置 › 安全、凭据面板状态条）。没设主密码时什么都不做。
        LockVault,
        /// 弹出解锁小卡片（「解锁…」）。手动入口一直可用，取消过自动解锁也一样。
        UnlockVault,
        /// 设置或更换主密码（设置 › 安全）：没设时弹「设置主密码」，设了弹「更换主密码」。
        SetMasterPassword,
        /// 重置保险库，先确认（设置 › 安全）：清空所有秘密，主机和凭据保留。
        ResetVault,
    ]
);
