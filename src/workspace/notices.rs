//! 终端要告诉用户的事（工单 03）：程序要的通知（OSC 9 / 777）和响铃。
//! 窗口不在前时走系统通知；窗口在前时在窗口里提示，只是看得见的卡片响铃不提示。
//! 工单 06 加了卡片的橙点：程序在用户看不到的地方结束或响铃时亮，看到了就熄。
// 参考 since2006/shell-rs @ b2a307a: src/workspace/notices.rs（裁剪：去掉关键字高亮和通知开关）

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;
use micaterm_terminal::{TerminalEvent, TerminalNotice};

use super::Workspace;
use crate::card::{CardId, TerminalCard};
use crate::i18n::t;

/// 一张卡片多久提示一次：程序最多每 2 秒一次，响铃每 10 秒一次；中间来的丢掉。
const PROGRAM_INTERVAL: Duration = Duration::from_secs(2);
const BELL_INTERVAL: Duration = Duration::from_secs(10);

/// 通知送到哪里。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Delivery {
    /// 系统的通知中心：窗口不在前。
    System,
    /// 窗口里的提示。
    InApp,
    None,
}

/// `notice` 送到哪里。程序要通知就通知，自己的卡片看得见也一样；响铃常常是打字引起的
/// （Tab 补全不出来），所以只在卡片看不见时提示。
pub(super) fn delivery(notice: &TerminalNotice, window_active: bool, in_sight: bool) -> Delivery {
    let quiet_in_sight = matches!(notice, TerminalNotice::Bell { .. });
    if !window_active {
        Delivery::System
    } else if in_sight && quiet_in_sight {
        Delivery::None
    } else {
        Delivery::InApp
    }
}

/// 标题和正文：标题以卡片名称开头，说明是哪张卡片的，程序也冒充不了别的应用。
pub(super) fn notice_text(card: &str, notice: &TerminalNotice) -> (String, String) {
    match notice {
        TerminalNotice::Program {
            title: Some(title),
            body,
        } => (
            t!("workspace.notice.titled", card = card, title = title).to_string(),
            body.clone(),
        ),
        TerminalNotice::Program { title: None, body } => (card.to_string(), body.clone()),
        TerminalNotice::Bell { line } => (
            t!("workspace.notice.bell", card = card).to_string(),
            line.clone(),
        ),
    }
}

/// 通知的类别：各自限频。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum NoticeKind {
    /// 程序要的通知（OSC 9 / 777）。
    Program,
    /// 响铃。
    Bell,
}

impl NoticeKind {
    fn of(notice: &TerminalNotice) -> Self {
        match notice {
            TerminalNotice::Program { .. } => Self::Program,
            TerminalNotice::Bell { .. } => Self::Bell,
        }
    }

    /// 同一张卡片这一类通知最多多久一次。
    fn interval(self) -> Duration {
        match self {
            Self::Program => PROGRAM_INTERVAL,
            Self::Bell => BELL_INTERVAL,
        }
    }
}

/// 每张卡片每类通知上一次发出的时间，用来限频。卡片关了就 `forget` 它。
#[derive(Default)]
pub(super) struct NoticeTimes(HashMap<(CardId, NoticeKind), Instant>);

impl NoticeTimes {
    /// 卡片 `card` 现在能不能发一条 `kind` 类的通知：离上一条够久就能，并记下这一条。
    pub(super) fn try_send(&mut self, card: CardId, kind: NoticeKind, now: Instant) -> bool {
        let due = self
            .0
            .get(&(card, kind))
            .is_none_or(|last| now.saturating_duration_since(*last) >= kind.interval());
        if due {
            self.0.insert((card, kind), now);
        }
        due
    }

    /// 卡片关了：丢掉它的记录。
    pub(super) fn forget(&mut self, card: CardId) {
        self.0.retain(|(id, _), _| *id != card);
    }

    /// 有记录的卡片，从小到大。
    pub(super) fn cards(&self) -> Vec<CardId> {
        let mut cards: Vec<_> = self.0.keys().map(|(card, _)| *card).collect();
        cards.sort();
        cards.dedup();
        cards
    }
}

/// 系统通知的 tag：一张卡片的新通知替换它的旧通知；点通知时靠它找回卡片。
/// 系统通知只认字符串，这是卡片 id 唯一要写成字符串的地方。
pub(super) fn notice_tag(card: CardId) -> String {
    format!("card:{}", card.0)
}

pub(super) fn card_of_tag(tag: &str) -> Option<CardId> {
    tag.strip_prefix("card:")?.parse().ok().map(CardId)
}

/// 窗口里的终端提示，一张卡片同时只留一条。
struct TerminalNoticeTag;

impl Workspace {
    /// 听卡片里的终端要告诉用户的事。卡片关掉、终端释放后，订阅自己失效。
    pub(super) fn watch_notices(
        &mut self,
        card: &Entity<TerminalCard>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = card.read(cx).id();
        let terminal = card.read(cx).terminal().clone();
        cx.subscribe_in(
            &terminal,
            window,
            move |this, _, event: &TerminalEvent, window, cx| {
                if let TerminalEvent::Notice(notice) = event {
                    this.on_terminal_notice(id, notice, window, cx);
                }
            },
        )
        .detach();
    }

    /// 点系统通知时把窗口和那张卡片带到前面。窗口壳建好时调用一次。
    pub(super) fn listen_for_notice_clicks(window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        let handle = window.window_handle();
        cx.on_system_notification_response(move |response, cx| {
            _ = handle.update(cx, |_, window, cx| {
                _ = this.update(cx, |this, cx| {
                    window.activate_window();
                    if let Some(card) = card_of_tag(&response.tag) {
                        this.show_card(card, window, cx);
                    }
                });
            });
        });
    }

    /// 卡片看不看得见：在前那个标签里画出来的卡片看得见；放大着时被藏起来的那几张看不见。
    pub(super) fn card_in_sight(&self, card: CardId, _cx: &App) -> bool {
        self.active_terminal_tab()
            .is_some_and(|tab| tab.shows(card))
    }

    /// 用户看不看得到这张卡片：窗口在前，卡片也看得见。
    fn card_seen(&self, card: CardId, cx: &App) -> bool {
        self.window_active && self.card_in_sight(card, cx)
    }

    /// 卡片的程序结束了，或者响铃了（工单 06）：用户看不到它就亮橙点，等看到了再熄。
    pub(super) fn on_card_wants_attention(
        &mut self,
        card: &Entity<TerminalCard>,
        cx: &mut Context<Self>,
    ) {
        let id = card.read(cx).id();
        if !self.card_seen(id, cx) {
            card.update(cx, |card, cx| card.set_attention(true, cx));
        }
    }

    /// 用户看得到的卡片都熄掉橙点：窗口壳每次重画前（切标签、放大还原、关卡片……）和窗口回到前面时调用。
    pub(super) fn see_cards(&mut self, cx: &mut Context<Self>) {
        if !self.window_active {
            return;
        }
        let Some(tab) = self.active_terminal_tab() else {
            return;
        };
        let seen: Vec<_> = tab
            .cards()
            .filter(|card| {
                let card = card.read(cx);
                card.wants_attention() && tab.shows(card.id())
            })
            .cloned()
            .collect();
        for card in seen {
            card.update(cx, |card, cx| card.set_attention(false, cx));
        }
    }

    /// 把卡片所在的标签放到前面，焦点给这张卡片；它被放大的别的卡片藏着时先还原放大，
    /// 焦点卡片、画出来的卡片和橙点才说的是同一件事。
    fn show_card(&mut self, card: CardId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .find(|tab| tab.as_terminal().is_some_and(|tab| tab.contains(card, cx)))
        else {
            return;
        };
        self.tabs.activate(tab);
        if let Some(tab) = self.terminal_tab_mut(tab) {
            tab.reveal_card(card, cx);
        }
        if let Some(entity) = self.card(card, cx) {
            window.focus(&entity.read(cx).focus_handle(), cx);
        }
        cx.notify();
    }

    fn on_terminal_notice(
        &mut self,
        card: CardId,
        notice: &TerminalNotice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delivery = delivery(
            notice,
            window.is_window_active(),
            self.card_in_sight(card, cx),
        );
        if delivery == Delivery::None {
            return;
        }
        let Some(name) = self.card(card, cx).map(|entity| entity.read(cx).name()) else {
            return;
        };
        // 执行器的时钟，测试能拨它。
        let now = cx.background_executor().now();
        if !self
            .notice_times
            .try_send(card, NoticeKind::of(notice), now)
        {
            return;
        }
        let (title, body) = notice_text(&name, notice);
        match delivery {
            Delivery::System => cx.show_system_notification(SystemNotification {
                tag: notice_tag(card).into(),
                title: title.into(),
                body: body.into(),
                actions: Vec::new(),
            }),
            Delivery::InApp => {
                let workspace = cx.weak_entity();
                let notification = Notification::new()
                    .id1::<TerminalNoticeTag>(card.element("notice"))
                    .title(title)
                    .message(body)
                    .on_click(move |_, window, cx| {
                        _ = workspace
                            .update(cx, |workspace, cx| workspace.show_card(card, window, cx));
                    });
                window.push_notification(notification, cx);
            }
            Delivery::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use micaterm_terminal::TerminalNotice;

    use super::{
        CardId, Delivery, NoticeKind, NoticeTimes, card_of_tag, delivery, notice_tag, notice_text,
    };

    fn program(title: Option<&str>) -> TerminalNotice {
        TerminalNotice::Program {
            title: title.map(str::to_string),
            body: "完成".into(),
        }
    }

    fn bell() -> TerminalNotice {
        TerminalNotice::Bell {
            line: "y/N?".into(),
        }
    }

    #[test]
    fn the_system_tells_of_a_window_away_and_the_window_of_a_card_out_of_sight() {
        for notice in [program(None), bell()] {
            assert_eq!(delivery(&notice, false, true), Delivery::System);
            assert_eq!(delivery(&notice, true, false), Delivery::InApp);
        }
        // 看得见的卡片：程序照样通知，响铃不提示。
        assert_eq!(delivery(&program(None), true, true), Delivery::InApp);
        assert_eq!(delivery(&bell(), true, true), Delivery::None);
    }

    #[test]
    fn a_notice_is_titled_with_its_card() {
        assert_eq!(
            notice_text("pwsh", &program(Some("构建"))),
            ("pwsh：构建".to_string(), "完成".to_string())
        );
        assert_eq!(
            notice_text("pwsh", &program(None)),
            ("pwsh".to_string(), "完成".to_string())
        );
        assert_eq!(
            notice_text("pwsh", &bell()),
            ("pwsh：响铃".to_string(), "y/N?".to_string())
        );
    }

    /// 同一张卡片同一类通知限频：响铃每 10 秒一次，程序每 2 秒一次，两类各算各的，卡片之间也不相干。
    #[test]
    fn notices_are_spaced_out_per_card_and_kind() {
        let start = Instant::now();
        let mut times = NoticeTimes::default();
        assert!(times.try_send(CardId(1), NoticeKind::Bell, start));
        assert!(!times.try_send(CardId(1), NoticeKind::Bell, start + Duration::from_secs(9)));
        assert!(times.try_send(
            CardId(1),
            NoticeKind::Program,
            start + Duration::from_secs(9)
        ));
        assert!(times.try_send(CardId(2), NoticeKind::Bell, start + Duration::from_secs(9)));
        assert!(times.try_send(CardId(1), NoticeKind::Bell, start + Duration::from_secs(10)));
        assert!(!times.try_send(
            CardId(1),
            NoticeKind::Program,
            start + Duration::from_secs(10)
        ));
        assert!(times.try_send(
            CardId(1),
            NoticeKind::Program,
            start + Duration::from_secs(11)
        ));
    }

    /// 卡片关了，它的限频记录跟着丢掉，别的卡片的留着。
    #[test]
    fn a_closed_card_is_forgotten() {
        let start = Instant::now();
        let mut times = NoticeTimes::default();
        times.try_send(CardId(1), NoticeKind::Bell, start);
        times.try_send(CardId(1), NoticeKind::Program, start);
        times.try_send(CardId(2), NoticeKind::Bell, start);

        times.forget(CardId(1));

        assert_eq!(times.cards(), [CardId(2)]);
        assert!(!times.try_send(CardId(2), NoticeKind::Bell, start));
    }

    #[test]
    fn a_tag_names_its_card() {
        assert_eq!(card_of_tag(&notice_tag(CardId(12))), Some(CardId(12)));
        assert_eq!(card_of_tag("card:x"), None);
        assert_eq!(card_of_tag("tab:1"), None);
    }
}
