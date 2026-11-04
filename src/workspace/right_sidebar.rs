//! 右侧栏（宽 46）：一列图标，依次是文件、命令片段、凭据。
//!
//! 「文件」打开或切换到 SFTP 标签（M3），不展开面板，所以没有按下的样式；
//! 另外两个点一下在右边展开约 290 宽的面板，再点一下收起，展开时图标显示成按下的样子。

use gpui_kit::*;

use super::icon_button::IconButton;
use crate::app::{CatalogIcon, OpenSftp, ShowCredentials, ShowSnippets};
use crate::i18n::t;
use crate::theme::{ICON_BUTTON_RADIUS, RAIL_BUTTON, RIGHT_SIDEBAR_WIDTH, Tokens};

/// 右侧栏展开的面板，同一时间最多一个。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightPanel {
    /// 命令片段（工单 19）。
    Snippets,
    /// 凭据（工单 14）。
    Credentials,
}

pub(super) fn render_right_sidebar(
    open: Option<RightPanel>,
    target: &FocusHandle,
    tokens: &Tokens,
) -> impl IntoElement {
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
            .pressed(open == Some(RightPanel::Snippets))
            .dispatch(Box::new(ShowSnippets), target, tokens),
        )
        .child(
            button(
                "right-sidebar-credentials",
                CatalogIcon::KeyRound,
                t!("workspace.right_sidebar.credentials"),
            )
            .pressed(open == Some(RightPanel::Credentials))
            .dispatch(Box::new(ShowCredentials), target, tokens),
        )
}
