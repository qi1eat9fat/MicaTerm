//! 连接前取秘密的钩子：这次登录要用的秘密拿得到了，再连接。
//!
//! 现在（工单 12）直接继续：连接经 `SecretStore`（`WorkspaceServices.secrets`）读秘密，
//! 保险库锁着（`SecretsLocked`）或者读不出时当作没存，连接改成现场询问密码。
//!
//! 工单 13（保险库界面）在这里接解锁框：登录要用的秘密（`target.login.password`、私钥、口令）
//! 在保险库里存着而保险库锁着时，先弹解锁小卡片，解开了（或用户选择现场输入）再调 `then`；
//! 无密码和 Agent 登录不弹。`then` 可以存起来以后再调。

use gpui_kit::*;

use crate::backend::RemoteTarget;
use crate::workspace::Workspace;

impl Workspace {
    pub(in crate::workspace) fn with_login_secrets(
        &mut self,
        _target: &RemoteTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        then(self, window, cx);
    }
}
