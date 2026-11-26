//! 多语言：界面文案有英文和简体中文两套，设置 › 应用 › 语言里切换，切换后立即生效。
//!
//! 文案放在 `locales/<模块>.yml`，一个模块一个文件（`workspace.yml`、`settings.yml`……），
//! 每个键下面写 `en:` 和 `zh-CN:`。代码里用 `t!("键")` 取文案，返回 `SharedString`，按当前界面语言查表。
//! 键说用途（`workspace.start.new_host`），不写英文原句；一句话一个键，不拼片段；插值写 `%{name}`。
//! 以 `%`、`{` 开头或含 `: ` 的值要加引号，否则那条文案会悄悄没了。
//!
//! 守卫测试（本文件末尾）：每个键中英文都有、代码里的键都有文案、文案都被用到、
//! `src/` 里测试以外没有中文字面量（确实要留的那一行加 `// i18n: keep`）。
// 来自 since2006/shell-rs @ b2a307a: src/i18n.rs（裁剪：只留界面语言和运行时切换，去掉 `tn!` 和 `join_list`）

use std::borrow::Cow;

use gpui_kit::SharedString;
use serde::{Deserialize, Serialize};

/// 界面语言。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// 英文：默认，也是缺文案时的回退语言。
    #[default]
    English,
    /// 简体中文。
    ChineseSimplified,
}

impl Language {
    /// 设置里按顺序列的（分段按钮从左到右）。
    pub const ALL: [Self; 2] = [Self::ChineseSimplified, Self::English];

    /// 文案表里的语言代码（`locales/*.yml` 里的 `en:` / `zh-CN:`）。
    pub fn locale(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::ChineseSimplified => "zh-CN",
        }
    }

    /// 从语言代码认语言：`zh`、`zh-CN`、`zh-HK` 都算简体中文，其余是英文。
    fn from_locale(locale: &str) -> Self {
        if locale.split(['-', '_']).next() == Some("zh") {
            Self::ChineseSimplified
        } else {
            Self::English
        }
    }
}

/// 当前的界面语言。
pub fn language() -> Language {
    Language::from_locale(&rust_i18n::locale())
}

/// 设置界面语言：应用的文案和 gpui-kit 自己的文案（对话框按钮、输入框菜单等）一起换。
/// 界面的重画由设置的应用那一端负责（`settings::apply`）。
pub fn set_language(language: Language) {
    gpui_kit::component::set_locale(language.locale());
}

/// 回到默认语言。开窗口前调用；设置里存了别的语言时再由 `set_language` 覆盖。
pub fn init() {
    set_language(Language::default());
}

/// `key` 的文案，按当前界面语言查表，把其中每个 `%{name}` 换成给的值：
/// `t!("workspace.start.title")`、`t!("settings.store.save_failed", error = error)`。
macro_rules! t {
    ($key:expr $(,)?) => {
        $crate::i18n::shared($crate::_rust_i18n_t!($key))
    };
    ($key:expr, $($args:tt)+) => {
        $crate::i18n::shared($crate::_rust_i18n_t!($key, $($args)+))
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
    use std::path::{Path, PathBuf};

    use regex::Regex;

    use super::Language;

    /// 标记这一行的中文字面量是有意保留的。
    const KEEP: &str = "i18n: keep";

    fn messages(language: Language) -> BTreeMap<String, String> {
        crate::_rust_i18n_backend()
            .messages_for_locale(language.locale())
            .unwrap_or_default()
            .into_iter()
            .map(|(key, text)| (key.into_owned(), text.into_owned()))
            .collect()
    }

    fn sources(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("source directory") {
            let path = entry.expect("source entry").path();
            if path.is_dir() {
                sources(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    fn all_sources() -> Vec<(PathBuf, String)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        sources(&root.join("src"), &mut files);
        files.sort();
        files
            .into_iter()
            .map(|path| {
                let text = fs::read_to_string(&path).expect("readable source");
                let relative = path
                    .strip_prefix(root)
                    .expect("under the crate")
                    .to_path_buf();
                (relative, text)
            })
            .collect()
    }

    fn is_cjk(c: char) -> bool {
        matches!(c,
            '\u{3000}'..='\u{303f}'   // 中文标点：、。「」
            | '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{ff00}'..='\u{ffef}' // 全角符号：，：（）
        )
    }

    #[test]
    fn the_language_codes_pick_the_right_table() {
        assert_eq!(Language::from_locale("en"), Language::English);
        assert_eq!(Language::from_locale("zh"), Language::ChineseSimplified);
        assert_eq!(Language::from_locale("zh-CN"), Language::ChineseSimplified);
        // 中文表里确实是中文；这里按语言代码直接查，不动进程的当前语言。
        let chinese = crate::_rust_i18n_try_translate("zh-CN", "settings.appearance.language")
            .expect("the Chinese text exists");
        assert_eq!(chinese.as_ref(), "语言");
        let fallback = crate::_rust_i18n_try_translate("en", "settings.appearance.language")
            .expect("the English text exists");
        assert_eq!(fallback.as_ref(), "Language");
    }

    #[test]
    fn every_key_has_english_and_chinese() {
        let english = messages(Language::English);
        let chinese = messages(Language::ChineseSimplified);
        assert!(!english.is_empty());
        assert_eq!(
            english.keys().collect::<Vec<_>>(),
            chinese.keys().collect::<Vec<_>>()
        );
        // 英文表里不该混进中文。
        assert!(
            !english.values().any(|text| text.chars().any(is_cjk)),
            "{:#?}",
            english
                .iter()
                .filter(|(_, text)| text.chars().any(is_cjk))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn texts_follow_the_interface_text_rules() {
        let mut problems = Vec::new();
        for language in Language::ALL {
            for (key, text) in messages(language) {
                if text.contains("...") {
                    problems.push(format!(
                        "{}: {key}: three dots instead of …",
                        language.locale()
                    ));
                }
                // 列表分隔符和括号里的详情是要拼进句子的片段，需要首尾空格。
                let fragment =
                    key.ends_with(".separator") || key.starts_with("sftp_tab.close.detail");
                if text.trim() != text && !fragment {
                    problems.push(format!(
                        "{}: {key}: spaces around the text",
                        language.locale()
                    ));
                }
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn every_key_in_the_code_has_text_and_every_text_is_used() {
        let texts = messages(Language::English);
        let call = Regex::new(r#"\bt!\(\s*"([^"]+)""#).expect("pattern");
        let sources = all_sources();
        let mut problems = Vec::new();
        for (path, text) in &sources {
            // 文档注释里的键只是例子。
            let code = text
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for capture in call.captures_iter(&code) {
                let key = &capture[1];
                if !texts.contains_key(key) {
                    problems.push(format!("{}: {key} has no text", path.display()));
                }
            }
        }
        let code = sources
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for key in texts.keys() {
            if !code.contains(&format!("\"{key}\"")) {
                problems.push(format!("{key} is used nowhere"));
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// 文案一律走 `t!`：测试以外的代码里没有中文，标了保留的行除外。
    #[test]
    fn the_code_has_no_chinese_literals() {
        let mut problems = Vec::new();
        for (path, text) in all_sources() {
            let name = path.to_string_lossy().replace('\\', "/");
            if name.ends_with("tests.rs") {
                continue;
            }
            let lines = text.lines().collect::<Vec<_>>();
            for (line, literal) in literals_outside_tests(&text) {
                if literal.chars().any(is_cjk) && !lines[line].contains(KEEP) {
                    problems.push(format!("{name}:{}: {literal}", line + 1));
                }
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn the_scanner_skips_comments_tests_and_lifetimes() {
        let source = r##"
            // "注释"
            /* "块注释" /* "嵌套" */ */
            fn shown<'a>(x: &'a str) -> char { let _ = "显示"; let _ = r#"原样"#; '、' }
            #[cfg(test)]
            fn helper() { let _ = "测试"; }
            #[cfg(test)]
            mod tests { fn t() { let _ = "测试"; } }
            const AFTER: &str = "之后";
        "##;
        let found = literals_outside_tests(source)
            .into_iter()
            .map(|(_, literal)| literal)
            .collect::<Vec<_>>();
        assert_eq!(found, ["显示", "原样", "、", "之后"]);
    }

    /// `source` 里的字符串和字符字面量及其行号（从 0 起），跳过注释和 `#[cfg(test)]` 下的条目。
    fn literals_outside_tests(source: &str) -> Vec<(usize, String)> {
        let chars = source.chars().collect::<Vec<_>>();
        let mut found = Vec::new();
        let mut line = 0;
        let mut i = 0;
        // `#[cfg(test)]` 置位：条目的大括号一打开，记下在哪一层结束。
        let mut skipping = false;
        let mut skip_depth = None;
        let mut depth = 0usize;
        let at = |i: usize, text: &str| chars[i..].starts_with(&text.chars().collect::<Vec<_>>());
        while i < chars.len() {
            let c = chars[i];
            if c == '\n' {
                line += 1;
                i += 1;
            } else if at(i, "//") {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            } else if at(i, "/*") {
                let mut nesting = 0;
                while i < chars.len() {
                    if at(i, "/*") {
                        nesting += 1;
                        i += 2;
                    } else if at(i, "*/") {
                        nesting -= 1;
                        i += 2;
                        if nesting == 0 {
                            break;
                        }
                    } else {
                        if chars[i] == '\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                }
            } else if at(i, "#[cfg(test)]") || at(i, "#[cfg(any(test") || at(i, "#[cfg(all(test") {
                skipping = true;
                i += 1;
            } else if c == '"'
                || ((c == 'r' || c == 'b')
                    && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_'))
                    && raw_or_byte_start(&chars[i..]).is_some())
            {
                let start_line = line;
                let (hashes, open) = if c == '"' {
                    (None, 1)
                } else {
                    raw_or_byte_start(&chars[i..]).expect("checked")
                };
                i += open;
                let mut literal = String::new();
                while i < chars.len() {
                    let d = chars[i];
                    match hashes {
                        None if d == '\\' => {
                            literal.push(d);
                            if let Some(&next) = chars.get(i + 1) {
                                literal.push(next);
                                if next == '\n' {
                                    line += 1;
                                }
                            }
                            i += 2;
                            continue;
                        }
                        None if d == '"' => {
                            i += 1;
                            break;
                        }
                        Some(count)
                            if d == '"'
                                && chars[i + 1..]
                                    .iter()
                                    .take(count)
                                    .filter(|h| **h == '#')
                                    .count()
                                    == count =>
                        {
                            i += 1 + count;
                            break;
                        }
                        _ => {}
                    }
                    if d == '\n' {
                        line += 1;
                    }
                    literal.push(d);
                    i += 1;
                }
                if !skipping {
                    found.push((start_line, literal));
                }
            } else if c == '\'' {
                // 字符字面量（'x'、'\n'），或者生命周期（'a）。
                let end = if chars.get(i + 1) == Some(&'\\') {
                    // 转义的字符本身可能就是引号：'\''。
                    chars[i + 3..]
                        .iter()
                        .position(|d| *d == '\'')
                        .map(|n| i + 3 + n)
                } else if chars.get(i + 2) == Some(&'\'') {
                    Some(i + 2)
                } else {
                    None
                };
                match end {
                    Some(end) => {
                        if !skipping {
                            found.push((line, chars[i + 1..end].iter().collect()));
                        }
                        i = end + 1;
                    }
                    None => i += 1,
                }
            } else {
                match c {
                    '{' => {
                        depth += 1;
                        if skipping && skip_depth.is_none() {
                            skip_depth = Some(depth);
                        }
                    }
                    '}' => {
                        if skip_depth == Some(depth) {
                            skipping = false;
                            skip_depth = None;
                        }
                        depth = depth.saturating_sub(1);
                    }
                    ';' if skipping && skip_depth.is_none() => skipping = false,
                    _ => {}
                }
                i += 1;
            }
        }
        found
    }

    /// 从这里开始的原始字符串或字节串（`r"`、`r#"`、`b"`、`br#"`）：
    /// 几个 `#` 收尾（字节串有转义，是 `None`），以及开头占几个字符。
    fn raw_or_byte_start(chars: &[char]) -> Option<(Option<usize>, usize)> {
        let mut i = 0;
        let byte = chars.first() == Some(&'b');
        if byte {
            i += 1;
        }
        if chars.get(i) == Some(&'r') {
            i += 1;
            let hashes = chars[i..].iter().take_while(|c| **c == '#').count();
            i += hashes;
            (chars.get(i) == Some(&'"')).then_some((Some(hashes), i + 1))
        } else if byte && chars.get(i) == Some(&'"') {
            Some((None, i + 1))
        } else {
            None
        }
    }
}
