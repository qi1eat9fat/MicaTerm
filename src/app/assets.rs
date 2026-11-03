//! 图标资源：gpui-kit 默认的那批 Lucide 图标，加上 QiShell 额外要用的。
// 参考 since2006/shell-rs @ b2a307a: src/app/assets.rs

use std::borrow::Cow;

use gpui_kit::assets::icon_assets;
use gpui_kit::{AssetSource, Result, SharedString};

// 默认组件图标包以外的图标，从 gpui-kit-assets 自带的 Lucide 目录里挑。要用新图标就加在这里。
// Terminal：开始卡片「新建本地终端」；CodeXml：右侧栏「命令片段」；KeyRound：右侧栏「凭据」。
icon_assets!(ExtraIcons, [Terminal, CodeXml, KeyRound]);

/// 图标名，凡是建 `Icon` 的地方都能用（`ExtraIcons` 和默认图标包里的都在里面）。
pub use gpui_kit::assets::IconName as CatalogIcon;

/// 应用的资源来源：先查额外图标，再回落到默认组件图标包。
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
