//! 远程卡片的 action（工单 12）：远程终端右键菜单里的重新连接、断开连接、复制 IP，
//! 以及断开后在卡片里按 Enter 原地重连（按 Space 关闭用的是 `CloseCard`）。
//! 「打开 SFTP」按焦点卡片的规则打开这张卡片所在主机的 SFTP 标签（工单 20）。

use gpui_kit::Action;

use crate::card::CardId;

/// 远程卡片原地重连：按打开时的主机重新取一次登录（编辑过就用新的，主机删了就用打开时的）。
/// 断开后在卡片里按 Enter，或者右键菜单「重新连接」。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct ReconnectCard(pub CardId);

/// 断开这张远程卡片（右键菜单「断开连接」）：关掉它的 shell 通道，卡片留着，可以按 Enter 重连。
/// 同一条连接上的别的卡片不受影响；连接上没有别的通道了才断开连接。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct DisconnectCard(pub CardId);

/// 打开这张远程卡片所在主机的 SFTP 标签，已经有就切过去（终端右键菜单「打开 SFTP」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct OpenCardSftp(pub CardId);

/// 把远程卡片主机的地址放进剪贴板（右键菜单「复制 IP」，地址是域名时叫「复制主机名」）。
#[derive(Action, Clone, Copy, Debug, PartialEq, Eq)]
#[action(namespace = qishell, no_json)]
pub struct CopyCardAddress(pub CardId);
