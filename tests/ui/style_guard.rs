//! 静态源码守卫：把 `docs/ui-standard.md` 第 6 节的规则落成测试，防止硬编码样式回潮。
//!
//! 只读源码文本，不启动 GPUI。扫描 `src/` 和 `crates/` 下的 `.rs` 文件，排除注释行。
//! 违规时把「文件:行号:内容」列进 panic 信息。

use std::fs;
use std::path::{Path, PathBuf};

/// 项目根目录（`Cargo.toml` 所在）。
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 递归收集目录下的 `.rs` 文件。
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// 源码文件：路径（相对项目根、用 `/` 分隔）和内容。
fn sources() -> Vec<(String, String)> {
    let base = root();
    let mut files = Vec::new();
    collect_rs(&base.join("src"), &mut files);
    collect_rs(&base.join("crates"), &mut files);
    files
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap_or_default();
            let rel = path
                .strip_prefix(&base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            (rel, text)
        })
        .collect()
}

/// 判断一行是不是注释（`//`、`//!`、`///` 开头）。
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// 在源码里找禁用片段；`allow` 里列的是允许出现的文件前缀。
fn offenders(needles: &[&str], allow: &[&str]) -> Vec<String> {
    let mut hits = Vec::new();
    for (path, text) in sources() {
        if allow.iter().any(|prefix| path.starts_with(prefix)) {
            continue;
        }
        for (index, line) in text.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            if needles.iter().any(|needle| line.contains(needle)) {
                hits.push(format!("{path}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    hits
}

fn assert_clean(name: &str, hits: Vec<String>) {
    assert!(
        hits.is_empty(),
        "{name}（docs/ui-standard.md §6）:\n{}",
        hits.join("\n")
    );
}

/// §6：`src/theme/` 之外不许有 `text_size(px(...))`、`.text_xs()` 等字号写法。
#[test]
fn no_hardcoded_text_sizes() {
    assert_clean(
        "禁止硬编码字号，改用 `TypeScale`",
        offenders(
            &[
                "text_size(px(",
                ".text_xs()",
                ".text_sm()",
                ".text_base()",
                ".text_lg()",
            ],
            &["src/theme/"],
        ),
    );
}

/// §6：`src/theme/` 与 `src/ui/` 之外不许有圆角字面量。
#[test]
fn no_hardcoded_radii() {
    assert_clean(
        "禁止硬编码圆角，改用 `size::RADIUS_*`",
        offenders(
            &[
                "rounded(px(",
                ".rounded_sm()",
                ".rounded_md()",
                ".rounded_lg()",
            ],
            &["src/theme/", "src/ui/"],
        ),
    );
}

/// §6：`BoxShadow` 只允许在 `src/theme/elevation.rs` 里构造。
#[test]
fn no_raw_shadows() {
    assert_clean(
        "禁止直接构造 `BoxShadow`，改用 `Elevation`",
        offenders(&["BoxShadow {"], &["src/theme/elevation.rs"]),
    );
}

/// §6：不许出现字符串字面量的字体族，改用 `UI_FONT_FAMILY` / `MONO_FONT_FAMILY`。
#[test]
fn no_font_family_literals() {
    assert_clean(
        "禁止字面量字体族，改用主题常量",
        offenders(&["font_family(\""], &["src/theme/"]),
    );
}

/// §6：`src/theme/size.rs` 里的常量必须都被引用，零死常量。
#[test]
fn no_dead_size_constants() {
    let base = root();
    let size_path = base.join("src/theme/size.rs");
    let size_text = fs::read_to_string(&size_path).expect("读取 src/theme/size.rs");
    let sources = sources();

    let mut dead = Vec::new();
    for line in size_text.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let Some(name) = rest.split([':', ' ']).next() else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        // 名字带词边界，避免前缀撞车（如 `LINE` 撞 `LINE_STRONG`）。
        let used = sources.iter().any(|(path, text)| {
            if path == "src/theme/size.rs" {
                return false;
            }
            text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|word| word == name)
        });
        if !used {
            dead.push(name.to_string());
        }
    }

    assert!(
        dead.is_empty(),
        "src/theme/size.rs 有未被引用的常量（docs/ui-standard.md §6）: {}",
        dead.join(", ")
    );
}

/// 判断一行中是否包含独立标识符形式的前缀调用（如 `rgb(`，不误伤 `.to_rgb()`、`indexed_rgb(`
/// 或函数返回类型 `-> Hsla {`）。
fn contains_ident_call(line: &str, needle: &str) -> bool {
    let bytes = line.as_bytes();
    let mut start = 0;
    while let Some(pos) = line[start..].find(needle) {
        let abs = start + pos;
        let boundary = abs == 0
            || !matches!(
                bytes[abs - 1],
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.'
            );
        let return_type = needle.ends_with('{') && line[..abs].trim_end().ends_with("->");
        if boundary && !return_type {
            return true;
        }
        start = abs + needle.len();
    }
    false
}

/// §6：除主题源与品牌数据色模块外，不许直接构造原始颜色（`rgb`/`rgba`/`hsla`/`Hsla`/`Rgba`）。
#[test]
fn no_raw_colors() {
    let allow = [
        "src/theme/",
        "crates/micaterm-terminal/src/theme.rs",
        "crates/micaterm-terminal/src/terminal_view.rs",
        "crates/micaterm-ui/src/",
        "src/programs/",
        "src/card/picker.rs",
        "src/card/program_tile.rs",
        "src/host/mark.rs",
    ];
    let needles = ["rgb(", "rgba(", "hsla(", "Hsla {", "Rgba {"];
    let mut hits = Vec::new();
    for (path, text) in sources() {
        if allow.iter().any(|prefix| path.starts_with(prefix)) {
            continue;
        }
        for (index, line) in text.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            if needles
                .iter()
                .any(|needle| contains_ident_call(line, needle))
            {
                hits.push(format!("{path}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    assert_clean("禁止硬编码原始颜色，改用 `theme::Tokens`", hits);
}

/// §6：`src/theme/` 之外不许对 `tokens.<field>` 直接做 `.opacity(...)` 临时派生，统一收进 `Tokens`。
#[test]
fn no_ad_hoc_token_opacity() {
    let mut hits = Vec::new();
    for (path, text) in sources() {
        if path.starts_with("src/theme/") {
            continue;
        }
        for (index, line) in text.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            let mut rest = line;
            while let Some(pos) = rest.find("tokens.") {
                let after = &rest[pos + "tokens.".len()..];
                let field_len = after
                    .bytes()
                    .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                    .count();
                if field_len > 0 && after[field_len..].starts_with(".opacity(") {
                    hits.push(format!("{path}:{}: {}", index + 1, line.trim()));
                    break;
                }
                rest = &after[field_len..];
            }
        }
    }
    assert_clean("禁止在主题外对 `Tokens` 字段临时 `.opacity(...)`，请在 `src/theme/tokens.rs` 定义语义 token", hits);
}

/// §6：`src/theme/tokens.rs` 里 `Tokens` 的每个公开颜色字段必须都被外部引用，零死 token。
#[test]
fn no_dead_color_tokens() {
    let base = root();
    let tokens_path = base.join("src/theme/tokens.rs");
    let tokens_text = fs::read_to_string(&tokens_path).expect("读取 src/theme/tokens.rs");
    let sources = sources();

    let mut in_struct = false;
    let mut dead = Vec::new();
    for line in tokens_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub struct Tokens") {
            in_struct = true;
            continue;
        }
        if in_struct && trimmed == "}" {
            break;
        }
        if !in_struct {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("pub ") else {
            continue;
        };
        let Some((name, ty)) = rest.split_once(':') else {
            continue;
        };
        if !ty.trim().starts_with("Hsla") {
            continue;
        }
        let name = name.trim();
        let needle = format!(".{name}");
        let used = sources.iter().any(|(path, text)| {
            if path == "src/theme/tokens.rs" {
                return false;
            }
            text.lines().any(|line| {
                let mut rest = line;
                while let Some(pos) = rest.find(&needle) {
                    let after = &rest[pos + needle.len()..];
                    let boundary = after
                        .chars()
                        .next()
                        .is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'));
                    if boundary {
                        return true;
                    }
                    rest = after;
                }
                false
            })
        });
        if !used {
            dead.push(name.to_string());
        }
    }

    assert!(
        dead.is_empty(),
        "src/theme/tokens.rs 有未被引用的颜色 token（docs/ui-standard.md §6）: {}",
        dead.join(", ")
    );
}
