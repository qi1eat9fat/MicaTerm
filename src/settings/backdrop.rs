//! 窗口背景：不透明的渐变（默认），或者 Mica（设置 › 外观 › Mica 背景，工单 26）。
//!
//! Mica 由系统在窗口后面画（DWM 的 `DWMWA_SYSTEMBACKDROP_TYPE`），GPUI 用
//! `WindowBackgroundAppearance::MicaBackdrop` 打开它；窗口壳根节点的渐变改成半透明，
//! 让 Mica 从标题栏、主机栏、卡片之间的间隙透出来（`Tokens::window_gradient`）。
//!
//! 只在 Windows 11 22H2（build 22621）及以上生效。GPUI 在更低的版本上不设 Mica，
//! 却照样把窗口清成全透明，窗口就成了一块玻璃；所以这里先看系统版本，
//! 不支持时开关开着也保持不透明（静默不生效）。

use gpui_kit::{App, Global, Window, WindowBackgroundAppearance};

/// 窗口现在用的背景。`apply` 设它，窗口壳画根节点时读它。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowBackdrop {
    /// 不透明的两色标渐变。
    #[default]
    Opaque,
    /// 系统的 Mica 材质，上面盖一层半透明的渐变。
    Mica,
}

impl Global for WindowBackdrop {}

impl WindowBackdrop {
    /// 当前的背景；还没应用过设置时是不透明。
    pub fn current(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }

    /// 设置里开了 Mica（`wanted`）时实际该用的背景：系统不支持就不透明。
    pub fn for_setting(wanted: bool) -> Self {
        if wanted && mica_supported() {
            Self::Mica
        } else {
            Self::Opaque
        }
    }

    fn appearance(self) -> WindowBackgroundAppearance {
        match self {
            Self::Opaque => WindowBackgroundAppearance::Opaque,
            Self::Mica => WindowBackgroundAppearance::MicaBackdrop,
        }
    }
}

/// 让窗口的背景和设置一致；没变的不动。
pub(super) fn apply_backdrop(wanted: bool, window: &mut Window, cx: &mut App) {
    let backdrop = WindowBackdrop::for_setting(wanted);
    if cx.try_global::<WindowBackdrop>() == Some(&backdrop) {
        return;
    }
    window.set_background_appearance(backdrop.appearance());
    cx.set_global(backdrop);
    window.refresh();
}

/// 有 Mica 的最早的 Windows build（Windows 11 22H2），和 GPUI 的判断一致。
pub const MICA_MIN_BUILD: u32 = 22621;

/// 这台电脑能不能用 Mica：Windows 11 22H2 及以上。别的系统都不能。
pub fn mica_supported() -> bool {
    windows_build().is_some_and(|build| build >= MICA_MIN_BUILD)
}

/// Windows 的 build 号。用 `RtlGetVersion`：`GetVersionEx` 会按程序清单谎报版本。
#[cfg(windows)]
fn windows_build() -> Option<u32> {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` 是有效的、可写的 `OSVERSIONINFOW`，长度字段已按要求填好。
    let status = unsafe { RtlGetVersion(&mut info) };
    status.is_ok().then_some(info.dwBuildNumber)
}

#[cfg(not(windows))]
fn windows_build() -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::{WindowBackdrop, mica_supported};

    #[test]
    fn mica_is_used_only_when_wanted_and_supported() {
        assert_eq!(WindowBackdrop::for_setting(false), WindowBackdrop::Opaque);
        let expected = if mica_supported() {
            WindowBackdrop::Mica
        } else {
            WindowBackdrop::Opaque
        };
        assert_eq!(WindowBackdrop::for_setting(true), expected);
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_build_is_read() {
        // 能跑这个测试的 Windows 至少是 Windows 10。
        assert!(super::windows_build().is_some_and(|build| build >= 10240));
    }
}
