//! 窗口壳里的快速连接弹层（工单 18）：「⌄」或 Ctrl+Shift+O 打开、再按一次关上；挂在「+ ⌄」下面。
//!
//! 弹层本身（列表、搜索、按键）在 `host::QuickConnectList`；这里管开关、位置、焦点，
//! 回车连接时关掉弹层再 `connect_host`（新标签，已连着的主机复用连接）。

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::*;

use crate::host::{QuickConnectEvent, QuickConnectList};
use crate::theme::TITLE_BAR_HEIGHT;
use crate::workspace::Workspace;

/// 「+ ⌄」这组按钮和「⌄」本身上一帧画在哪里（窗口坐标）：弹层挂在组的下面；
/// 在「⌄」上按下鼠标不算点在外面，交给按钮自己开关。
#[derive(Default)]
pub(in crate::workspace) struct QuickConnectAnchor {
    pub group: Cell<Option<Bounds<Pixels>>>,
    pub chevron: Cell<Option<Bounds<Pixels>>>,
}

/// 开着的快速连接弹层。
pub(in crate::workspace) struct QuickConnectPopup {
    view: Entity<QuickConnectList>,
    /// 打开前焦点在哪里：关掉时还回去。
    return_focus: Option<FocusHandle>,
    _events: Subscription,
}

impl Workspace {
    /// 开着就关上，关着就打开（「⌄」、Ctrl+Shift+O）。有对话框开着时不打开。
    pub(in crate::workspace) fn toggle_quick_connect(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quick_connect.is_some() {
            self.close_quick_connect(window, cx);
            return;
        }
        if gpui_kit::component::WindowExt::has_active_dialog(window, cx) {
            return;
        }
        let return_focus = window.focused(cx);
        let hosts = self.hosts.clone();
        let view = cx.new(|cx| QuickConnectList::new(hosts, window, cx));
        let events = cx.subscribe_in(&view, window, Self::on_quick_connect_event);
        view.update(cx, |view, cx| view.focus(window, cx));
        self.quick_connect = Some(QuickConnectPopup {
            view,
            return_focus,
            _events: events,
        });
        cx.notify();
    }

    /// 快速连接开着时它的列表，测试用。
    pub fn quick_connect(&self) -> Option<&Entity<QuickConnectList>> {
        self.quick_connect.as_ref().map(|popup| &popup.view)
    }

    fn on_quick_connect_event(
        &mut self,
        _: &Entity<QuickConnectList>,
        event: &QuickConnectEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match *event {
            QuickConnectEvent::Connect(host) => {
                self.close_quick_connect(window, cx);
                self.connect_host(host, window, cx);
            }
            QuickConnectEvent::Close => self.close_quick_connect(window, cx),
            QuickConnectEvent::ClickedOutside(position) => {
                let on_chevron = self
                    .quick_connect_anchor
                    .chevron
                    .get()
                    .is_some_and(|bounds| bounds.contains(&position));
                if !on_chevron {
                    self.close_quick_connect(window, cx);
                }
            }
        }
    }

    /// 关掉弹层。焦点还在搜索框里时还给打开前的地方（点了别处的，由点到的那里拿焦点）。
    fn close_quick_connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.quick_connect.take() else {
            return;
        };
        if popup.view.focus_handle(cx).contains_focused(window, cx) {
            let fallback = if self.tabs.active().is_none() {
                self.start_card_focus.clone()
            } else {
                self.focus_handle.clone()
            };
            window.focus(&popup.return_focus.unwrap_or(fallback), cx);
        }
        cx.notify();
    }

    /// 开着的弹层：挂在「+ ⌄」下面，贴不下时往窗口里挪。
    pub(in crate::workspace) fn render_quick_connect(&self) -> Option<impl IntoElement + use<>> {
        let popup = self.quick_connect.as_ref()?;
        let position = match self.quick_connect_anchor.group.get() {
            Some(bounds) => point(bounds.left(), bounds.bottom() + px(4.)),
            None => point(px(240.), TITLE_BAR_HEIGHT),
        };
        Some(
            deferred(
                anchored()
                    .position(position)
                    .snap_to_window_with_margin(px(8.))
                    .child(popup.view.clone()),
            )
            .with_priority(POPUP_PRIORITY),
        )
    }

    /// 标题栏记下「+ ⌄」位置用的。
    pub(in crate::workspace) fn quick_connect_anchor(&self) -> Rc<QuickConnectAnchor> {
        self.quick_connect_anchor.clone()
    }
}
