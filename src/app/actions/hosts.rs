//! 主机的 action：主机栏的搜索、右键菜单、拖动排序（工单 11）。新建主机（`NewHost`）在 `window`。
//!
//! 连接类的 action（`ConnectHost`、`ConnectHostRight`、`ConnectHostDown`、`OpenHostSftp`）
//! 先在这里定好，主机栏的双击、回车和右键菜单已经派发它们；处理器由后面的工单接上：
//! 12 接 `ConnectHost`，16 接分屏打开，M3 接 SFTP（右键菜单里的项由 `host::menu` 的开关控制显示）。

use gpui_kit::Action;

use qishell_hosts::{HostDrop, HostId};

gpui_kit::actions!(
    qishell,
    [
        /// 聚焦主机栏的搜索框（Ctrl+Shift+H）；主机栏收着时先展开。
        FocusHostSearch,
    ]
);

/// 编辑这台主机（右键菜单「编辑…」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct EditHost(pub HostId);

/// 复制这台主机，名字后加「 副本」、不带密码，放在原主机后面（右键菜单「创建副本」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct DuplicateHost(pub HostId);

/// 把这台主机的地址放进剪贴板（右键菜单「复制 IP」，地址是域名时叫「复制主机名」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct CopyHostAddress(pub HostId);

/// 删除这台主机，先确认（右键菜单「删除」）。已经打开的卡片不受影响。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct DeleteHost(pub HostId);

/// 把主机拖到另一台主机的前面或后面（主机栏拖动排序）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct MoveHost {
    pub host: HostId,
    pub drop: HostDrop,
}

/// 在新标签里连接这台主机（双击、回车、右键菜单「连接」）。处理器在工单 12 接上。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ConnectHost(pub HostId);

/// 在焦点卡片右侧分屏打开这台主机（右键菜单）。处理器在工单 16 接上。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ConnectHostRight(pub HostId);

/// 在焦点卡片下方分屏打开这台主机（右键菜单）。处理器在工单 16 接上。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ConnectHostDown(pub HostId);

/// 打开这台主机的 SFTP 标签，已经有就切过去（右键菜单）。处理器在 M3 接上。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct OpenHostSftp(pub HostId);
