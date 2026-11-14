// 参考 since2006/shell-rs @ b2a307a: src/shared/host_mark.rs 重写
// 裁剪：去掉 Telnet、串口、连接状态变灰和自带的提示框；尺寸按全部界面原型的 `.host .badge`。

//! 主机栏里主机的系统徽章（原型 `.badge`）：探测出系统时是系统的标志，用品牌色；没探测出时是名称的
//! 首字，颜色按地址从固定调色板取（同一个地址在哪台电脑、哪个版本上都是同一个颜色）。
//! 是淡底：颜色铺薄薄一层做底，标志和首字用颜色本身（用户定，2026-11-14）。
//! 标签上的图块是 `card::remote::host_badge`，和程序图块（vim、htop）站在一起，还是实色底。

use gpui_kit::component::Icon;
use gpui_kit::*;

use micaterm_hosts::{HostOs, mark_color};

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

    /// 边长为 `size` 的圆角方块徽章，淡底（用户定，2026-11-14）：底是这台主机的颜色铺薄薄一层，
    /// 标志或首字用这个颜色本身。`dark` 是深色界面：底铺得厚一点，字提亮，免得深色品牌（MySQL 的藏青）
    /// 在深底上看不见；浅色界面里浅色品牌（Linux 的黄）压暗，免得在浅底上看不见。
    pub fn render(&self, id: ElementId, size: Pixels, dark: bool) -> impl IntoElement + use<> {
        // 没有系统，或者是单色的标志（Apple）：用地址的颜色，徽章绝不会是灰的。
        let color: Hsla = rgb(self
            .os
            .and_then(|os| os.brand_color())
            .unwrap_or_else(|| mark_color(&self.address)))
        .into();
        let (background, ink) = soft(color, dark);
        let glyph = match self.os {
            Some(os) => Icon::default()
                .path(os.icon_path())
                .size(size * 0.58)
                .text_color(ink)
                .into_any_element(),
            None => div()
                .text_size(size * 0.64)
                .line_height(size * 0.64)
                .font_weight(FontWeight::BOLD)
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
            .rounded(size * (4. / 14.))
            .bg(background)
            .text_color(ink)
            .child(glyph)
    }
}

/// 淡底徽章的底色和字色：底是 `color` 铺一层（浅色界面 14%，深色 22%），字是 `color` 本身，
/// 亮度收进看得清的范围（浅色界面不高于 45%，深色不低于 74%）。
fn soft(color: Hsla, dark: bool) -> (Hsla, Hsla) {
    if dark {
        (
            color.opacity(0.22),
            Hsla {
                l: color.l.max(0.74),
                ..color
            },
        )
    } else {
        (
            color.opacity(0.14),
            Hsla {
                l: color.l.min(0.45),
                ..color
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::HostMark;
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
}
