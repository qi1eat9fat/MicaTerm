//! 直接问 Windows 的几件 GPUI 没给的事：要窗口的系统句柄。

use gpui_kit::Window;
use windows::Win32::Foundation::HWND;

/// 窗口的系统句柄。拿不到（无头测试的窗口没有）就是 `None`。
pub fn hwnd(window: &Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    // `Window` 自己也有个 `window_handle`（GPUI 的窗口 id），这里要的是系统的句柄。
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(HWND(handle.hwnd.get() as *mut std::ffi::c_void))
}

/// 窗口是不是最小化着。GPUI 的 `is_maximized` 在最小化时是 false，分不出最小化前是不是最大化。
pub fn is_minimized(window: &Window) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::IsIconic;

    // SAFETY: `hwnd` 是这个窗口自己的、活着的句柄；`IsIconic` 只读窗口状态。
    hwnd(window).is_some_and(|hwnd| unsafe { IsIconic(hwnd) }.as_bool())
}
