//! Agent › Agent 命令（Agent CLI 工单 07）：应用内启动 Agent 时运行的命令。留空是自动检测
//! （先找 claude，再找 codex），占位字写检测到的那个。每次改动立刻写进设置。

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;

use super::SettingsStore;
use super::controls::field_input;
use super::pages::WIDE_WIDTH;
use crate::agent::agent_program;
use crate::i18n::t;
use crate::theme::Tokens;

/// 输入框的文字（设置在它下面变时也保住光标）。
pub struct AgentCommandEditor {
    store: Entity<SettingsStore>,
    input: Entity<InputState>,
    _subscriptions: [Subscription; 2],
    _scan: Task<()>,
}

impl AgentCommandEditor {
    pub fn new(store: Entity<SettingsStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let command = stored(&store, cx).unwrap_or_default();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("settings.cli.agent_command_detecting"))
                .default_value(command)
        });
        let subscriptions = [
            cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let command = input.read(cx).value().trim().to_string();
                    this.store.update(cx, |store, cx| {
                        store.update(
                            |settings| {
                                settings.cli.agent_command =
                                    (!command.is_empty()).then_some(command)
                            },
                            cx,
                        )
                    });
                }
            }),
            // 别处的改动（恢复默认）显示到输入框里。
            cx.observe_in(&store, window, |this, store, window, cx| {
                let command = stored(&store, cx).unwrap_or_default();
                if this.input.read(cx).value().trim() != command {
                    this.input
                        .update(cx, |input, cx| input.set_value(command, window, cx));
                }
            }),
        ];
        // 在 PATH 里找要看文件系统，放到后台。
        let scan = cx.spawn_in(window, async move |this, cx| {
            let found = cx.background_spawn(async { agent_program(None) }).await;
            this.update_in(cx, |this, window, cx| {
                let placeholder = match found {
                    Some(program) => t!(
                        "settings.cli.agent_command_found",
                        command = program.command
                    ),
                    None => t!("settings.cli.agent_command_none"),
                };
                this.input.update(cx, |input, cx| {
                    input.set_placeholder(placeholder, window, cx)
                });
            })
            .ok();
        });
        Self {
            store,
            input,
            _subscriptions: subscriptions,
            _scan: scan,
        }
    }
}

/// 设置里存的命令，`None` 是自动检测。
fn stored(store: &Entity<SettingsStore>, cx: &App) -> Option<String> {
    store.read(cx).settings().cli.agent_command
}

impl Render for AgentCommandEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        div().w(WIDE_WIDTH).child(field_input(
            Input::new(&self.input)
                .id("agent-command")
                .aria_label(t!("settings.cli.agent_command")),
            focused,
            &tokens,
        ))
    }
}
