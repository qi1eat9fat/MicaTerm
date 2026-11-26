//! 图标资源：gpui-kit 默认的那批 Lucide 图标，加上 MicaTerm 额外要用的。
// 参考 since2006/shell-rs @ b2a307a: src/app/assets.rs

use std::borrow::Cow;

use gpui_kit::assets::icon_assets;
use gpui_kit::{AssetSource, Result, SharedString};

// 默认组件图标包以外的图标，从 gpui-kit-assets 自带的 Lucide 目录里挑。要用新图标就加在这里。
// Terminal：新标签页「新建本地终端」；CodeXml：右侧栏「命令片段」；KeyRound：右侧栏「凭据」。
// Columns2 / Rows2 / Maximize2 / Minimize2：分屏、放大、还原（工单 05）。
icon_assets!(
    ExtraIcons,
    [
        Terminal,
        CodeXml,
        KeyRound,
        Columns2,
        Rows2,
        Maximize2,
        Minimize2,
        // 程序图块（工单 06，`crate::programs`）。
        Activity,
        Globe,
        Cpu,
        FileCode,
        Diff,
        GitBranch,
        // 命令片段面板：悬停时的 ⚡（点击就执行，工单 19）。
        Zap,
        // 保险库（工单 13）：解锁小卡片和主机栏底部的锁、设置主密码框和设置 › 安全。
        Lock,
        LockOpen,
        Shield,
        // 主机密钥变了的警示（26 号 `shieldx`，UI 重做工单 06）。
        ShieldX,
        // 设置 › 应用（工单 26）。
        AppWindow,
        // 应用内的 Agent（Agent CLI 工单 07）：右侧图标栏的「Agent」、启动前确认框里要做的那几行。
        Sparkles,
        Circle,
        // 设置标签：改过的设置标题后面的「恢复默认」，导航里的「终端」（UI 重做工单 12，26 号 `undo`、`term`）。
        Undo2,
        SquareTerminal,
        // Muxy 空壳的标签栏、右侧面板和状态栏用到的 Lucide 图标。
        GitFork,
        GitPullRequest,
        ArrowUpDown,
        CircleArrowUp,
        SquareArrowOutUpRight,
        PinOff,
        // SFTP 标签、编辑器标签、预览（M3，工单 20 一次加全）。
        ArrowUpRight,
        // 编辑器查找浮卡的「替换」那一行（UI 重做工单 11，26 号 `replace`）。
        ArrowRightLeft,
        CircleMinus,
        Download,
        Eraser,
        Eye,
        EyeOff,
        FilePenLine,
        FilePlus,
        FileSymlink,
        FileText,
        FolderPlus,
        FolderSymlink,
        FolderUp,
        Laptop,
        Monitor,
        Pencil,
        Play,
        RefreshCw,
        Save,
        Server,
        Square,
        SquarePen,
        Trash,
        Upload,
        ZoomIn,
        ZoomOut
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

/// 主机的系统徽章（工单 11）：每个 `HostOs` 一个文件，路径就是 `HostOs::icon_path()`。
/// 来自 shell-rs 的 assets/icons/os/（Simple Icons，CC0；Windows 那个是 shell-rs 自己画的），
/// 许可见 `assets/icons/os/LICENSE.md`。
macro_rules! os_icons {
    ($($name:literal),* $(,)?) => {
        const OS_ICONS: &[(&str, &[u8])] = &[
            $((
                concat!("icons/os/", $name, ".svg"),
                include_bytes!(concat!("../../assets/icons/os/", $name, ".svg")),
            )),*
        ];
    };
}

os_icons!(
    "almalinux",
    "alpinelinux",
    "apple",
    "archlinux",
    "centos",
    "debian",
    "fedora",
    "freebsd",
    "gentoo",
    "kalilinux",
    "linux",
    "manjaro",
    "netbsd",
    "openbsd",
    "opensuse",
    "raspberrypi",
    "redhat",
    "rockylinux",
    "ubuntu",
    "windows",
);

fn os_icon(path: &str) -> Option<Cow<'static, [u8]>> {
    OS_ICONS
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
}

/// MicaTerm 彩色应用图标（棱光云母 M 标识 + 终端光标）：用于侧栏顶栏。
pub const LOGO_IMAGE: &str = "logo/micaterm.png";
const LOGO_IMAGE_BYTES: &[u8] = include_bytes!("../../assets/logo/micaterm.png");

/// MicaTerm 线稿标志（与彩色应用图标同构的折叠 M + 终端光标，两种笔画粗细；
/// 新标签页 72 的背景水印用细线版）。
pub const MARK_ICON: &str = "icons/micaterm/mark.svg";
pub const MARK_THIN_ICON: &str = "icons/micaterm/mark-thin.svg";
const MARK_ICONS: &[(&str, &[u8])] = &[
    (
        MARK_ICON,
        include_bytes!("../../assets/icons/micaterm/mark.svg"),
    ),
    (
        MARK_THIN_ICON,
        include_bytes!("../../assets/icons/micaterm/mark-thin.svg"),
    ),
];

/// 图标名，凡是建 `Icon` 的地方都能用（`ExtraIcons` 和默认图标包里的都在里面）。
pub use gpui_kit::assets::IconName as CatalogIcon;

/// 应用的资源来源：先查额外图标，再回落到默认组件图标包。
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        if let Some(bytes) = brand_icon(path) {
            return Ok(Some(bytes));
        }
        if let Some(bytes) = os_icon(path) {
            return Ok(Some(bytes));
        }
        if path == LOGO_IMAGE {
            return Ok(Some(Cow::Borrowed(LOGO_IMAGE_BYTES)));
        }
        if let Some((_, bytes)) = MARK_ICONS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        if LOGO_IMAGE.starts_with(path) {
            paths.push(SharedString::from(LOGO_IMAGE));
        }
        paths.extend(
            BRAND_ICONS
                .iter()
                .map(|(name, _)| *name)
                .filter(|name| name.starts_with(path))
                .map(SharedString::from),
        );
        paths.extend(
            OS_ICONS
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

    /// 每种系统的徽章图标都加载得出来（工单 11）。
    #[test]
    fn every_host_os_icon_loads() {
        for os in micaterm_hosts::HostOs::ALL {
            let bytes = AppAssets.load(os.icon_path()).unwrap();
            assert!(
                bytes.is_some_and(|bytes| bytes.starts_with(b"<svg")),
                "{os:?}"
            );
        }
    }

    /// MicaTerm 自己画的图标（两种笔画粗细的标志）都加载得出来。
    #[test]
    fn every_own_icon_loads() {
        for (path, _) in super::MARK_ICONS {
            let bytes = AppAssets.load(path).unwrap();
            assert!(
                bytes.is_some_and(|bytes| bytes.starts_with(b"<svg")),
                "{path}"
            );
        }
    }

    /// 彩色应用图标 PNG 加载得出来。
    #[test]
    fn logo_image_loads() {
        let bytes = AppAssets.load(super::LOGO_IMAGE).unwrap();
        assert!(bytes.is_some_and(|bytes| bytes.starts_with(b"\x89PNG\r\n\x1a\n")));
    }

    /// 程序名表里的每个图标都加载得出来：Lucide 的在 gpui-kit 的图标包里，品牌的是 MicaTerm 自带的。
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
