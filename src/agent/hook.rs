//! `micaterm hook` 收到的东西（Agent CLI 工单 09）：Claude Code 的 hook 把 JSON 写在 stdin，
//! Codex 的 `notify` 把 JSON 当最后一个参数。这里只认得出的几种事，别的一律不报。
// 参照 Kuddev/pebrel（GPL-3.0）: nebula_app/src/ai_hook/protocol.rs、payload.rs 的事件归类，
// 只留 Claude Code 和 Codex。

use serde_json::Value;

use super::state::HookEvent;

/// 通知正文最多留多少个字。
const MESSAGE_MAX_CHARS: usize = 200;

/// Claude Code 的 `Notification` 只有这几种类型是在等用户：同一个事件也用来提醒「闲着等输入」。
const ACTIONABLE_NOTIFICATIONS: &[&str] = &[
    "permission",
    "permission_prompt",
    "permission_request",
    "tool_permission",
    "tool_permission_request",
    "approval_request",
    "confirmation_request",
    "input_request",
    "awaiting_input",
    "elicitation_dialog",
];

/// 报 hook 的是哪个 Agent（`micaterm hook claude` / `micaterm hook codex`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookSource {
    Claude,
    Codex,
}

impl HookSource {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "claude" => Some(HookSource::Claude),
            "codex" => Some(HookSource::Codex),
            _ => None,
        }
    }
}

/// 一次 hook 是什么事，和它带来的那句话；不认得的事 `None`。
pub fn hook_event(source: HookSource, payload: &Value) -> Option<(HookEvent, Option<String>)> {
    // 别家 hook 运行器用 camelCase 字段名，Claude Code、Codex 自己从不这样发：不是它们，别串台。
    if payload.get("hookEventName").is_some() {
        return None;
    }
    let tool = payload.get("tool_name").and_then(Value::as_str);
    let event = payload.get("hook_event_name").and_then(Value::as_str);
    let found = match (source, event) {
        (_, Some("UserPromptSubmit")) => HookEvent::PromptSubmit,
        (HookSource::Claude, Some("PreToolUse")) if tool == Some("AskUserQuestion") => {
            HookEvent::NeedsAttention
        }
        (HookSource::Codex, Some("PreToolUse")) if tool == Some("request_user_input") => {
            HookEvent::NeedsAttention
        }
        (_, Some("PreToolUse" | "PostToolUse" | "PostToolUseFailure")) => HookEvent::ToolComplete,
        (_, Some("PermissionRequest")) => HookEvent::NeedsAttention,
        (HookSource::Claude, Some("Notification")) if actionable(payload) => {
            HookEvent::NeedsAttention
        }
        (_, Some("Stop" | "StopFailure" | "Interrupt")) => HookEvent::TurnDone,
        (_, Some("SessionEnd")) => HookEvent::SessionEnd,
        (HookSource::Codex, None)
            if payload.get("type").and_then(Value::as_str) == Some("agent-turn-complete") =>
        {
            HookEvent::TurnDone
        }
        _ => return None,
    };
    let message = (found == HookEvent::NeedsAttention)
        .then(|| text(payload, &["message", "title", "reason", "description"]))
        .flatten();
    Some((found, message))
}

fn actionable(payload: &Value) -> bool {
    text(payload, &["notification_type", "notificationType"])
        .is_some_and(|kind| ACTIONABLE_NOTIFICATIONS.contains(&kind.to_ascii_lowercase().as_str()))
}

/// 第一个有字的字段，截到 [`MESSAGE_MAX_CHARS`] 个字。
fn text(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| payload.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(|value| {
            let mut short: String = value.chars().take(MESSAGE_MAX_CHARS).collect();
            if value.chars().count() > MESSAGE_MAX_CHARS {
                short.push('…');
            }
            short
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{HookSource, hook_event};
    use crate::agent::state::HookEvent;

    #[test]
    fn claude_code_hooks_map_to_events() {
        let claude = |payload| hook_event(HookSource::Claude, &payload);
        assert_eq!(
            claude(json!({"hook_event_name": "UserPromptSubmit", "prompt": "看一下磁盘"})),
            Some((HookEvent::PromptSubmit, None))
        );
        assert_eq!(
            claude(
                json!({"hook_event_name": "Notification", "notification_type": "permission_prompt",
                          "message": "Claude needs your permission to use Bash"})
            ),
            Some((
                HookEvent::NeedsAttention,
                Some("Claude needs your permission to use Bash".into())
            ))
        );
        assert_eq!(
            claude(json!({"hook_event_name": "Stop"})),
            Some((HookEvent::TurnDone, None))
        );
        assert_eq!(
            claude(
                json!({"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion", "message": "选哪个？"})
            ),
            Some((HookEvent::NeedsAttention, Some("选哪个？".into())))
        );
    }

    /// 「闲着等你输入」的提醒、没写类型的通知都不算等你确认；别家运行器的 camelCase 载荷不认。
    #[test]
    fn idle_reminders_and_foreign_payloads_are_not_events() {
        let claude = |payload| hook_event(HookSource::Claude, &payload);
        assert_eq!(
            claude(
                json!({"hook_event_name": "Notification", "notification_type": "idle_prompt",
                          "message": "Claude is waiting for your input"})
            ),
            None
        );
        assert_eq!(
            claude(json!({"hook_event_name": "Notification", "message": "anything"})),
            None
        );
        assert_eq!(
            claude(json!({"hookEventName": "Stop", "hook_event_name": "Stop"})),
            None
        );
        assert_eq!(claude(json!({"hook_event_name": "SubagentStop"})), None);
    }

    #[test]
    fn codex_notify_and_hooks_map_to_events() {
        let codex = |payload| hook_event(HookSource::Codex, &payload);
        assert_eq!(
            codex(json!({"type": "agent-turn-complete", "last-assistant-message": "done"})),
            Some((HookEvent::TurnDone, None))
        );
        assert_eq!(
            codex(json!({"hook_event_name": "PermissionRequest", "message": "Allow command?"})),
            Some((HookEvent::NeedsAttention, Some("Allow command?".into())))
        );
        assert_eq!(codex(json!({"type": "something-else"})), None);
    }

    #[test]
    fn a_long_message_is_cut_short() {
        let long = "字".repeat(300);
        let (_, message) = hook_event(
            HookSource::Claude,
            &json!({"hook_event_name": "PermissionRequest", "message": long}),
        )
        .unwrap();
        let message = message.unwrap();
        assert_eq!(message.chars().count(), 201);
        assert!(message.ends_with('…'));
    }
}
