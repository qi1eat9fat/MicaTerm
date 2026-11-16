//! 屏幕规则（Agent CLI 工单 09）：没装状态 hook 时，看 Agent 终端底部几行认出它在干什么。
//! 只认 Claude Code、Codex，只看界面结构（选项框、底栏提示、提示符），不看回答正文里的字。
// 来自 Kuddev/pebrel（GPL-3.0）: nebula_app/src/agent_detection/claude.toml、codex.toml 的规则，
// 和 nebula_app/src/ai_agents.rs 的区域切分与匹配（`region`、`CompiledGate::matches`）；改写成 Rust 数据，
// 去掉别家 Agent、身份识别和用户覆盖文件。

use std::sync::OnceLock;

use regex::Regex;

use crate::cli::AgentKind;

/// 屏幕上看得出的样子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenState {
    /// 提示符在等新的输入。
    Idle,
    /// 回合在进行：底栏有「esc to interrupt」。
    Working,
    /// 在等用户确认或回答（选项框）。
    Blocked,
}

/// 规则看屏幕的哪一段。
#[derive(Clone, Copy)]
enum Region {
    /// 最后 n 个非空行。
    Bottom(usize),
    /// 最后一条横线（`───`）以下。
    AfterRule,
}

/// 一组条件：`contains` 全都有（不分大小写）、`lines` 每个正则都有一行对得上、
/// `any` 至少一个成立（空就不管）、`not` 一个都不成立。
#[derive(Clone, Copy)]
struct Gate {
    contains: &'static [&'static str],
    lines: &'static [&'static str],
    any: &'static [Gate],
    not: &'static [Gate],
}

const fn contains(needles: &'static [&'static str]) -> Gate {
    Gate {
        contains: needles,
        lines: &[],
        any: &[],
        not: &[],
    }
}

const fn line(patterns: &'static [&'static str]) -> Gate {
    Gate {
        contains: &[],
        lines: patterns,
        any: &[],
        not: &[],
    }
}

struct Rule {
    state: ScreenState,
    priority: u32,
    region: Region,
    gate: Gate,
}

/// 提示符行在回合进行中也看得见（可以排队输入），所以这几样出现时不算空闲。
const CLAUDE_NOT_IDLE: &[Gate] = &[
    contains(&["esc to cancel"]),
    contains(&["enter to select"]),
    contains(&["esc to interrupt"]),
];

const CLAUDE: &[Rule] = &[
    Rule {
        state: ScreenState::Blocked,
        priority: 900,
        region: Region::AfterRule,
        gate: Gate {
            contains: &["esc to cancel"],
            lines: &[],
            any: &[
                contains(&["enter to select"]),
                contains(&["enter to confirm"]),
                contains(&["do you want to proceed?"]),
                contains(&["tab to amend"]),
            ],
            not: &[],
        },
    },
    Rule {
        state: ScreenState::Working,
        priority: 400,
        region: Region::Bottom(8),
        gate: contains(&["esc to interrupt"]),
    },
    Rule {
        state: ScreenState::Idle,
        priority: 100,
        region: Region::Bottom(4),
        gate: Gate {
            contains: &[],
            lines: &[r"^\s*❯(?:\s|$)"],
            any: &[],
            not: CLAUDE_NOT_IDLE,
        },
    },
    // 2.x 的底栏把提示符放在倒数第六个非空行：有模式切换提示时才往上多看两行。
    Rule {
        state: ScreenState::Idle,
        priority: 100,
        region: Region::Bottom(6),
        gate: Gate {
            contains: &["(shift+tab to cycle)"],
            lines: &[r"^\s*❯(?:\s|$)"],
            any: &[],
            not: CLAUDE_NOT_IDLE,
        },
    },
];

const CODEX: &[Rule] = &[
    Rule {
        state: ScreenState::Blocked,
        priority: 900,
        region: Region::Bottom(12),
        gate: Gate {
            contains: &[],
            lines: &[],
            any: &[
                contains(&["press enter to confirm or esc to cancel"]),
                contains(&["enter to submit answer"]),
                contains(&["enter to submit all"]),
                contains(&["allow command?"]),
                contains(&["tab to add notes", "enter to submit"]),
            ],
            not: &[],
        },
    },
    Rule {
        state: ScreenState::Working,
        priority: 400,
        region: Region::Bottom(16),
        gate: Gate {
            contains: &[],
            lines: &[],
            any: &[
                line(&[
                    r"^[•◦]\s+(?:Working|Compacting context) \([^)]*esc to interrupt\)(?: · .*)?$",
                ]),
                line(&[
                    r"^[•◦]\s+Messages to be submitted after next tool call \(press esc to interrupt and send immediately\)$",
                ]),
            ],
            not: &[],
        },
    },
    Rule {
        state: ScreenState::Idle,
        priority: 100,
        region: Region::Bottom(4),
        gate: Gate {
            contains: &[],
            lines: &[r"^\s*›(?:\s|$)"],
            any: &[],
            not: &[contains(&["esc to interrupt"])],
        },
    },
];

/// `kind` 的 Agent 屏幕现在是什么样；哪条规则都对不上是 `None`（说不准，不改状态）。
/// `screen` 是终端底部的若干行（`cards read` 那样读出来的，软换行接好）。
pub fn read_screen(kind: AgentKind, screen: &str) -> Option<ScreenState> {
    let rules = match kind {
        AgentKind::ClaudeCode => CLAUDE,
        AgentKind::Codex => CODEX,
        AgentKind::Generic => return None,
    };
    let mut lower = None;
    rules
        .iter()
        .filter(|rule| {
            let text = region(screen, rule.region);
            lower = None;
            matches(&rule.gate, text, &mut lower)
        })
        .max_by_key(|rule| rule.priority)
        .map(|rule| rule.state)
}

fn matches(gate: &Gate, text: &str, lower: &mut Option<String>) -> bool {
    gate.contains.iter().all(|needle| {
        lower
            .get_or_insert_with(|| text.to_lowercase())
            .contains(needle)
    }) && gate
        .lines
        .iter()
        .all(|pattern| text.lines().any(|line| regex(pattern).is_match(line)))
        && (gate.any.is_empty() || gate.any.iter().any(|gate| matches(gate, text, lower)))
        && !gate.not.iter().any(|gate| matches(gate, text, lower))
}

/// 规则里的正则：编一次记住（规则是常量，总共不到十个）。
fn regex(pattern: &'static str) -> &'static Regex {
    static COMPILED: OnceLock<std::sync::Mutex<Vec<(&'static str, &'static Regex)>>> =
        OnceLock::new();
    let compiled = COMPILED.get_or_init(Default::default);
    let mut compiled = compiled.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((_, regex)) = compiled.iter().find(|(known, _)| *known == pattern) {
        return regex;
    }
    let regex: &'static Regex = Box::leak(Box::new(
        Regex::new(pattern)
            .expect("the screen rules' patterns are constants: a typo fails the tests"),
    ));
    compiled.push((pattern, regex));
    regex
}

fn region(screen: &str, region: Region) -> &str {
    match region {
        Region::Bottom(count) => {
            let lines: Vec<&str> = screen.lines().collect();
            let Some(start) = lines
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, line)| !line.trim().is_empty())
                .take(count)
                .last()
                .map(|(index, _)| index)
            else {
                return "";
            };
            let offset = lines[..start]
                .iter()
                .map(|line| line.len() + 1)
                .sum::<usize>()
                .min(screen.len());
            &screen[offset..]
        }
        Region::AfterRule => {
            let (mut offset, mut last) = (0, 0);
            for line in screen.lines() {
                offset = (offset + line.len() + 1).min(screen.len());
                if line.trim().chars().filter(|c| *c == '─').count() >= 3 {
                    last = offset;
                }
            }
            &screen[last..]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ScreenState, read_screen};
    use crate::cli::AgentKind;

    const CLAUDE_ASKING: &str = "\
● Bash(micaterm exec Kq3m9TzW1aB7xYp2 \"sudo -n systemctl restart nginx\")

────────────────────────────────────────────
 Bash command

   micaterm exec Kq3m9TzW1aB7xYp2 \"sudo -n systemctl restart nginx\"
   Restart nginx on web-01

 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and don't ask again for micaterm exec
   3. No, and tell Claude what to do differently

 Esc to cancel · Tab to amend";

    const CLAUDE_WORKING: &str = "\
> 看一下这台机器的磁盘

● Bash(micaterm exec Kq3m9TzW1aB7xYp2 \"df -h /\")

✻ Pondering… (12s · ↑ 1.2k tokens · esc to interrupt)

╭──────────────────────────────────────────╮
│ ❯                                        │
╰──────────────────────────────────────────╯";

    const CLAUDE_IDLE: &str = "\
● web-01 上 nginx 正常运行。要我做什么？

────────────────────────────────────────────
❯
────────────────────────────────────────────
  ? for shortcuts";

    #[test]
    fn claude_code_screens_read_as_asking_working_and_idle() {
        assert_eq!(
            read_screen(AgentKind::ClaudeCode, CLAUDE_ASKING),
            Some(ScreenState::Blocked)
        );
        assert_eq!(
            read_screen(AgentKind::ClaudeCode, CLAUDE_WORKING),
            Some(ScreenState::Working)
        );
        assert_eq!(
            read_screen(AgentKind::ClaudeCode, CLAUDE_IDLE),
            Some(ScreenState::Idle)
        );
    }

    /// 回答正文里出现「Do you want to proceed?」「Allow」之类的字不算：选项框要在最后一条横线下面，
    /// 还要有「esc to cancel」这样的底栏。
    #[test]
    fn words_in_an_answer_are_not_a_question() {
        let answer = "\
● 部署脚本会先问 \"Do you want to proceed?\"，选 Allow 就行，Enter to confirm。

────────────────────────────────────────────
❯
────────────────────────────────────────────
  ? for shortcuts";
        assert_eq!(
            read_screen(AgentKind::ClaudeCode, answer),
            Some(ScreenState::Idle)
        );
    }

    #[test]
    fn codex_screens_read_as_asking_working_and_idle() {
        let asking = "\
  Allow command?

  $ sudo -n systemctl restart nginx

› 1. Yes, allow once
  2. No, tell Codex what to do

  Press enter to confirm or esc to cancel";
        let working = "\
• Ran micaterm exec Kq3m9TzW1aB7xYp2 \"df -h /\"

• Working (8s • esc to interrupt)

› Ask Codex to do anything";
        let idle = "\
• web-01 的根分区用了 82%。

› Ask Codex to do anything

  gpt-5 high · 100% context left";
        assert_eq!(
            read_screen(AgentKind::Codex, asking),
            Some(ScreenState::Blocked)
        );
        assert_eq!(
            read_screen(AgentKind::Codex, working),
            Some(ScreenState::Working)
        );
        assert_eq!(read_screen(AgentKind::Codex, idle), Some(ScreenState::Idle));
    }

    #[test]
    fn an_unrecognised_screen_says_nothing() {
        assert_eq!(
            read_screen(AgentKind::ClaudeCode, "PS C:\\Users\\Qi> "),
            None
        );
        assert_eq!(read_screen(AgentKind::Generic, CLAUDE_ASKING), None);
    }
}
