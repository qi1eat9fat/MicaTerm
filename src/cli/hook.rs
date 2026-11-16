//! `micaterm hook <claude|codex> [JSON]`（Agent CLI 工单 09）：Agent 的状态 hook 调它，它把事件报给
//! 正在运行的 MicaTerm。Claude Code 把 JSON 写在 stdin，Codex 的 `notify` 把 JSON 当最后一个参数。
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
    let text = match payload {
        Some(payload) => payload,
        // 从终端里手敲 `micaterm hook claude` 时 stdin 是终端：不等它。
        None if io::stdin().is_terminal() => return None,
        None => {
            let mut text = String::new();
            io::stdin()
                .take(MAX_PAYLOAD)
                .read_to_string(&mut text)
                .ok()?;
            text
        }
    };
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let (event, message) = hook_event(source, &value)?;
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
