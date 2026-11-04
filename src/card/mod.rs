//! 卡片：标签里的一格终端。M1 只有本地卡片（本地 shell），远程卡片在 M2（工单 12）加。
//!
//! - `view`：`TerminalCard` 实体，持有它的 `TerminalView`，头部（高 33）+ 终端 + 状态栏（高 24）；
//! - `header`：程序图标、名称、目录、右侧按钮区（02 只有关闭，05 加分屏、放大）；
//! - `status_bar`：左边连接状态和 shell 版本，右边编码、终端类型、尺寸；
//! - `program_tile`：头部最左边的程序图块和标签上叠放的图块（按程序名表画，工单 06）；
//! - `status_dot`：名称、目录后面的状态点（有输出的绿点、要注意的橙点，工单 06）；
//! - `directory`：显示用的目录（主目录写成 `~`）。
//!
//! 卡片只认终端 crate 的公开类型，不依赖窗口壳；按钮一律经窗口壳的焦点派发 action
//! （`CloseCard(CardId)`），由窗口壳处理。

mod directory;
mod header;
mod program_tile;
mod status_bar;
mod status_dot;
mod view;

pub use directory::{display_directory, home_dir, reported_path};
pub use program_tile::{MAX_STACKED_TILES, render_program_tile, render_tile_stack};
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
