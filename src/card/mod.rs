//! 卡片：标签里的一格终端，本地卡片（本地 shell）或远程卡片（连到一台主机，工单 12）。
//!
//! - `view`：`TerminalCard` 实体，持有它的 `TerminalView`：顶行（高 28）+ 终端 + 状态栏（高 28）；
//!   放大、移到新标签、关闭也在终端右键菜单的最后一组；
//! - `title_row`：卡片顶行，左边名称、正中在跑的程序、右边分屏和关闭按钮（2026-11-14 用户定）；
//! - `status_bar`：左边连接状态、正中路径、右边系统和尺寸；
//! - `fit`：顶行、状态栏正中那一段窄时让位；
//! - `program_tile`：标签前面那一块（按程序名表画，工单 06）；
//! - `status_dot`：名称、目录后面的状态点（有输出的绿点、要注意的橙点，工单 06）；
//! - `directory`：显示用的目录（主目录写成 `~`）；
//! - `remote`：远程卡片的那一部分（主机快照、连接状态、延迟、系统，工单 12；被动解析出的程序、
//!   目录、标题，工单 17）；
//! - `holding`：等保险库时远程卡片里的占位传输（工单 33）；
//! - `picker`：刚分出来、还没选开什么的新卡片（2026-11-14 分屏重做）。
//!
//! 卡片只认终端 crate 的公开类型，不依赖窗口壳；按钮一律经窗口壳的焦点派发 action
//! （`CloseCard(CardId)`），由窗口壳处理。

mod directory;
mod fit;
mod holding;
mod picker;
mod program_tile;
mod remote;
mod status_bar;
mod status_dot;
mod title_row;
mod view;

pub use directory::{display_directory, home_dir, reported_path};
pub use picker::{CardPicker, PickerChoice, PickerEvent, PickerSource, next_picker_id};
pub use program_tile::render_tab_tile;
pub use remote::{RemoteActivity, RemoteLaunch, RemoteState, RemoteUpdate, VaultHold, host_badge};
pub use status_dot::{ACTIVE_FOR, StatusDot, render_status_dot};
pub use view::{CardEvent, TerminalCard};

/// 一张卡片的 id：从 1 起递增，不复用。元素 id 写成 `("card", id)`、`("terminal", id)` 等。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CardId(pub u64);

impl CardId {
    /// 这张卡片上名为 `name` 的元素的 id，如 `("card-close", 3)`。
    pub fn element(self, name: &'static str) -> gpui_kit::ElementId {
        (name, self.0 as usize).into()
    }
}
