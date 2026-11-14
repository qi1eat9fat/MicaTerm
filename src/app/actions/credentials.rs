//! 凭据的 action（工单 14）：凭据面板的「+」、双击和回车、右键菜单都派发它们，由窗口壳的处理器
//! （`workspace/handlers/credentials.rs`）交给 `crate::credential`。展开、收起面板的 `ShowCredentials`
//! 在 `window` 里（右侧栏图标）。

use gpui_kit::Action;

use micaterm_hosts::CredentialId;

gpui_kit::actions!(
    micaterm,
    [
        /// 新建凭据：打开凭据对话框（凭据面板标题栏的「+」）。
        NewCredential,
    ]
);

/// 编辑这条凭据（双击、回车、右键菜单「编辑…」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct EditCredential(pub CredentialId);

/// 把这条密钥凭据的公钥放进剪贴板（右键菜单「复制公钥」，只有密钥凭据有）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct CopyCredentialPublicKey(pub CredentialId);

/// 删除这条凭据，先确认（右键菜单「删除」）。在用它的主机改成「密码」或「无密码」，用户名保留。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = micaterm, no_json)]
pub struct DeleteCredential(pub CredentialId);
