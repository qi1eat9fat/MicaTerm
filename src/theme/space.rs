//! 间距刻度（`docs/ui-standard.md` §3.1）：布局里的 gap / padding / margin 只用这几档。
//!
//! 功能模块引用这里的常量，不再写 `px(2.)` / `px(4.)` 这类字面量。

use gpui_kit::{Pixels, px};

/// 2px。
pub const SPACE_2: Pixels = px(2.);
/// 4px。
pub const SPACE_4: Pixels = px(4.);
/// 5px（紧凑行、图标与字之间）。
pub const SPACE_5: Pixels = px(5.);
/// 6px。
pub const SPACE_6: Pixels = px(6.);
/// 7px（选择器搜索这类紧凑行）。
pub const SPACE_7: Pixels = px(7.);
/// 8px。
pub const SPACE_8: Pixels = px(8.);
/// 10px（行内左右内边距）。
pub const SPACE_10: Pixels = px(10.);
/// 12px。
pub const SPACE_12: Pixels = px(12.);
/// 16px。
pub const SPACE_16: Pixels = px(16.);
/// 20px。
pub const SPACE_20: Pixels = px(20.);
/// 24px。
pub const SPACE_24: Pixels = px(24.);

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::{
        SPACE_2, SPACE_4, SPACE_5, SPACE_6, SPACE_7, SPACE_8, SPACE_10, SPACE_12, SPACE_16,
        SPACE_20, SPACE_24,
    };

    /// 刻度值就是规范里的 2 / 4 / 6 / 8 / 12 / 16 / 20 / 24。
    #[test]
    fn scale_matches_the_standard() {
        let scale = [
            (SPACE_2, 2.),
            (SPACE_4, 4.),
            (SPACE_5, 5.),
            (SPACE_6, 6.),
            (SPACE_7, 7.),
            (SPACE_8, 8.),
            (SPACE_10, 10.),
            (SPACE_12, 12.),
            (SPACE_16, 16.),
            (SPACE_20, 20.),
            (SPACE_24, 24.),
        ];
        for (actual, expected) in scale {
            assert_eq!(actual, px(expected));
        }
    }
}
