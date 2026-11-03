//! 终端自己的文案：`locales/terminal.yml`，只有简体中文。用法同应用的 `i18n`：
//! `t!("terminal.menu.copy")`、`t!("terminal.notice.exited", code = code)`，返回 `SharedString`。
// 参考 since2006/shell-rs @ b2a307a: src/i18n.rs（只留 zh-CN 和 `t!`）

use std::borrow::Cow;

use gpui_kit::SharedString;

/// 文案所用的语言。
pub const LOCALE: &str = "zh-CN";

macro_rules! t {
    ($key:expr $(,)?) => {
        $crate::i18n::shared($crate::_rust_i18n_t!($key, locale = $crate::i18n::LOCALE))
    };
    ($key:expr, $($args:tt)+) => {
        $crate::i18n::shared($crate::_rust_i18n_t!(
            $key,
            locale = $crate::i18n::LOCALE,
            $($args)+
        ))
    };
}
pub(crate) use t;

/// gpui 要的文本类型，已经是 `'static` 的就不再拷贝。
#[doc(hidden)]
pub fn shared(text: Cow<'static, str>) -> SharedString {
    match text {
        Cow::Borrowed(text) => SharedString::new_static(text),
        Cow::Owned(text) => text.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use regex::Regex;

    use super::LOCALE;

    fn messages() -> BTreeMap<String, String> {
        crate::_rust_i18n_backend()
            .messages_for_locale(LOCALE)
            .unwrap_or_default()
            .into_iter()
            .map(|(key, text)| (key.into_owned(), text.into_owned()))
            .collect()
    }

    /// 代码里的键都有文案，文案都被用到。
    #[test]
    fn every_key_in_the_code_has_text_and_every_text_is_used() {
        let texts = messages();
        assert!(!texts.is_empty());
        let call = Regex::new(r#"\bt!\(\s*"([^"]+)""#).expect("pattern");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut code = String::new();
        for entry in fs::read_dir(dir).expect("src") {
            let path = entry.expect("entry").path();
            let text = fs::read_to_string(&path).expect("source");
            code.extend(
                text.lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .flat_map(|line| [line, "\n"]),
            );
        }
        let mut problems = Vec::new();
        for capture in call.captures_iter(&code) {
            if !texts.contains_key(&capture[1]) {
                problems.push(format!("{} has no text", &capture[1]));
            }
        }
        for key in texts.keys() {
            if !code.contains(&format!("\"{key}\"")) {
                problems.push(format!("{key} is used nowhere"));
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
