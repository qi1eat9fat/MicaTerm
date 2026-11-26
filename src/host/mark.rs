// 参考 since2006/shell-rs @ b2a307a: src/shared/host_mark.rs 重写
// 裁剪：去掉 Telnet、串口、连接状态变灰和自带的提示框；尺寸按全部界面原型的 `.host .badge`。

//! 主机栏里主机的系统徽章：探测出系统时是系统的标志，用品牌色；没探测出时是名称的首字，
//! 颜色按地址从固定调色板取（同一个地址在哪台电脑、哪个版本上都是同一个颜色）。
//! 照空壳的主机行：实色底，标志和首字用黑或白。标签上的图块是 `card::remote::host_badge`，
//! 和程序图块（vim、htop）站在一起，也是实色底。

use gpui_kit::component::Icon;
use gpui_kit::*;

use micaterm_hosts::{HostOs, mark_color};

use crate::theme::{WEIGHT_SEMIBOLD, dark_ink_on};

/// 画一个徽章要的东西，从主机上取。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostMark {
    pub name: SharedString,
    pub address: SharedString,
    pub os: Option<HostOs>,
}

impl HostMark {
    pub fn of(host: &micaterm_hosts::Host) -> Self {
        Self {
            name: host.name.clone(),
            address: host.address.clone(),
            os: host.os,
        }
    }

    /// 徽章上的字：没探测出系统时是名称的首字（大写），有系统时是 `None`（画标志）。
    pub fn glyph(&self) -> Option<String> {
        if self.os.is_some() {
            return None;
        }
        Some(
            self.name
                .chars()
                .next()
                .map(|character| character.to_uppercase().to_string())
                .unwrap_or_default(),
        )
    }

    /// 无障碍名称，测试靠它断言：系统名，或者首字。
    pub fn label(&self) -> SharedString {
        match self.os {
            Some(os) => os.label().into(),
            None => self.glyph().unwrap_or_default().into(),
        }
    }

    /// 边长为 `size` 的圆角方块徽章：底是这台主机的品牌色，标志或首字用黑或白。
    /// 系统标志的边长由调用方按徽章所在的位置给出（`size::HOST_BADGE_GLYPH` 或
    /// `size::TAB_BADGE_GLYPH`），两者都是统一配置里的常量。
    pub fn render(
        &self,
        id: ElementId,
        size: Pixels,
        glyph_size: Pixels,
    ) -> impl IntoElement + use<> {
        let (background, ink) = badge_colors(self.os, &self.address);
        let background: Hsla = rgb(background).into();
        let ink: Hsla = rgb(ink).into();
        let glyph = match self.os {
            Some(os) => Icon::default()
                .path(os.icon_path())
                .size(glyph_size)
                .text_color(ink)
                .into_any_element(),
            None => div()
                .text_size(size * 0.46)
                .line_height(size * 0.46)
                .font_weight(WEIGHT_SEMIBOLD)
                .child(self.glyph().unwrap_or_default())
                .into_any_element(),
        };
        div()
            .id(id)
            .test_support()
            .aria_label(self.label())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(size)
            .rounded(size * (6. / 28.))
            .bg(background)
            .text_color(ink)
            .child(glyph)
    }
}

/// 徽章的底色和字色：有系统品牌色用它，否则按地址取地址色；品牌没给字色时按底色亮度选黑/白。
pub(crate) fn badge_colors(os: Option<HostOs>, address: &str) -> (u32, u32) {
    let own = mark_color(address);
    match os {
        Some(os) => match (os.brand_color(), os.brand_foreground()) {
            (Some(background), Some(ink)) => (background, ink),
            _ => (own, ink_on(own)),
        },
        None => (own, ink_on(own)),
    }
}

fn ink_on(background: u32) -> u32 {
    if dark_ink_on(rgb(background).into()) {
        0x000000
    } else {
        0xffffff
    }
}

#[cfg(test)]
mod tests {
    use super::{HostMark, badge_colors};
    use micaterm_hosts::HostOs;

    fn mark(name: &str, os: Option<HostOs>) -> HostMark {
        HostMark {
            name: name.into(),
            address: "10.0.1.12".into(),
            os,
        }
    }

    #[test]
    fn without_a_system_the_mark_is_the_names_first_character() {
        assert_eq!(mark("web-01", None).glyph().as_deref(), Some("W"));
        assert_eq!(mark("数据库", None).glyph().as_deref(), Some("数"));
        assert_eq!(mark("web-01", None).label().as_ref(), "W");
    }

    #[test]
    fn with_a_system_the_mark_is_its_logo() {
        let ubuntu = mark("web-01", Some(HostOs::Ubuntu));
        assert_eq!(ubuntu.glyph(), None);
        assert_eq!(ubuntu.label().as_ref(), "Ubuntu");
    }

    #[test]
    fn badge_colors_use_the_system_then_the_address() {
        let own = micaterm_hosts::mark_color("10.0.1.12");
        assert_eq!(badge_colors(None, "10.0.1.12"), (own, 0xffffff));
        let ubuntu = HostOs::Ubuntu;
        assert_eq!(
            badge_colors(Some(ubuntu), "10.0.1.12"),
            (
                ubuntu.brand_color().unwrap(),
                ubuntu.brand_foreground().unwrap()
            )
        );
    }
}
