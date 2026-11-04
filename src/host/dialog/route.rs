//! 主机对话框的「连接方式」组。本工单（11）只有「直接」。
//!
//! 扩展点：工单 15（跳板与代理）给 `RouteChoice` 加「SSH 跳板」「代理」，在 `render` 里画路线、
//! 跳板列表和代理字段，`fill` 按选择写 `draft.route`，出错时记到 `FIELD_ROUTE` 一类的字段上。

use std::rc::Rc;

use gpui_kit::*;

use qishell_hosts::{HostDraft, Route};

use super::form::{FieldErrors, FieldNote, form_field, form_group};
use crate::i18n::t;
use crate::settings::segmented;
use crate::theme::Tokens;

/// 连接方式的分段按钮。
pub const ROUTE_CHOICE: &str = "host-route";

/// 主机怎么连过去。工单 15 加「SSH 跳板」「代理」。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum RouteChoice {
    #[default]
    Direct,
}

impl RouteChoice {
    const ALL: [RouteChoice; 1] = [RouteChoice::Direct];

    fn label(self) -> SharedString {
        match self {
            RouteChoice::Direct => t!("host.dialog.route_direct"),
        }
    }
}

pub(super) struct RouteSection {
    choice: RouteChoice,
    /// 编辑的主机原来的路线。这一组还没有跳板和代理的界面，保存时原样写回，免得丢掉。
    route: Route,
}

impl RouteSection {
    pub(super) fn new(draft: &HostDraft) -> Self {
        Self {
            choice: RouteChoice::Direct,
            route: draft.route.clone(),
        }
    }

    pub(super) fn fill(&self, draft: &mut HostDraft, _: &mut FieldErrors, _: &App) {
        match self.choice {
            RouteChoice::Direct => draft.route = self.route.clone(),
        }
    }

    pub(super) fn render(&self, errors: &FieldErrors, tokens: &Tokens) -> AnyElement {
        let choice = self.choice;
        form_group(
            t!("host.dialog.route"),
            [form_field(
                None,
                segmented(
                    ROUTE_CHOICE,
                    RouteChoice::ALL.iter().map(|choice| choice.label()).collect(),
                    RouteChoice::ALL
                        .iter()
                        .position(|candidate| *candidate == choice)
                        .unwrap_or(0),
                    Rc::new(|_, _, _| {}),
                    tokens,
                ),
                FieldNote::error_of(ROUTE_CHOICE, errors).into_iter().collect(),
                tokens,
            )],
            tokens,
        )
    }
}
