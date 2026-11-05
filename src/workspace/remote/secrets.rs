//! 连接前取秘密的钩子：这次登录要用的秘密拿得到了，再连接（连接、重连都经过这里）。
//!
//! 登录要用的秘密（密码、私钥和口令、跳板和代理的密码，见 `vault::login_secrets`）在保险库里存着
//! 而保险库锁着时，先弹解锁小卡片，解开了或者用户取消了都接着连（取消了就现场询问密码，这次运行
//! 不再自动弹）；保险库是另一台电脑上建的、这里打不开时，问一次要不要重置（工单 13）。
//! 无密码和 Agent 登录不弹。连接经 `SecretStore`（`WorkspaceServices.secrets`，就是保险库）读秘密，
//! 读不出时连接现场询问。

use gpui_kit::*;

use crate::backend::RemoteTarget;
use crate::host::HostId;
use crate::vault::{asks_before_using, before_using_secrets, login_secrets};
use crate::workspace::Workspace;

impl Workspace {
    pub(in crate::workspace) fn with_login_secrets(
        &mut self,
        target: &RemoteTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let secrets = login_secrets(&target.login);
        // 不用弹框就直接连：同步回调会重入正在更新的窗口壳。
        if !asks_before_using(&secrets, cx) {
            then(self, window, cx);
            return;
        }
        let name = self
            .hosts
            .read(cx)
            .host(HostId(target.host))
            .map(|host| host.name.to_string())
            .unwrap_or_else(|| target.login.host.clone());
        let this = cx.entity().downgrade();
        before_using_secrets(&name, &secrets, window, cx, move |window, cx| {
            this.update(cx, |this, cx| then(this, window, cx)).ok();
        });
    }
}
