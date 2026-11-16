//! 交给 Agent 时告诉它的话（Agent CLI 工单 07）：交给它哪张卡片、卡片 ID、主机，怎么在那张卡片里
//! 执行命令、看屏幕。Agent 没在跑时是它启动命令的第一个参数；已在跑时填进它的输入框、不发送。
//!
//! 作为参数时要经得过 Windows 上的 `claude.cmd`（`cmd.exe` 会解释 `"`、`%`、`<`、`>`、`&`、`|`、`^`、`!`）：
//! 文案里不用这些字符，插进去的卡片名、主机名把它们去掉；也不换行。

use gpui_kit::SharedString;

use crate::i18n::t;

/// 交出去的那张卡片。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handoff {
    /// CLI 认的卡片 ID（交出去时起的）。
    pub card: SharedString,
    /// 卡片的名字（主机名）。
    pub card_name: SharedString,
    /// 主机的 ID（`micaterm hosts list` 里的）。
    pub host: SharedString,
}

/// 告诉 Agent 的一段话，一行。
pub fn handoff_prompt(handoff: &Handoff) -> String {
    let text = t!(
        "agent.prompt.handoff",
        name = plain(&handoff.card_name),
        card = plain(&handoff.card),
        host = plain(&handoff.host)
    );
    plain(&text)
}

/// 去掉 `cmd.exe` 会解释的字符和换行。
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '"' | '%' | '<' | '>' | '&' | '|' | '^' | '!' | '`'))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_names_the_card_and_both_commands_on_one_safe_line() {
        let prompt = handoff_prompt(&Handoff {
            card: "2xO1LwDSmQjK4EJM".into(),
            card_name: "web&01 \"prod\"\n%PATH%".into(),
            host: "Kq3m9TzW1aB7xYp2".into(),
        });
        assert!(
            prompt.contains("micaterm exec --card 2xO1LwDSmQjK4EJM"),
            "{prompt}"
        );
        assert!(
            prompt.contains("micaterm cards read 2xO1LwDSmQjK4EJM"),
            "{prompt}"
        );
        assert!(prompt.contains("Kq3m9TzW1aB7xYp2"), "{prompt}");
        assert!(prompt.contains("web01 prod PATH"), "{prompt}");
        for bad in ['"', '%', '<', '>', '&', '|', '^', '!', '\n'] {
            assert!(!prompt.contains(bad), "{bad:?} in {prompt}");
        }
    }
}
