//! 设置 › 应用 › 记住窗口大小和位置（工单 26）：主窗口的大小、位置和最大化状态，
//! 一变就记进数据目录的 `window.json`；下次启动按它和设置决定窗口开在哪。
// 来自 since2006/shell-rs @ b2a307a: src/workspace/window_state.rs
// （「记住大小」「记住位置」两个开关合成一个；文件路径由数据目录给，没有数据目录就不记）

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::*;
use serde::{Deserialize, Serialize};

use super::Workspace;
use crate::settings::WindowSettings;

/// 移动、缩放停下来多久才写文件：拖动时每一步都会报一次。
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// 新窗口的大小，也是不记大小时的大小。
pub fn default_window_size() -> Size<Pixels> {
    size(px(1280.), px(800.))
}

/// 窗口至少要有这么多在屏幕上，才放回原处：够抓住标题栏拖回来。
const VISIBLE_WIDTH: f32 = 120.;
const VISIBLE_HEIGHT: f32 = 40.;

/// 主窗口上次的样子。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    /// 在哪块屏幕上，用系统跨重启不变的标识。
    display: Option<String>,
    /// 不最大化、不全屏时的大小和位置，照 GPUI 给的样子存：macOS 从屏幕左上角算，别处从桌面算。
    bounds: Bounds<Pixels>,
    maximized: bool,
}

impl WindowState {
    /// 窗口现在的样子。外接屏拔掉后醒来，窗口可能暂时不在任何屏幕上；或者窗口最小化着：
    /// 这时返回 `None`，留着上一次的。
    pub fn capture(window: &Window, cx: &App) -> Option<Self> {
        // GPUI 在通知观察者之前已经更新了窗口所在的屏幕；先看它，macOS 的 `is_maximized` 假定有屏幕。
        let display = window.display(cx)?;
        Self::observe(
            display.uuid().ok().map(|uuid| uuid.to_string()),
            window.window_bounds(),
            window.is_maximized(),
            is_minimized(window),
        )
    }

    /// 从窗口报的样子得出要记的。最小化着时不记：Windows 上最大化后再最小化，报的是还原时的
    /// 大小位置、`is_maximized` 是 false，记下来就丢了最大化。最小化不改还原时的大小位置，
    /// 也不改最小化前是不是最大化，所以留着最小化前记的就对了。
    fn observe(
        display: Option<String>,
        bounds: WindowBounds,
        is_maximized: bool,
        minimized: bool,
    ) -> Option<Self> {
        if minimized {
            return None;
        }
        let (bounds, maximized) = match bounds {
            WindowBounds::Windowed(bounds) => (bounds, is_maximized),
            WindowBounds::Maximized(bounds) => (bounds, true),
            WindowBounds::Fullscreen(bounds) => (bounds, false),
        };
        Some(Self {
            display,
            bounds,
            maximized,
        })
    }

    /// 存的大小和位置。
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// 存的时候是不是最大化。
    pub fn maximized(&self) -> bool {
        self.maximized
    }

    /// 读 `path` 上存的样子；没有或读不了就是 `None`，窗口照新窗口的样子开。
    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// 整个写进 `path`，和设置一样先写临时文件再改名。
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, path)
    }
}

impl Workspace {
    /// 窗口的大小和位置一变，就记到 `path`，给下次启动用。总是记：设置只决定下次启动用不用。
    pub(super) fn remember_window(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let latest: Rc<RefCell<Option<WindowState>>> = Rc::default();
        // 退出时再写一次，免得最后一次改动还没停稳。挂在 App 上而不是这个视图上：
        // Windows 和 Linux 上关窗口时视图已经没了。
        App::on_app_quit(cx, {
            let (latest, path) = (latest.clone(), path.clone());
            move |_| {
                if let Some(state) = latest.borrow().as_ref() {
                    write(state, &path);
                }
                async {}
            }
        })
        .detach();
        let subscription = cx.observe_window_bounds(window, move |this, window, cx| {
            let Some(state) = WindowState::capture(window, cx) else {
                return;
            };
            if latest.borrow().as_ref() == Some(&state) {
                return;
            }
            *latest.borrow_mut() = Some(state.clone());
            let path = path.clone();
            this.window_state_save = Some(cx.spawn(async move |_, cx| {
                cx.background_executor().timer(SAVE_DELAY).await;
                cx.background_spawn(async move { write(&state, &path) })
                    .await;
            }));
        });
        self._subscriptions.push(subscription);
    }
}

/// 窗口是不是最小化着。GPUI 没有这个查询；macOS、Linux 上当作没有最小化，照旧记。
#[cfg(windows)]
fn is_minimized(window: &Window) -> bool {
    crate::app::win32::is_minimized(window)
}

#[cfg(not(windows))]
fn is_minimized(_: &Window) -> bool {
    false
}

/// 记窗口的位置只是方便：写不进去只记日志，不打扰用户。
fn write(state: &WindowState, path: &Path) {
    if let Err(error) = state.save(path) {
        eprintln!("qishell: cannot save the window's place: {error}");
    }
}

/// 主窗口开在哪：设置记住窗口时按 `saved`，放到现在有的屏幕上。
pub fn window_placement(
    saved: Option<&WindowState>,
    settings: WindowSettings,
    cx: &App,
) -> (WindowBounds, Option<DisplayId>) {
    let screens: Vec<Screen> = cx
        .displays()
        .iter()
        .map(|display| Screen {
            id: display.id(),
            uuid: display.uuid().ok().map(|uuid| uuid.to_string()),
            bounds: display.bounds(),
            visible: display.visible_bounds(),
        })
        .collect();
    let primary = cx.primary_display().map(|display| display.id());
    placement(
        saved,
        settings,
        &screens,
        primary,
        cfg!(target_os = "macos"),
    )
}

/// 窗口能开在上面的一块屏幕。
struct Screen {
    id: DisplayId,
    uuid: Option<String>,
    bounds: Bounds<Pixels>,
    /// 去掉菜单栏、Dock、任务栏之后的部分。
    visible: Bounds<Pixels>,
}

/// 在给定的屏幕上算 [`window_placement`]。`macos`：最大化是一个填满屏幕可见部分的窗口，不是一种状态。
fn placement(
    saved: Option<&WindowState>,
    settings: WindowSettings,
    screens: &[Screen],
    primary: Option<DisplayId>,
    macos: bool,
) -> (WindowBounds, Option<DisplayId>) {
    let primary = screens
        .iter()
        .find(|screen| Some(screen.id) == primary)
        .or(screens.first());
    let saved = saved.filter(|_| settings.remember);

    // 它原来那块屏幕还在就用它，否则用主屏幕。
    let remembered = saved
        .and_then(|saved| saved.display.as_ref())
        .and_then(|uuid| {
            screens
                .iter()
                .find(|screen| screen.uuid.as_ref() == Some(uuid))
        });
    let screen = remembered.or(primary);
    // 从大屏幕上记下来的大小，缩到这块屏幕放得下。
    let size = saved.map_or(default_window_size(), |saved| saved.bounds.size);
    let size = match screen {
        Some(screen) => size.min(&screen.visible.size),
        None => size,
    };
    let bounds = match (saved, remembered) {
        (Some(saved), Some(screen))
            if reachable(Bounds::new(saved.bounds.origin, size), screen.bounds) =>
        {
            Bounds::new(saved.bounds.origin, size)
        }
        // 避开菜单栏、Dock 和任务栏，放在正中。
        _ => match screen {
            Some(screen) => Bounds::centered_at(screen.visible.center(), size),
            None => Bounds::new(point(px(0.), px(0.)), size),
        },
    };
    let display = remembered.map(|screen| screen.id);
    let maximized = match saved {
        Some(saved) => saved.maximized,
        // 屏幕放不下新窗口的大小（比如 150% 缩放的笔记本）：最大化，还原时是放得下的大小。
        None => screen.is_some_and(|screen| {
            default_window_size().width > screen.visible.size.width
                || default_window_size().height > screen.visible.size.height
        }),
    };
    if maximized {
        return match screen {
            Some(screen) if macos => (WindowBounds::Windowed(screen.visible), display),
            _ => (WindowBounds::Maximized(bounds), display),
        };
    }
    (WindowBounds::Windowed(bounds), display)
}

/// `bounds` 在屏幕上的部分够不够抓住：上边在屏幕里，还连着一截标题栏。
fn reachable(bounds: Bounds<Pixels>, screen: Bounds<Pixels>) -> bool {
    let overlap = bounds.intersect(&screen);
    bounds.top() >= screen.top()
        && bounds.top() <= screen.bottom() - px(VISIBLE_HEIGHT)
        && overlap.size.width >= px(VISIBLE_WIDTH)
        && overlap.size.height >= px(VISIBLE_HEIGHT)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Bounds, DisplayId, Pixels, WindowBounds, point, px, size};

    use super::{Screen, WindowState, default_window_size, placement};
    use crate::settings::WindowSettings;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    /// 一块笔记本屏幕（可见部分去掉了菜单栏和 Dock），右边一块更大的显示器。
    fn screens() -> Vec<Screen> {
        vec![
            Screen {
                id: DisplayId::new(1),
                uuid: Some("laptop".into()),
                bounds: rect(0., 0., 1512., 982.),
                visible: rect(0., 33., 1512., 890.),
            },
            Screen {
                id: DisplayId::new(2),
                uuid: Some("monitor".into()),
                bounds: rect(1512., 0., 2560., 1440.),
                visible: rect(1512., 0., 2560., 1440.),
            },
        ]
    }

    fn saved(display: &str, bounds: Bounds<Pixels>, maximized: bool) -> WindowState {
        WindowState {
            display: Some(display.into()),
            bounds,
            maximized,
        }
    }

    const REMEMBER: WindowSettings = WindowSettings { remember: true };
    const FORGET: WindowSettings = WindowSettings { remember: false };

    fn place(
        saved: Option<&WindowState>,
        settings: WindowSettings,
        macos: bool,
    ) -> (WindowBounds, Option<DisplayId>) {
        placement(saved, settings, &screens(), Some(DisplayId::new(1)), macos)
    }

    fn centered_on_laptop(size: gpui_kit::Size<Pixels>) -> WindowBounds {
        WindowBounds::Windowed(Bounds::centered_at(
            rect(0., 33., 1512., 890.).center(),
            size,
        ))
    }

    #[test]
    fn a_first_launch_opens_in_the_middle_of_the_main_screen() {
        let opened = place(None, REMEMBER, false);
        assert_eq!(opened, (centered_on_laptop(default_window_size()), None));
    }

    #[test]
    fn the_window_comes_back_where_it_was_on_the_screen_it_was_on() {
        let was = rect(1700., 120., 1600., 1000.);
        let state = saved("monitor", was, false);
        assert_eq!(
            place(Some(&state), REMEMBER, false),
            (WindowBounds::Windowed(was), Some(DisplayId::new(2)))
        );
        // 不记住：像新窗口一样。
        assert_eq!(
            place(Some(&state), FORGET, false),
            (centered_on_laptop(default_window_size()), None)
        );
    }

    #[test]
    fn a_window_whose_screen_is_gone_or_that_is_off_it_is_centered() {
        let state = saved("projector", rect(100., 100., 1000., 700.), false);
        assert_eq!(
            place(Some(&state), REMEMBER, false),
            (centered_on_laptop(size(px(1000.), px(700.))), None)
        );
        // 拖到了屏幕上边外面，或者几乎全在屏幕侧边外面。
        for was in [
            rect(100., -200., 1000., 700.),
            rect(1450., 100., 1000., 700.),
        ] {
            let state = saved("laptop", was, false);
            let (bounds, display) = place(Some(&state), REMEMBER, false);
            assert_eq!(bounds, centered_on_laptop(was.size));
            assert_eq!(display, Some(DisplayId::new(1)));
        }
    }

    #[test]
    fn a_maximized_window_opens_maximized_if_remembered() {
        let restored = rect(1700., 120., 1600., 1000.);
        let state = saved("monitor", restored, true);
        assert_eq!(
            place(Some(&state), REMEMBER, false),
            (WindowBounds::Maximized(restored), Some(DisplayId::new(2)))
        );
        // macOS 上是填满屏幕可见部分的窗口。
        let state = saved("laptop", rect(0., 33., 1512., 890.), true);
        assert_eq!(
            place(Some(&state), REMEMBER, true),
            (
                WindowBounds::Windowed(rect(0., 33., 1512., 890.)),
                Some(DisplayId::new(1))
            )
        );
        assert_eq!(
            place(Some(&state), FORGET, false),
            (centered_on_laptop(default_window_size()), None)
        );
    }

    #[test]
    fn a_new_window_too_large_for_the_screen_opens_maximized() {
        // 150% 缩放的 1920 × 1080 笔记本，可见部分去掉了任务栏。
        let small = vec![Screen {
            id: DisplayId::new(1),
            uuid: Some("laptop".into()),
            bounds: rect(0., 0., 1280., 720.),
            visible: rect(0., 0., 1280., 672.),
        }];
        let place = |saved: Option<&WindowState>, settings| {
            placement(saved, settings, &small, Some(DisplayId::new(1)), false)
        };
        let fitted = rect(0., 0., 1280., 672.);
        assert_eq!(
            place(None, REMEMBER),
            (WindowBounds::Maximized(fitted), None)
        );
        // 记住的大小缩到放得下，保持原来的（不最大化）。
        let state = saved("monitor", rect(1700., 120., 1600., 1000.), false);
        assert_eq!(
            place(Some(&state), REMEMBER),
            (WindowBounds::Windowed(fitted), None)
        );
    }

    #[test]
    fn a_minimized_window_keeps_the_place_it_had_before() {
        let restored = rect(1700., 120., 1600., 1000.);
        let monitor = || Some("monitor".to_string());
        // 最大化着：记下最大化和还原时的大小、位置。
        assert_eq!(
            WindowState::observe(monitor(), WindowBounds::Maximized(restored), true, false),
            Some(saved("monitor", restored, true))
        );
        // 再最小化：Windows 报的是还原时的大小、位置，`is_maximized` 是 false。不记，留着上一次的，
        // 这时从任务栏关掉，下次启动照样最大化。
        assert_eq!(
            WindowState::observe(monitor(), WindowBounds::Windowed(restored), false, true),
            None
        );
        assert_eq!(
            WindowState::observe(monitor(), WindowBounds::Windowed(restored), false, false),
            Some(saved("monitor", restored, false))
        );
    }

    #[test]
    fn the_state_is_kept_in_a_file_and_a_bad_one_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join("window.json");
        assert_eq!(WindowState::load(&path), None);
        let state = saved("monitor", rect(1700., 120., 1600., 1000.), true);
        state.save(&path).unwrap();
        assert_eq!(WindowState::load(&path), Some(state));
        std::fs::write(&path, "{\"bounds\": 7}").unwrap();
        assert_eq!(WindowState::load(&path), None);
    }
}
