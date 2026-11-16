//! 启动 Agent 前要做好的事（Agent CLI 工单 07，26 号「启动 Agent」屏）：外部 CLI 没启用、
//! Agent 敲不了 `micaterm`、这个 Agent 的 Skill 没装或旧了。缺什么列什么，一行一件；
//! 用户确认后按顺序做完再启动，有一件做失败就停在框里、那一行写原因。都满足时不出框。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::CatalogIcon;
use crate::cli::{
    AgentKind, BinaryStatus, CliIntegration, IntegrationChange, IntegrationPaths,
    IntegrationStatus, SkillStatus, cli_callable,
};
use crate::i18n::t;
use crate::settings::SettingsStore;
use crate::theme::{MONO_FONT_FAMILY, Tokens, TypeScale};
use crate::ui::DialogLayer;
use crate::ui::modal::{
    ModalButtonTone, ModalHead, OnClick, modal_button, modal_dialog, modal_note, modal_question,
    render_modal,
};

/// 启动前要做的一件事。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepStep {
    /// 打开设置 › Agent 的总开关。
    EnableCli,
    /// 把 `micaterm` 装到 `PATH` 上；`update` 是换掉别的版本留下的。
    InstallCommand { update: bool },
    /// 装这个 Agent 的 Skill；`update` 是换掉旧的。
    InstallSkill { kind: AgentKind, update: bool },
}

/// 缺的那几件，按要做的顺序。`status` 是 `None`（这次运行装不了命令和 Skill）时只看总开关。
pub fn prep_steps(
    cli_enabled: bool,
    paths: Option<&IntegrationPaths>,
    status: Option<&IntegrationStatus>,
    kind: Option<AgentKind>,
) -> Vec<PrepStep> {
    let mut steps = Vec::new();
    if !cli_enabled {
        steps.push(PrepStep::EnableCli);
    }
    let (Some(paths), Some(status)) = (paths, status) else {
        return steps;
    };
    match status.binary {
        BinaryStatus::Outdated => steps.push(PrepStep::InstallCommand { update: true }),
        // 没装时 macOS、Linux 上 Agent 照样敲得了（程序自己就叫 micaterm，目录放在它的 PATH 最前面）；
        // Windows 上旁边那个叫 micaterm-cli.exe，要装一份。
        BinaryStatus::Missing | BinaryStatus::NotOnPath | BinaryStatus::Stale { .. }
            if !cli_callable(paths) =>
        {
            steps.push(PrepStep::InstallCommand { update: false })
        }
        _ => {}
    }
    if let Some(kind) = kind {
        match status.skill(kind) {
            SkillStatus::Missing => steps.push(PrepStep::InstallSkill {
                kind,
                update: false,
            }),
            SkillStatus::Outdated => steps.push(PrepStep::InstallSkill { kind, update: true }),
            SkillStatus::Installed => {}
        }
    }
    steps
}

/// 确认框写什么。
pub struct Prep {
    /// Agent 的名字（Claude Code）。
    pub name: SharedString,
    /// 交给它哪张卡片（从远程卡片打开时）。
    pub card: Option<SharedString>,
    pub steps: Vec<PrepStep>,
}

/// 都做好了：启动。
pub type OnReady = Box<dyn FnOnce(&mut Window, &mut App)>;

/// 确认框的内容。元素 id：框是 `agent-prep`，第 n 行是 `("agent-prep-step", n)`，
/// 按钮是 `agent-prep-cancel`、`agent-prep-confirm`。
pub struct PrepDialog {
    prep: Prep,
    /// 做失败的那一行和原因。
    failed: Option<(usize, SharedString)>,
    store: Entity<SettingsStore>,
    integration: Entity<CliIntegration>,
    layer: DialogLayer,
    on_ready: Rc<RefCell<Option<OnReady>>>,
    working: Option<Task<()>>,
    focus_handle: FocusHandle,
}

impl PrepDialog {
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working.is_some() {
            return;
        }
        self.failed = None;
        let steps = self.prep.steps.clone();
        let store = self.store.clone();
        let integration = self.integration.clone();
        self.working = Some(cx.spawn_in(window, async move |this, cx| {
            for (index, step) in steps.into_iter().enumerate() {
                let change = match step {
                    PrepStep::EnableCli => {
                        let _ = cx.update(|_, cx| {
                            store.update(cx, |store, cx| {
                                store.update(|settings| settings.cli.enabled = true, cx)
                            })
                        });
                        continue;
                    }
                    PrepStep::InstallCommand { .. } => IntegrationChange::InstallBinary,
                    PrepStep::InstallSkill { kind, .. } => IntegrationChange::InstallSkill(kind),
                };
                let Ok(task) =
                    cx.update(|_, cx| integration.update(cx, |it, cx| it.apply(change, cx)))
                else {
                    return;
                };
                if let Err(reason) = task.await {
                    let _ = this.update(cx, |this, cx| {
                        this.failed = Some((index, reason.into()));
                        this.working = None;
                        cx.notify();
                    });
                    return;
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.working = None;
                this.layer.close(window, cx);
                if let Some(on_ready) = this.on_ready.borrow_mut().take() {
                    on_ready(window, cx);
                }
            });
        }));
        cx.notify();
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working.is_none() {
            self.on_ready.borrow_mut().take();
            self.layer.close(window, cx);
        }
    }

    /// 只差更新 Skill：说「更新并启动」。
    fn only_update(&self) -> bool {
        matches!(
            self.prep.steps.as_slice(),
            [PrepStep::InstallSkill { update: true, .. }]
        )
    }

    fn step_text(&self, step: PrepStep, cx: &App) -> (SharedString, Option<SharedString>) {
        let paths = self.integration.read(cx).paths();
        match step {
            PrepStep::EnableCli => (
                t!("agent.prep.enable_cli"),
                Some(t!("agent.prep.enable_cli_where")),
            ),
            PrepStep::InstallCommand { update } => (
                if update {
                    t!("agent.prep.update_command")
                } else {
                    t!("agent.prep.install_command")
                },
                paths.map(|paths| paths.bin_link.display().to_string().into()),
            ),
            PrepStep::InstallSkill { kind, update } => (
                if update {
                    t!("agent.prep.update_skill", agent = kind.label())
                } else {
                    t!("agent.prep.install_skill", agent = kind.label())
                },
                paths.and_then(|paths| {
                    paths
                        .skill_file(kind)
                        .parent()
                        .map(|dir| dir.display().to_string().into())
                }),
            ),
        }
    }
}

impl Focusable for PrepDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PrepDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let scale = TypeScale::current(cx);
        let working = self.working.is_some();
        let question = if self.only_update() {
            t!("agent.prep.question_update", agent = self.prep.name.clone())
        } else {
            t!("agent.prep.question", agent = self.prep.name.clone())
        };
        let rows = self.prep.steps.iter().enumerate().map(|(index, step)| {
            let (title, detail) = self.step_text(*step, cx);
            let failed = self
                .failed
                .as_ref()
                .filter(|(at, _)| *at == index)
                .map(|(_, reason)| reason.clone());
            div()
                .id(("agent-prep-step", index))
                .test_support()
                .aria_label(title.clone())
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(26.))
                .child(
                    div()
                        .flex_none()
                        .text_color(tokens.icon)
                        .child(Icon::new(CatalogIcon::Circle).size(px(12.))),
                )
                .child(div().flex_none().text_color(tokens.text).child(title))
                .map(|row| match (failed, detail) {
                    (Some(reason), _) => row.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(tokens.err)
                            .child(reason),
                    ),
                    (None, Some(detail)) => row.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(tokens.st_dim)
                            .when(*step != PrepStep::EnableCli, |detail| {
                                detail
                                    .font_family(MONO_FONT_FAMILY)
                                    .text_size(scale.mono_small)
                            })
                            .child(detail),
                    ),
                    (None, None) => row,
                })
        });
        let body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(modal_question(question, &tokens))
            .child(div().flex().flex_col().children(rows))
            .child(modal_note(t!("agent.prep.note"), &tokens));
        let footer = div()
            .flex()
            .gap(px(6.))
            .child(modal_button(
                "agent-prep-cancel",
                t!("agent.prep.cancel"),
                Some("Esc"),
                ModalButtonTone::Plain,
                (!working).then(|| -> OnClick {
                    Box::new(cx.listener(|this, _, window, cx| this.cancel(window, cx)))
                }),
                &tokens,
                window,
                cx,
            ))
            .child(modal_button(
                "agent-prep-confirm",
                if self.only_update() {
                    t!("agent.prep.confirm_update")
                } else {
                    t!("agent.prep.confirm")
                },
                Some("Enter"),
                ModalButtonTone::Primary,
                (!working).then(|| -> OnClick {
                    Box::new(cx.listener(|this, _, window, cx| this.confirm(window, cx)))
                }),
                &tokens,
                window,
                cx,
            ));
        let head = ModalHead {
            icon: CatalogIcon::Sparkles,
            title: self.prep.name.clone(),
            detail: self
                .prep
                .card
                .clone()
                .map(|card| t!("agent.prep.handing", card = card)),
        };
        render_modal(Some(head), body, footer, &tokens, window, cx)
            .id("agent-prep")
            .test_support()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" {
                    this.confirm(window, cx);
                }
            }))
    }
}

/// 叠一个启动前的确认框。Esc 等于取消。焦点给框。
pub fn open_prep_dialog(
    prep: Prep,
    store: Entity<SettingsStore>,
    integration: Entity<CliIntegration>,
    on_ready: OnReady,
    window: &mut Window,
    cx: &mut App,
) {
    let layer = DialogLayer::new();
    let on_ready = Rc::new(RefCell::new(Some(on_ready)));
    let view = cx.new(|cx| PrepDialog {
        prep,
        failed: None,
        store,
        integration,
        layer: layer.clone(),
        on_ready: on_ready.clone(),
        working: None,
        focus_handle: cx.focus_handle(),
    });
    let focus = view.read(cx).focus_handle.clone();
    layer.open_dismissable(
        move |dialog, _, _| modal_dialog(dialog).child(view.clone()),
        move |_, _| {
            on_ready.borrow_mut().take();
        },
        window,
        cx,
    );
    window.focus(&focus, cx);
}

#[cfg(test)]
mod tests {
    // 不能 `use super::*`：带进来的 `gpui_kit::*` 会遮住 `#[test]`。
    use super::{PrepStep, prep_steps};
    use crate::cli::AgentKind;

    #[test]
    fn only_what_is_missing_is_listed_in_the_order_it_is_done() {
        // 没有装命令和 Skill 的路径：只看总开关。
        assert_eq!(
            prep_steps(false, None, None, Some(AgentKind::ClaudeCode)),
            [PrepStep::EnableCli]
        );
        assert!(prep_steps(true, None, None, Some(AgentKind::ClaudeCode)).is_empty());
    }
}
