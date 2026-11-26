// 来自 since2006/shell-rs @ b2a307a: src/editor/model.rs
// 裁剪：去掉 `EditorId`、`EditorKey`；改造：状态栏按原型分成光标、编码、换行、语言四段。
//! 编辑器不用窗口就能决定的事：文件用哪个语法高亮、怎么缩进、状态栏写什么。

use gpui_kit::SharedString;
use gpui_kit::component::input::TabSize;

use crate::backend::sftp::TextFormat;
use crate::i18n::t;

/// 按文件名选语法：编进来的语言之一，其余是 `text`。
pub fn language_for(file_name: &str) -> &'static str {
    let name = file_name.to_ascii_lowercase();
    match name.as_str() {
        "makefile" | "gnumakefile" => return "make",
        ".bashrc" | ".bash_profile" | ".bash_aliases" | ".bash_logout" | ".profile" | ".zshrc"
        | ".zprofile" | ".envrc" => return "bash",
        "pipfile" | "cargo.lock" => return "toml",
        _ => {}
    }
    let Some((_, extension)) = name.rsplit_once('.') else {
        return "text";
    };
    match extension {
        "sh" | "bash" | "zsh" | "ksh" | "env" => "bash",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "json" | "jsonc" => "json",
        "py" | "pyi" | "pyw" => "python",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "html" | "htm" => "html",
        "css" | "scss" => "css",
        "md" | "markdown" => "markdown",
        "sql" => "sql",
        "php" | "phtml" => "php",
        "go" => "go",
        "lua" => "lua",
        "mk" | "mak" => "make",
        "diff" | "patch" => "diff",
        _ => "text",
    }
}

/// Tab 键怎么缩进。Makefile 必须用真的制表符，主要靠制表符缩进的文件也跟着用；
/// 其余用空格，宽度按语言的习惯。
pub fn indentation(language: &str, text: &str) -> TabSize {
    let (mut tabs, mut spaces) = (0, 0);
    for line in text.lines().take(2000) {
        if line.starts_with('\t') {
            tabs += 1;
        } else if line.starts_with("  ") {
            spaces += 1;
        }
    }
    let hard_tabs = language == "make" || tabs > spaces;
    let tab_size = match language {
        "yaml" | "json" | "html" | "css" | "javascript" | "lua" => 2,
        _ => 4,
    };
    TabSize {
        tab_size,
        hard_tabs,
    }
}

/// 状态栏最后一段的语言名。
pub fn language_label(language: &str) -> SharedString {
    match language {
        "bash" => "Shell".into(),
        "yaml" => "YAML".into(),
        "toml" => "TOML".into(),
        "json" => "JSON".into(),
        "python" => "Python".into(),
        "javascript" => "JavaScript".into(),
        "html" => "HTML".into(),
        "css" => "CSS".into(),
        "markdown" => "Markdown".into(),
        "sql" => "SQL".into(),
        "php" => "PHP".into(),
        "go" => "Go".into(),
        "lua" => "Lua".into(),
        "make" => "Makefile".into(),
        "diff" => "Diff".into(),
        _ => t!("editor.status.plain_text"),
    }
}

/// 「第 12 行，第 5 列」，参数是编辑器从 0 数的位置。
pub fn cursor_label(line: u32, character: u32) -> SharedString {
    t!(
        "editor.status.cursor",
        line = line + 1,
        column = character + 1
    )
}

/// 编码：「UTF-8」「UTF-8 BOM」。
pub fn encoding_label(format: TextFormat) -> &'static str {
    if format.bom() { "UTF-8 BOM" } else { "UTF-8" }
}

/// 换行方式：「LF」「CRLF」。
pub fn line_end_label(format: TextFormat) -> &'static str {
    if format.crlf() { "CRLF" } else { "LF" }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::highlighter::LanguageRegistry;

    use super::*;

    #[test]
    fn a_file_gets_the_highlighter_its_name_says() {
        for (name, language) in [
            ("nginx.conf", "text"),
            ("docker-compose.yml", "yaml"),
            ("deploy.SH", "bash"),
            (".bashrc", "bash"),
            ("Makefile", "make"),
            ("rules.mk", "make"),
            ("config.toml", "toml"),
            ("package.json", "json"),
            ("app.py", "python"),
            ("site.js", "javascript"),
            ("index.html", "html"),
            ("site.css", "css"),
            ("README.md", "markdown"),
            ("init.sql", "sql"),
            ("index.php", "php"),
            ("main.go", "go"),
            ("init.lua", "lua"),
            ("fix.patch", "diff"),
            ("hosts", "text"),
            ("archive.tar.gz", "text"),
        ] {
            assert_eq!(language_for(name), language, "{name}");
        }
    }

    /// 工单要的 15 种语言都编进来了：少开一个 feature 就只是悄悄没有颜色。
    #[test]
    fn every_language_named_is_built_in() {
        let registry = LanguageRegistry::singleton();
        for name in [
            "bash",
            "yaml",
            "toml",
            "json",
            "python",
            "javascript",
            "html",
            "css",
            "markdown",
            "sql",
            "php",
            "go",
            "lua",
            "make",
            "diff",
        ] {
            assert!(
                registry
                    .language(name)
                    .is_some_and(|config| config.has_grammar()),
                "{name} 没有编进来"
            );
        }
    }

    #[test]
    fn makefiles_and_tab_indented_files_indent_with_tabs() {
        assert!(indentation("make", "all:\n    echo").hard_tabs);
        assert!(indentation("text", "a {\n\tb;\n\tc;\n}").hard_tabs);
        let yaml = indentation("yaml", "a:\n  b: 1\n");
        assert!(!yaml.hard_tabs);
        assert_eq!(yaml.tab_size, 2);
        assert_eq!(indentation("python", "").tab_size, 4);
    }

    #[test]
    fn the_status_bar_counts_from_one_and_names_the_format() {
        assert_eq!(cursor_label(11, 4), "Line 12, Column 5");
        assert_eq!(encoding_label(TextFormat::default()), "UTF-8");
        assert_eq!(line_end_label(TextFormat::default()), "LF");
        let windows = TextFormat::new(true, true);
        assert_eq!(encoding_label(windows), "UTF-8 BOM");
        assert_eq!(line_end_label(windows), "CRLF");
        assert_eq!(language_label("yaml"), "YAML");
        assert_eq!(language_label("make"), "Makefile");
    }
}
