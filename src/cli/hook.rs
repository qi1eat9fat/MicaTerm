//! `micaterm hook <claude|codex|agy> [事件名或 JSON]`（Agent CLI 工单 09）：Agent 的状态 hook 调它，
//! 它把事件报给正在运行的 MicaTerm。Claude Code 把 JSON 写在 stdin；Codex 的 `notify` 把 JSON 当最后
//! 一个参数；Antigravity CLI 把事件名当参数、JSON 写在 stdin。
//!
//! 不能卡住 Agent：不在 MicaTerm 的本地卡片里（没有 `MICATERM_SELF_CARD`）、认不出的事、连不上、
//! 外部 Agent 没启用，一律什么也不说、退出码 0；报出去最多等 500 毫秒，不等应用回话。

use std::io::{self, IsTerminal as _, Read as _};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

use super::agent_env::SELF_CARD_ENV;
use super::client;
use super::protocol::Request;
use crate::agent::{HookSource, hook_event};

/// 最多等多久把事件交出去。
const SEND_TIMEOUT: Duration = Duration::from_millis(500);
/// stdin 最多读多少：hook 的载荷是一小段 JSON，别的不要。
const MAX_PAYLOAD: u64 = 1024 * 1024;

/// 从 stdin 读 hook 的载荷；从终端里手敲时 stdin 是终端，不等它。
fn read_stdin() -> Option<String> {
    if io::stdin().is_terminal() {
        return None;
    }
    let mut text = String::new();
    io::stdin()
        .take(MAX_PAYLOAD)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// 总是 0。
pub fn run(source: &str, payload: Option<String>) -> i32 {
    let _ = report(source, payload);
    0
}

fn report(source: &str, payload: Option<String>) -> Option<()> {
    let card = std::env::var(SELF_CARD_ENV)
        .ok()
        .filter(|card| !card.is_empty())?;
    let source = HookSource::parse(source)?;
    // Antigravity CLI 把事件名当参数，JSON 照旧写在 stdin；别家是 JSON 当参数或者写在 stdin。
    let (given, text) = match source {
        HookSource::Antigravity => {
            let event = payload.filter(|event| !event.trim().is_empty())?;
            (Some(event), read_stdin()?)
        }
        _ => (
            None,
            match payload {
                Some(payload) => payload,
                None => read_stdin()?,
            },
        ),
    };
    // Antigravity 的载荷认不出（或者没给）也不影响按事件名报；Claude Code、Codex 只认 JSON。
    let value: Value = match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(value) => value,
        Err(_) if source == HookSource::Antigravity => Value::Null,
        Err(_) => return None,
    };
    let (event, message) = hook_event(source, given.as_deref(), &value)?;
    let endpoint = crate::app::cli_endpoint(&crate::app::data_dir().ok()?);
    let request = Request::Hook {
        card,
        event,
        message,
    };
    let (sent, done) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sent.send(client::send(&endpoint, request));
    });
    done.recv_timeout(SEND_TIMEOUT).ok()?.ok()
}
