//! 启动 Agent 的框（Agent CLI 工单 07，26 号「启动 Agent」屏；2026-11-16 用户定每次启动都问）：
//! 开新的 Agent 时先选文件夹（默认上次的）和对话——「新对话」或这个文件夹里的某个历史对话
//! （最近用过的在前，默认选最近的那个）；外部 CLI 没启用、Agent 敲不了 `micaterm`、这个 Agent 的
//! Skill 没装或旧了时，缺的几件也列在同一个框里，一行一件。用户点了以后按顺序做完再启动，
//! 有一件做失败就停在框里、那一行写原因。
//!
//! 交卡片给已经在跑的 Agent 时不选文件夹和对话：只在有缺的时候出框，都满足时不出框。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::history::{Conversation, conversation_time};
use crate::app::CatalogIcon;
use crate::cli::{
    AgentKind, BinaryStatus, CliIntegration, IntegrationChange, IntegrationPaths,
    IntegrationStatus, SkillStatus, cli_callable,
};
use crate::i18n::t;
use crate::settings::SettingsStore;
use crate::theme::{Tokens, TypeScale, size, space};
use crate::ui::DialogLayer;
use crate::ui::modal::{
    ModalButtonTone, ModalHead, OnClick, modal_button, modal_dialog, modal_note, modal_question,
    render_modal,
};

/// 对话列表一行的高度，和最多露出几行（多了滚动）。
const ROW_HEIGHT: Pixels = px(30.);
const VISIBLE_ROWS: f32 = 6.5;

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

/// 列出某个文件夹里的历史对话（阻塞，在后台跑）。
pub type ListConversations = Arc<dyn Fn(&Path) -> Vec<Conversation> + Send + Sync>;

/// 开新的 Agent 时框里的文件夹和对话。
pub struct StartOptions {
    /// 默认的文件夹：设置里记着的；没有（或不在了）是主目录。`None` 只在连主目录都找不到时。
    pub directory: Option<PathBuf>,
    /// 列文件夹里的历史对话；`None` 是这个 Agent 不能接着以前的对话（Codex、认不出的程序）。
    pub history: Option<ListConversations>,
}

/// 框里写什么。
pub struct Prep {
    /// Agent 的名字（Claude Code）。
    pub name: SharedString,
    /// 交给它哪张卡片（从远程卡片打开时）。
    pub card: Option<SharedString>,
    pub steps: Vec<PrepStep>,
    /// 开新的 Agent：选文件夹和对话。交给已在跑的 Agent（只补缺的）时 `None`。
    pub start: Option<StartOptions>,
}

/// 用户选的：在哪个文件夹、接着哪个对话（`None` 是新对话）。交给已在跑的 Agent 时都是 `None`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartChoice {
    pub directory: Option<PathBuf>,
    pub resume: Option<String>,
}

/// 都做好了：启动。
pub type OnReady = Box<dyn FnOnce(StartChoice, &mut Window, &mut App)>;

/// 框的内容。元素 id：框是 `agent-prep`；文件夹是 `agent-prep-folder`（无障碍名是完整路径），
/// 「选择…」是 `agent-prep-folder-choose`；对话列表里「新对话」是 `agent-prep-new`，第 n 个历史
/// 对话是 `("agent-prep-conversation", n)`（无障碍名是标题），选中的那行无障碍名后面加「（已选）」；
/// 第 n 件缺的是 `("agent-prep-step", n)`；按钮是 `agent-prep-cancel`，开新的时 `agent-prep-start`，
/// 只补缺的时 `agent-prep-confirm`。
pub struct PrepDialog {
    prep: Prep,
    /// 框里现在选的文件夹。
    directory: Option<PathBuf>,
    /// 这个文件夹里的历史对话；`None` 是还在读。
    conversations: Option<Vec<Conversation>>,
    /// 选中的历史对话；`None` 是「新对话」。
    selected: Option<usize>,
    /// 后台读历史对话（换文件夹时换掉旧的）。
    _listing: Option<Task<()>>,
    scroll: ScrollHandle,
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
    /// 换了文件夹：在后台重新读历史对话，读好后默认选最近的那个（没有就是新对话）。
    fn set_directory(&mut self, directory: Option<PathBuf>, cx: &mut Context<Self>) {
        self.directory = directory.clone();
        self.selected = None;
        let list = self
            .prep
            .start
            .as_ref()
            .and_then(|start| start.history.clone());
        let (Some(list), Some(directory)) = (list, directory) else {
            self.conversations = Some(Vec::new());
            self._listing = None;
            return;
        };
        self.conversations = None;
        self._listing = Some(cx.spawn(async move |this, cx| {
            let found = cx.background_spawn(async move { list(&directory) }).await;
            this.update(cx, |this, cx| {
                this.selected = (!found.is_empty()).then_some(0);
                this.conversations = Some(found);
                this.scroll.scroll_to_item(0);
                cx.notify();
            })
            .ok();
        }));
    }

    /// 「选择…」：系统的选择框挑一个文件夹，换进框里（启动时才记进设置）。
    fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("agent.start.folder_prompt")),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(directory) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.set_directory(Some(directory), cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn select(&mut self, selected: Option<usize>, cx: &mut Context<Self>) {
        self.selected = selected;
        // 列表里「新对话」占第 0 行。
        self.scroll
            .scroll_to_item(selected.map_or(0, |index| index + 1));
        cx.notify();
    }

    /// 上下键在「新对话」和历史对话之间挪。
    fn step_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.conversations.as_ref().map_or(0, Vec::len);
        let next = match (self.selected, down) {
            (None, true) if count > 0 => Some(0),
            (None, _) => None,
            (Some(index), true) => Some((index + 1).min(count.saturating_sub(1))),
            (Some(0), false) => None,
            (Some(index), false) => Some(index - 1),
        };
        self.select(next, cx);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working.is_some() {
            return;
        }
        // 还在读历史对话时先别启动：读好了默认选的是最近那个，现在按下去会变成新对话。
        if self.prep.start.is_some() && self.conversations.is_none() {
            return;
        }
        self.failed = None;
        let choice = match &self.prep.start {
            Some(_) => StartChoice {
                directory: self.directory.clone(),
                resume: self.selected.and_then(|index| {
                    self.conversations
                        .as_ref()
                        .and_then(|list| list.get(index))
                        .map(|conversation| conversation.id.clone())
                }),
            },
            None => StartChoice {
                directory: None,
                resume: None,
            },
        };
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
                    on_ready(choice, window, cx);
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

    /// 文件夹一行：「文件夹」+ 路径 +「选择…」。
    fn folder_row(&self, tokens: &Tokens, window: &Window, cx: &mut Context<Self>) -> Div {
        let working = self.working.is_some();
        let path: SharedString = match &self.directory {
            Some(directory) => directory.to_string_lossy().into_owned().into(),
            None => t!("agent.start.folder_home"),
        };
        div()
            .flex()
            .items_center()
            .gap(space::SPACE_10)
            .child(
                div()
                    .flex_none()
                    .text_color(tokens.label)
                    .child(t!("agent.start.folder")),
            )
            .child(
                div()
                    .id("agent-prep-folder")
                    .test_support()
                    .aria_label(path.clone())
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(tokens.text)
                    .child(path),
            )
            .child(modal_button(
                "agent-prep-folder-choose",
                t!("agent.start.folder_choose"),
                None,
                ModalButtonTone::Line,
                (!working).then(|| -> OnClick {
                    Box::new(cx.listener(|this, _, _, cx| this.choose_folder(cx)))
                }),
                tokens,
                window,
                cx,
            ))
    }

    /// 对话列表的一行：选中的底色是 `sel`，悬停是 `hover`；右边淡字时间。
    #[allow(clippy::too_many_arguments)]
    fn conversation_row(
        &self,
        id: ElementId,
        selected: bool,
        leading: Option<CatalogIcon>,
        title: SharedString,
        time: Option<String>,
        on_click: OnClick,
        tokens: &Tokens,
        cx: &App,
    ) -> impl IntoElement + use<> {
        let scale = TypeScale::current(cx);
        let label: SharedString = if selected {
            t!("agent.start.selected", title = title.clone())
        } else {
            title.clone()
        };
        let hover = tokens.hover;
        div()
            .id(id)
            .test_support()
            .aria_label(label)
            .flex()
            .flex_none()
            .items_center()
            .gap(space::SPACE_8)
            .h(ROW_HEIGHT)
            .px(space::SPACE_10)
            .rounded(size::RADIUS_MD)
            .cursor_pointer()
            .text_color(tokens.text)
            .map(|row| {
                if selected {
                    row.bg(tokens.sel)
                } else {
                    row.hover(move |style| style.bg(hover))
                }
            })
            .on_click(on_click)
            .children(leading.map(|icon| {
                div()
                    .flex_none()
                    .text_color(tokens.icon)
                    .child(Icon::new(icon).size(px(14.)))
            }))
            .child(div().flex_1().min_w_0().truncate().child(title))
            .children(time.map(|time| {
                div()
                    .flex_none()
                    .text_size(scale.sm)
                    .text_color(tokens.st_dim)
                    .child(time)
            }))
    }

    /// 「对话」：「新对话」+ 历史对话（最近的在前），多了滚动；Agent 不能接着以前的对话时下面一行说。
    fn conversation_list(&self, tokens: &Tokens, cx: &mut Context<Self>) -> Div {
        let now = chrono::Local::now();
        let mut rows = vec![self.conversation_row(
            "agent-prep-new".into(),
            self.selected.is_none(),
            Some(CatalogIcon::Plus),
            t!("agent.start.new"),
            None,
            Box::new(cx.listener(|this, _, _, cx| this.select(None, cx))),
            tokens,
            cx,
        )];
        for (index, conversation) in self.conversations.iter().flatten().enumerate() {
            rows.push(self.conversation_row(
                ("agent-prep-conversation", index).into(),
                self.selected == Some(index),
                None,
                conversation.title.clone().into(),
                Some(conversation_time(conversation.modified, now)),
                Box::new(cx.listener(move |this, _, _, cx| this.select(Some(index), cx))),
                tokens,
                cx,
            ));
        }
        let note = match &self.prep.start {
            Some(StartOptions { history: None, .. }) => Some(t!(
                "agent.start.no_resume_agent",
                agent = self.prep.name.clone()
            )),
            _ if self.conversations.is_none() => Some(t!("agent.start.reading")),
            _ => None,
        };
        div()
            .flex()
            .flex_col()
            .gap(space::SPACE_6)
            .child(
                div()
                    .text_color(tokens.label)
                    .child(t!("agent.start.conversation")),
            )
            .child(
                div()
                    .id("agent-prep-conversations")
                    .flex()
                    .flex_col()
                    .gap(space::SPACE_2)
                    .max_h(ROW_HEIGHT * VISIBLE_ROWS)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows),
            )
            .children(note.map(|note| {
                div()
                    .id("agent-prep-conversation-note")
                    .test_support()
                    .aria_label(note.clone())
                    .child(modal_note(note, tokens))
            }))
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
        let working = self.working.is_some();
        let starting = self.prep.start.is_some();
        let question = if self.only_update() {
            t!("agent.prep.question_update", agent = self.prep.name.clone())
        } else {
            t!("agent.prep.question", agent = self.prep.name.clone())
        };
        let rows: Vec<_> = self
            .prep
            .steps
            .iter()
            .enumerate()
            .map(|(index, step)| {
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
                    .gap(space::SPACE_8)
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
                                .child(detail),
                        ),
                        (None, None) => row,
                    })
            })
            .collect();
        let steps = (!self.prep.steps.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(space::SPACE_8)
                .child(modal_question(question, &tokens))
                .child(div().flex().flex_col().children(rows))
                .child(modal_note(t!("agent.prep.note"), &tokens))
        });
        let folder = starting.then(|| self.folder_row(&tokens, window, cx));
        let conversations = starting.then(|| self.conversation_list(&tokens, cx));
        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .children(folder)
            .children(conversations)
            .children(steps);

        let ready = !working && (!starting || self.conversations.is_some());
        let (confirm_id, confirm_label) = if starting {
            ("agent-prep-start", t!("agent.start.start"))
        } else if self.only_update() {
            ("agent-prep-confirm", t!("agent.prep.confirm_update"))
        } else {
            ("agent-prep-confirm", t!("agent.prep.confirm"))
        };
        let footer = div()
            .flex()
            .gap(space::SPACE_6)
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
                confirm_id,
                confirm_label,
                Some("Enter"),
                ModalButtonTone::Primary,
                ready.then(|| -> OnClick {
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
                match event.keystroke.key.as_str() {
                    "enter" => this.confirm(window, cx),
                    "up" if this.prep.start.is_some() => this.step_selection(false, cx),
                    "down" if this.prep.start.is_some() => this.step_selection(true, cx),
                    _ => {}
                }
            }))
    }
}

/// 叠一个启动 Agent 的框。Esc 等于取消。焦点给框。
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
    let directory = prep
        .start
        .as_ref()
        .and_then(|start| start.directory.clone());
    let view = cx.new(|cx| {
        let mut dialog = PrepDialog {
            prep,
            directory: None,
            conversations: Some(Vec::new()),
            selected: None,
            _listing: None,
            scroll: ScrollHandle::new(),
            failed: None,
            store,
            integration,
            layer: layer.clone(),
            on_ready: on_ready.clone(),
            working: None,
            focus_handle: cx.focus_handle(),
        };
        dialog.set_directory(directory, cx);
        dialog
    });
    let focus = view.read(cx).focus_handle.clone();
    layer.open_dismissable(
        move |dialog, _, cx| modal_dialog(dialog, &Tokens::current(cx)).child(view.clone()),
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
