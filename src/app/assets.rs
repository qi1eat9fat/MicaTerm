//! 图标资源：gpui-kit 默认的那批 Lucide 图标，加上 QiShell 额外要用的。
// 参考 since2006/shell-rs @ b2a307a: src/app/assets.rs

use std::borrow::Cow;

use gpui_kit::assets::icon_assets;
use gpui_kit::{AssetSource, Result, SharedString};
use qishell_terminal::TerminalIcons;

// 默认组件图标包以外的图标，从 gpui-kit-assets 自带的 Lucide 目录里挑。要用新图标就加在这里。
// Terminal：开始卡片「新建本地终端」；CodeXml：右侧栏「命令片段」；KeyRound：右侧栏「凭据」。
// Columns2 / Rows2 / Maximize2 / Minimize2：卡片头部的向右分屏、向下分屏、放大、还原（工单 05）。
icon_assets!(
    ExtraIcons,
    [
        Terminal, CodeXml, KeyRound, Columns2, Rows2, Maximize2, Minimize2,
        // 程序图块（工单 06，`crate::programs`）。
        Activity, Globe, Cpu, FileCode, Diff, GitBranch,
        // 命令片段面板：悬停时的 ⚡（点击就执行，工单 19）。
        Zap
    ]
);

/// 程序图块上的品牌图标：Simple Icons（CC0，许可见 `assets/icons/brands/LICENSE.md`），
/// 取自 gorex 带的那一份。路径和 `crate::programs` 的表对应。
macro_rules! brand_icons {
    ($($name:literal),* $(,)?) => {
        const BRAND_ICONS: &[(&str, &[u8])] = &[
            $((
                concat!("icons/brands/", $name, ".svg"),
                include_bytes!(concat!("../../assets/icons/brands/", $name, ".svg")),
            )),*
        ];
    };
}

brand_icons!(
    "bun",
    "claude",
    "deno",
    "docker",
    "git",
    "gnuemacs",
    "go",
    "helix",
    "kotlin",
    "lua",
    "mysql",
    "neovim",
    "nodedotjs",
    "npm",
    "openai",
    "php",
    "pnpm",
    "postgresql",
    "python",
    "redis",
    "ruby",
    "rust",
    "swift",
    "tmux",
    "vim",
    "yarn",
);

fn brand_icon(path: &str) -> Option<Cow<'static, [u8]>> {
    BRAND_ICONS
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
}

/// 图标名，凡是建 `Icon` 的地方都能用（`ExtraIcons` 和默认图标包里的都在里面）。
pub use gpui_kit::assets::IconName as CatalogIcon;

/// 应用的资源来源：先查额外图标和终端右键菜单的图标，再回落到默认组件图标包。
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        if let Some(bytes) = TerminalIcons.load(path)? {
            return Ok(Some(bytes));
        }
        if let Some(bytes) = brand_icon(path) {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.extend(TerminalIcons.list(path)?);
        paths.extend(
            BRAND_ICONS
                .iter()
                .map(|(name, _)| *name)
                .filter(|name| name.starts_with(path))
                .map(SharedString::from),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AssetSource as _;

    use super::AppAssets;

    /// 程序名表里的每个图标都加载得出来：Lucide 的在 gpui-kit 的图标包里，品牌的是 QiShell 自带的。
    #[test]
    fn every_program_glyph_loads() {
        for glyph in crate::programs::glyphs() {
            let bytes = AppAssets.load(glyph).unwrap();
            assert!(
                bytes.is_some_and(|bytes| bytes.starts_with(b"<svg")),
                "{glyph}"
            );
        }
    }
}
