//! 窗口壳的 action 处理器挂在 `Root` 的最外层，对话框里派发的 action 也到得了。
//!
//! gpui-kit 0.9 起，对话框、侧滑层、通知由 gpui-component 注册的 `RootPlugin` 画在 `Root` 里，
//! 和 `Workspace` 是兄弟节点，不再是窗口壳的子元素。焦点在对话框里时（预览、解锁框、问答框），
//! 快捷键和按钮派发的 action 从焦点往上冒泡，经过的只有对话框和 `Root`，到不了窗口壳的处理器：
//! 预览开着时按关闭标签没反应，问答框里的回答送不到连接。
//!
//! 这里注册一个插件，在 `decorate` 里把整个根（窗口壳内容和各层）包进一个带 `Workspace`
//! 键上下文和全部处理器的元素，冒泡路径又和 0.6 时一样经过窗口壳。根节点的内容不是
//! `Workspace` 的窗口（右键菜单的小窗口、测试里单独挂的视图）原样不动。

use gpui_kit::base::{Root, RootPlugin};
use gpui_kit::*;

use super::Workspace;

/// 只做 `decorate` 的插件，自己不画东西。
struct WorkspaceFrame;

impl Render for WorkspaceFrame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl RootPlugin for WorkspaceFrame {
    fn decorate(
        &self,
        surface: AnyElement,
        root: &Root,
        _: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        match root.view().clone().downcast::<Workspace>() {
            Ok(workspace) => {
                workspace.update(cx, |workspace, cx| workspace.render_frame(surface, cx))
            }
            Err(_) => surface,
        }
    }
}

/// 注册插件。在 `gpui_kit::init` 之后、开窗口之前调用：后注册的插件包在外面。
pub fn init(cx: &mut App) {
    Root::register_plugin::<WorkspaceFrame>(cx, |_, _| WorkspaceFrame);
}
