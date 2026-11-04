// 参考 since2006/shell-rs @ b2a307a: src/shared/host_mark.rs 重写
// 裁剪：去掉 Telnet、串口、连接状态变灰和自带的提示框；尺寸按全部界面原型的 `.host .badge`。

//! 主机的系统徽章（原型 `.badge`）：探测出系统时是系统的标志加品牌色底；没探测出时是名称的
//! 首字，底色按地址从固定调色板取（同一个地址在哪台电脑、哪个版本上都是同一个颜色）。
//! 主机栏、快速连接、开始卡片共用它，同一台主机到处是同一个样子。

use gpui_kit::component::Icon;
use gpui_kit::*;

use qishell_hosts::{HostOs, mark_color};

/// 没有品牌色时字和标志用白色（调色板的颜色都够深，白字对比度至少 4.5:1）。
const MARK_INK: u32 = 0xFF_FFFF;

/// 画一个徽章要的东西，从主机上取。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostMark {
    pub name: SharedString,
    pub address: SharedString,
    pub os: Option<HostOs>,
}

impl HostMark {
    pub fn of(host: &qishell_hosts::Host) -> Self {
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

    /// 直径为 `size` 的圆形徽章。
    pub fn render(&self, id: ElementId, size: Pixels) -> impl IntoElement + use<> {
        let own: Hsla = rgb(mark_color(&self.address)).into();
        let white: Hsla = rgb(MARK_INK).into();
        let (background, ink) = match self.os.and_then(|os| os.brand_color().zip(os.brand_foreground())) {
            Some((background, ink)) => (rgb(background).into(), rgb(ink).into()),
            // 没有系统，或者是单色的标志（Apple）：用地址的颜色，徽章绝不会是灰的。
            None => (own, white),
        };
        let glyph = match self.os {
            Some(os) => Icon::default()
                .path(os.icon_path())
                .size(size * 0.58)
                .text_color(ink)
                .into_any_element(),
            None => div()
                .text_size(size * 0.5)
                .font_weight(FontWeight::SEMIBOLD)
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
            .rounded_full()
            .bg(background)
            .text_color(ink)
            .child(glyph)
    }
}

#[cfg(test)]
mod tests {
    use super::HostMark;
    use qishell_hosts::HostOs;

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
}
