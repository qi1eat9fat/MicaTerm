//! 右侧栏（宽 46）：一列图标，依次是文件、命令片段、凭据。
//!
//! 「文件」打开或切换到 SFTP 标签（M3）；另外两个点一下在右边展开约 290 宽的面板（M2）。
//! M1 只有图标，点了派发 action，处理器暂时什么都不做。

use gpui_kit::*;

use super::icon_button::IconButton;
use crate::app::{CatalogIcon, OpenSftp, ShowCredentials, ShowSnippets};
use crate::i18n::t;
use crate::theme::{ICON_BUTTON_RADIUS, RAIL_BUTTON, RIGHT_SIDEBAR_WIDTH, Tokens};

pub(super) fn render_right_sidebar(target: &FocusHandle, tokens: &Tokens) -> impl IntoElement {
    let button = |id: &'static str, icon: CatalogIcon, label: SharedString| {
        IconButton::new(id, icon, label).size(RAIL_BUTTON, ICON_BUTTON_RADIUS)
    };
    div()
        .id("right-sidebar")
        .test_support()
        .flex()
        .flex_col()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .w(RIGHT_SIDEBAR_WIDTH)
        .pt(px(2.))
        .child(
            button(
                "right-sidebar-files",
                CatalogIcon::Folder,
                t!("workspace.right_sidebar.files"),
            )
            .dispatch(Box::new(OpenSftp), target, tokens),
        )
        .child(
            button(
                "right-sidebar-snippets",
                CatalogIcon::CodeXml,
                t!("workspace.right_sidebar.snippets"),
            )
            .dispatch(Box::new(ShowSnippets), target, tokens),
        )
        .child(
            button(
                "right-sidebar-credentials",
                CatalogIcon::KeyRound,
                t!("workspace.right_sidebar.credentials"),
            )
            .dispatch(Box::new(ShowCredentials), target, tokens),
        )
}
