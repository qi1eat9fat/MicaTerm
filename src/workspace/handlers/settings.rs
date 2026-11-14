//! 设置的处理器：打开设置标签（UI 重做工单 12），标题栏的深浅切换。

use gpui_kit::*;

use gpui_kit::component::{ActiveTheme as _, ThemeMode};

use crate::app::{OpenSettings, ToggleAppearance};
use crate::settings::{Appearance, SettingsPaths, SettingsTab};
use crate::tabs::Tab;
use crate::workspace::Workspace;

pub(in crate::workspace) fn bind_settings_actions(
    root: Stateful<Div>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    root.on_action(cx.listener(Workspace::on_open_settings))
        .on_action(cx.listener(Workspace::on_toggle_appearance))
}

/// 深浅切换之后「应用外观」该是什么：和现在显示的相反，固定下来。
fn toggled_appearance(now: ThemeMode) -> Appearance {
    match now {
        ThemeMode::Light => Appearance::Dark,
        ThemeMode::Dark => Appearance::Light,
    }
}

impl Workspace {
    /// 标题栏的深浅切换：改成和现在相反的固定外观，写进设置（设置的观察者负责换主题）。
    fn on_toggle_appearance(
        &mut self,
        _: &ToggleAppearance,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let appearance = toggled_appearance(cx.theme().mode);
        self.settings.update(cx, |store, cx| {
            store.update(|settings| settings.appearance = appearance, cx)
        });
    }

    /// 打开设置标签（Ctrl+, 或标题栏的齿轮）：开着就切过去。
    fn on_open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.find(|tab| tab.as_settings().is_some()) {
            self.activate_tab(tab, window, cx);
            return;
        }
        let paths = SettingsPaths {
            downloads: self.services.local_files.downloads(),
            data_dir: self.services.data_dir.clone(),
        };
        let store = self.settings.clone();
        let settings = cx.new(|cx| SettingsTab::new(store, paths, window, cx));
        self.open_tab(Tab::Settings(settings), window, cx);
    }

    /// 开着的设置标签，测试用。
    pub fn settings_tab(&self) -> Option<Entity<SettingsTab>> {
        self.tabs
            .iter()
            .find_map(|(_, tab)| tab.as_settings().cloned())
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::ThemeMode;

    use super::toggled_appearance;
    use crate::settings::Appearance;

    #[test]
    fn the_toggle_fixes_the_opposite_appearance() {
        assert_eq!(toggled_appearance(ThemeMode::Light), Appearance::Dark);
        assert_eq!(toggled_appearance(ThemeMode::Dark), Appearance::Light);
    }
}
