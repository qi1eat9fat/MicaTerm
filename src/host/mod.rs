//! 主机管理：主机核心（`qishell-hosts`）在应用里的样子，加上应用自己的部分——
//! 包住数据的 GPUI 实体 `HostStore`（写穿、报告失败）。
//! 主机栏、主机对话框、快速连接由后续工单加在这里（11、18）。

mod store;

pub use qishell_hosts::*;
pub use store::{HostStore, HostStoreEvent};
