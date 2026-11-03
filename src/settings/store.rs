//! `settings.json` 的读写。
// 来自 since2006/shell-rs @ b2a307a: src/settings/store.rs（测试按 QiShell 的字段改写）

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use gpui_kit::{Context, EventEmitter, SharedString};

use super::AppSettings;
use crate::i18n::t;

#[derive(Clone, Debug)]
pub enum SettingsStoreEvent {
    /// 改动留在了内存里，但写不进文件。
    PersistFailed(SharedString),
}

/// 设置文件为什么没在用。窗口打开后告诉用户一次。
#[derive(Clone, Debug)]
pub enum SettingsProblem {
    /// 文件读不了：本次运行用默认设置。
    Unreadable(String),
    /// 没有数据目录可放文件：改动只在本次运行有效。
    Unsaved(String),
}

impl SettingsProblem {
    pub fn message(&self) -> SharedString {
        match self {
            Self::Unreadable(error) => t!("settings.store.unreadable", error = error),
            Self::Unsaved(error) => t!("settings.store.unsaved", error = error),
        }
    }
}

/// 设置，设置对话框和应用设置的窗口壳共用。内存为准，每次改动立刻写穿到磁盘；
/// 写失败时保留改动，并发出 `SettingsStoreEvent::PersistFailed`。
pub struct SettingsStore {
    settings: AppSettings,
    /// `None` 表示只放在内存里。
    path: Option<PathBuf>,
}

impl EventEmitter<SettingsStoreEvent> for SettingsStore {}

impl SettingsStore {
    /// 不写到任何地方的设置：给 UI 测试用，也给没有数据目录的机器用。
    pub fn in_memory() -> Self {
        Self {
            settings: AppSettings::default(),
            path: None,
        }
    }

    /// 读 `path` 上保存的设置。文件不存在就是默认设置；文件读不了也给默认设置，
    /// 同时返回要告诉用户的问题，文件原样留着，直到下一次改动把它换掉。
    pub fn load(path: PathBuf) -> (Self, Option<SettingsProblem>) {
        let loaded = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppSettings::default()),
            Err(error) => Err(error.to_string()),
        };
        let (settings, problem) = match loaded {
            Ok(settings) => (AppSettings::normalized(settings), None),
            Err(error) => (
                AppSettings::default(),
                Some(SettingsProblem::Unreadable(error)),
            ),
        };
        (
            Self {
                settings,
                path: Some(path),
            },
            problem,
        )
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.clone()
    }

    /// 改设置、写文件、通知观察者；写失败时发出 `PersistFailed`。
    pub fn update(&mut self, change: impl FnOnce(&mut AppSettings), cx: &mut Context<Self>) {
        let Some(written) = self.update_unnotified(change) else {
            return;
        };
        cx.notify();
        if let Err(error) = written {
            cx.emit(SettingsStoreEvent::PersistFailed(t!(
                "settings.store.save_failed",
                error = error
            )));
        }
    }

    /// `update` 的内存那一半：什么都没变时返回 `None`，否则返回写文件的结果。
    /// 不管写没写成，内存都保留改动。
    pub fn update_unnotified(
        &mut self,
        change: impl FnOnce(&mut AppSettings),
    ) -> Option<io::Result<()>> {
        let before = self.settings.clone();
        change(&mut self.settings);
        self.settings = self.settings.clone().normalized();
        if self.settings == before {
            return None;
        }
        Some(match &self.path {
            Some(path) => write(path, &self.settings),
            None => Ok(()),
        })
    }
}

/// 一步换掉整个文件：先写临时文件再改名，中途崩溃也不会留下半个文件。
fn write(path: &Path, settings: &AppSettings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(settings).map_err(io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, json)?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    // 不写 `super::*`：它会带进 gpui_kit 的 `test`，遮蔽 `#[test]`。
    use std::fs;

    use super::{SettingsProblem, SettingsStore};
    use crate::settings::{AppSettings, Appearance, RightClick};

    #[test]
    fn a_first_start_has_the_defaults_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let (mut store, problem) = SettingsStore::load(path.clone());
        assert!(problem.is_none());
        assert_eq!(store.settings(), AppSettings::default());

        // 选已经选着的值不算改动。
        let default = AppSettings::default().appearance;
        assert!(
            store
                .update_unnotified(|settings| settings.appearance = default)
                .is_none()
        );
        assert!(!path.exists());
    }

    #[test]
    fn a_change_is_written_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        // 数据目录可能还不存在。
        let path = dir.path().join("data").join("settings.json");
        let (mut store, _) = SettingsStore::load(path.clone());
        let written = store.update_unnotified(|settings| {
            settings.appearance = Appearance::Dark;
            settings.terminal_font.size = 14.;
            settings.terminal_interaction.right_click = RightClick::CopyOrPaste;
            settings.local_terminal.shell = Some("cmd.exe".into());
        });
        assert!(matches!(written, Some(Ok(()))));

        let (reloaded, problem) = SettingsStore::load(path);
        assert!(problem.is_none());
        assert_eq!(reloaded.settings().appearance, Appearance::Dark);
        assert_eq!(reloaded.settings().terminal_font.size, 14.);
        assert_eq!(
            reloaded.settings().terminal_interaction.right_click,
            RightClick::CopyOrPaste
        );
        assert_eq!(
            reloaded.settings().local_terminal.shell.as_deref(),
            Some("cmd.exe")
        );
    }

    #[test]
    fn an_unreadable_file_falls_back_to_the_defaults_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        let (store, problem) = SettingsStore::load(path.clone());
        assert_eq!(store.settings(), AppSettings::default());
        assert!(matches!(problem, Some(SettingsProblem::Unreadable(_))));
        assert!(problem.unwrap().message().starts_with("无法读取设置"));
        // 用户改设置之前，文件原样留着。
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[test]
    fn a_failed_write_keeps_the_change_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        // 文件的位置上是一个目录，文件换不掉它。
        let path = dir.path().join("settings.json");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep"), "").unwrap();
        let (mut store, _) = SettingsStore::load(path);
        let written = store.update_unnotified(|settings| settings.appearance = Appearance::Light);
        assert!(matches!(written, Some(Err(_))));
        assert_eq!(store.settings().appearance, Appearance::Light);
    }

    #[test]
    fn in_memory_settings_touch_no_file() {
        let mut store = SettingsStore::in_memory();
        let written = store.update_unnotified(|settings| settings.appearance = Appearance::Dark);
        assert!(matches!(written, Some(Ok(()))));
        assert_eq!(store.settings().appearance, Appearance::Dark);
    }
}
