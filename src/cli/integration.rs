// 来自 since2006/shell-rs @ b2a307a: src/cli/integration.rs（Agent CLI 工单 05）
// 改动：安装、移除也经它做（在后台跑，跑完重看一遍状态），设置页的按钮只管调它。

use gpui_kit::*;

use super::install::{
    AgentKind, BinaryStatus, IntegrationPaths, SkillStatus, binary_status, install_binary,
    install_skill, remove_binary, remove_skill, skill_status, update_outdated_binary,
    update_outdated_skills,
};

/// What the settings page shows about the `micaterm` command and the agent
/// skills: where they go and whether they are there.
///
/// Without paths nothing can be installed. That is the state UI tests start
/// in, so no test writes to the real home directory by accident; they inject
/// a temporary one.
pub struct CliIntegration {
    paths: Option<IntegrationPaths>,
    /// `None` until the first check comes back.
    status: Option<IntegrationStatus>,
    /// 正在安装或移除：设置页的按钮先不让点。
    busy: bool,
    _check: Option<Task<()>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrationStatus {
    pub binary: BinaryStatus,
    skills: Vec<(AgentKind, SkillStatus)>,
}

impl IntegrationStatus {
    pub fn skill(&self, agent: AgentKind) -> SkillStatus {
        self.skills
            .iter()
            .find(|(kind, _)| *kind == agent)
            .map_or(SkillStatus::Missing, |(_, status)| *status)
    }

    /// 现在装成什么样。阻塞：看文件系统（几个文件，UI 线程上点按钮时读也不慢）。
    pub fn read(paths: &IntegrationPaths) -> Self {
        Self {
            binary: binary_status(paths),
            skills: AgentKind::ALL
                .iter()
                .map(|agent| (*agent, skill_status(paths, *agent)))
                .collect(),
        }
    }
}

/// 一次安装或移除要做的事。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrationChange {
    InstallBinary,
    RemoveBinary,
    InstallSkill(AgentKind),
    RemoveSkill(AgentKind),
}

impl CliIntegration {
    pub fn new(paths: Option<IntegrationPaths>) -> Self {
        Self {
            paths,
            status: None,
            busy: false,
            _check: None,
        }
    }

    pub fn paths(&self) -> Option<&IntegrationPaths> {
        self.paths.as_ref()
    }

    pub fn status(&self) -> Option<&IntegrationStatus> {
        self.status.as_ref()
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Look at the file system again, off the main thread.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.check(false, cx);
    }

    /// Bring a copy of the command left by an older build up to this one,
    /// and the installed agent skills with it, then look again. A copy that
    /// old answers every request with `version_mismatch`. Quietly: if it
    /// fails, the page offers 更新.
    pub fn update_outdated(&mut self, cx: &mut Context<Self>) {
        self.check(true, cx);
    }

    fn check(&mut self, update: bool, cx: &mut Context<Self>) {
        let Some(paths) = self.paths.clone() else {
            self._check = None;
            cx.notify();
            return;
        };
        self._check = Some(cx.spawn(async move |this, cx| {
            let status = cx
                .background_spawn(async move {
                    if update {
                        let _ = update_outdated_binary(&paths);
                        let _ = update_outdated_skills(&paths);
                    }
                    IntegrationStatus::read(&paths)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(status);
                cx.notify();
            });
        }));
    }

    /// 在后台做一次安装或移除，做完重看一遍状态；失败时交回原因。没有路径时什么也不做。
    pub fn apply(
        &mut self,
        change: IntegrationChange,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let Some(paths) = self.paths.clone() else {
            return Task::ready(Ok(()));
        };
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (result, status) = cx
                .background_spawn(async move {
                    let result = match change {
                        IntegrationChange::InstallBinary => install_binary(&paths),
                        IntegrationChange::RemoveBinary => remove_binary(&paths),
                        IntegrationChange::InstallSkill(agent) => install_skill(&paths, agent),
                        IntegrationChange::RemoveSkill(agent) => remove_skill(&paths, agent),
                    };
                    (result, IntegrationStatus::read(&paths))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.status = Some(status);
                cx.notify();
            });
            result.map_err(|error| error.to_string())
        })
    }
}
