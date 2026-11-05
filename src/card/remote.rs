// 参考 since2006/shell-rs @ b2a307a: src/terminal/terminal_panel.rs（远程面板的连接状态、
// `remote_menu_items`、主机快照）重写：外壳是 QiShell 的卡片，连接的事件由窗口壳转来。

//! 远程卡片的那一部分：连到哪台主机（打开时的快照，编辑或删除主机不影响已开的卡片）、
//! 连接的状态、延迟、探测出的系统和 `uname -n`、远程 shell 报告的目录。
//!
//! 扩展点：
//! - 工单 16（分屏复用连接）：`connection()` 是卡片所在的连接，分屏时据此复用；
//! - 工单 17（被动解析）：`RemoteActivity` 放解析出的程序、目录、标题，`badge` 之前先看它；
//!   `hostname()` 用来核对目录报告。

use gpui_kit::SharedString;
use qishell_hosts::{Host, HostOs, mark_color};
use qishell_terminal::{ReportedDirectory, SharedTerminalTransportFactory, TerminalLifecycle};

use crate::backend::{ConnectionId, Latency, ProbedHost, RemoteTarget, latency_label};
use crate::i18n::t;
use crate::programs::{Program, TERMINAL_GLYPH};

/// 新开一张远程卡片要的东西：窗口壳按主机建好。
pub struct RemoteLaunch {
    /// 打开时的主机：名称、地址、记下的系统。
    pub host: Host,
    /// 怎么连（登录在 UI 线程上解析好）。
    pub target: RemoteTarget,
    /// 远程终端的传输（`RemoteTerminals::factory_for`）。
    pub factory: SharedTerminalTransportFactory,
}

/// 窗口壳转给远程卡片的连接事件（`RemoteEvent` 里属于这张卡片的那些）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteUpdate {
    /// shell 通道开在了这条连接上。
    Attached(ConnectionId),
    Latency(Latency),
    Probed(ProbedHost),
}

/// 远程卡片被动解析出的东西（工单 17 填：程序、目录、标题）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteActivity {}

/// 远程卡片现在的连接状态，从终端的生命周期看出来。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteState {
    /// 在连接（可能在等用户回答主机密钥、密码）。
    Connecting,
    Connected,
    /// 断开了，带原因。
    Disconnected(SharedString),
}

pub(super) struct RemoteCard {
    host: Host,
    target: RemoteTarget,
    os: Option<HostOs>,
    hostname: Option<String>,
    latency: Option<Latency>,
    connection: Option<ConnectionId>,
    /// 远程 shell 报告的目录（OSC 7 等，核对过主机名）。
    directory: Option<String>,
    /// 用户点了「断开连接」：断开的原因就是它，不是 shell 退出。
    stopped: bool,
    pub(super) activity: RemoteActivity,
}

impl RemoteCard {
    pub(super) fn new(host: Host, target: RemoteTarget) -> Self {
        Self {
            os: host.os,
            host,
            target,
            hostname: None,
            latency: None,
            connection: None,
            directory: None,
            stopped: false,
            activity: RemoteActivity::default(),
        }
    }

    pub(super) fn host(&self) -> &Host {
        &self.host
    }

    pub(super) fn target(&self) -> &RemoteTarget {
        &self.target
    }

    pub(super) fn os(&self) -> Option<HostOs> {
        self.os
    }

    pub(super) fn hostname(&self) -> Option<&str> {
        self.hostname.as_deref()
    }

    pub(super) fn connection(&self) -> Option<ConnectionId> {
        self.connection
    }

    pub(super) fn latency(&self) -> Option<Latency> {
        self.latency
    }

    /// 重连：换成现在的主机和登录（主机删了就还是打开时的），忘掉上一条连接说的。
    pub(super) fn reconnecting(&mut self, host: Host, target: RemoteTarget) {
        if host.os.is_some() {
            self.os = host.os;
        }
        self.host = host;
        self.target = target;
        self.latency = None;
        self.connection = None;
        self.directory = None;
        self.stopped = false;
    }

    pub(super) fn stopped(&mut self) {
        self.stopped = true;
    }

    /// 收下连接的事件；返回探测出的系统（窗口壳记进主机数据），其余是 `None`。
    pub(super) fn apply(&mut self, update: RemoteUpdate) -> Option<HostOs> {
        match update {
            RemoteUpdate::Attached(connection) => self.connection = Some(connection),
            RemoteUpdate::Latency(latency) => self.latency = Some(latency),
            RemoteUpdate::Probed(probed) => {
                self.hostname = probed.hostname;
                if let Some(os) = HostOs::from_stored(probed.os) {
                    self.os = Some(os);
                    return Some(os);
                }
            }
        }
        None
    }

    /// 远程 shell 报告了目录：带主机名的要和 `uname -n` 对得上（在卡片里又 ssh 到别处的不算）。
    pub(super) fn report_directory(&mut self, directory: &ReportedDirectory) -> bool {
        if !directory.is_from(self.hostname.as_deref()) {
            return false;
        }
        self.directory = Some(directory.path.clone());
        true
    }

    pub(super) fn directory(&self) -> Option<&str> {
        self.directory.as_deref()
    }

    pub(super) fn state(&self, lifecycle: &TerminalLifecycle) -> RemoteState {
        match lifecycle {
            TerminalLifecycle::Starting => RemoteState::Connecting,
            TerminalLifecycle::Running => RemoteState::Connected,
            _ if self.stopped => RemoteState::Disconnected(t!("card.remote.stopped")),
            TerminalLifecycle::Failed(error) => RemoteState::Disconnected(error.clone().into()),
            TerminalLifecycle::Exited {
                signal: Some(signal),
                ..
            } => RemoteState::Disconnected(t!("card.remote.exited_signal", signal = signal)),
            TerminalLifecycle::Exited { code, .. } => {
                RemoteState::Disconnected(t!("card.remote.exited", code = code))
            }
            TerminalLifecycle::Closing => RemoteState::Disconnected(t!("card.remote.stopped")),
        }
    }

    /// 状态栏上「user@地址:端口」。
    pub(super) fn endpoint(&self) -> SharedString {
        let login = &self.target.login;
        format!("{}@{}:{}", login.user, login.host, login.port).into()
    }

    /// 状态栏上的延迟。
    pub(super) fn latency_label(&self) -> Option<SharedString> {
        self.latency.map(|latency| latency_label(latency).into())
    }

    /// 状态栏上的系统：还没探测出时写「未探测」。
    pub(super) fn os_label(&self) -> SharedString {
        match self.os {
            Some(os) => os.label().into(),
            None => t!("card.remote.unknown_os"),
        }
    }

    /// 拿不到程序名时卡片和标签上的图块：主机的系统徽章；还没探测出系统时是名称的首字，
    /// 底色按地址取（和主机栏的徽章一样）。
    pub(super) fn badge(&self) -> Program {
        let own = mark_color(&self.host.address);
        match self.os {
            Some(os) => {
                let (background, ink) = os
                    .brand_color()
                    .zip(os.brand_foreground())
                    .unwrap_or((own, 0xffffff));
                Program {
                    name: os.label().to_string(),
                    glyph: os.icon_path(),
                    tile_bg: background,
                    tile_fg: ink,
                    border: false,
                    shell: true,
                    letter: None,
                    off: false,
                }
            }
            None => {
                let letter = self
                    .host
                    .name
                    .chars()
                    .next()
                    .map(|first| first.to_uppercase().to_string())
                    .unwrap_or_default();
                Program {
                    name: letter.clone(),
                    glyph: TERMINAL_GLYPH,
                    tile_bg: own,
                    tile_fg: 0xffffff,
                    border: false,
                    shell: true,
                    letter: Some(letter),
                    off: false,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use qishell_hosts::{AuthKind, HostDraft, HostId};

    use super::*;
    use crate::backend::HostLogin;

    fn card(os: Option<HostOs>) -> RemoteCard {
        let mut host = Host::new(
            HostId(3),
            HostDraft::new("web-01", "10.0.1.21", 22, "ubuntu", AuthKind::Password),
        );
        host.os = os;
        let login = HostLogin::new(
            "10.0.1.21",
            22,
            "ubuntu",
            qishell_ssh::LoginMethod::Password,
        );
        RemoteCard::new(host, RemoteTarget { host: 3, login })
    }

    #[test]
    fn without_a_system_the_badge_is_the_names_first_character() {
        let badge = card(None).badge();
        assert_eq!(badge.letter.as_deref(), Some("W"));
        assert_eq!(badge.name, "W");
    }

    #[test]
    fn a_probed_system_becomes_the_badge() {
        let mut card = card(None);
        let os = card.apply(RemoteUpdate::Probed(ProbedHost {
            os: "ubuntu",
            hostname: Some("web-01".into()),
        }));
        assert_eq!(os, Some(HostOs::Ubuntu));
        assert_eq!(card.badge().name, "Ubuntu");
        assert_eq!(card.badge().letter, None);
        assert_eq!(card.os_label().as_ref(), "Ubuntu");
    }

    #[test]
    fn a_directory_report_naming_another_host_is_ignored() {
        let mut card = card(None);
        card.apply(RemoteUpdate::Probed(ProbedHost {
            os: "ubuntu",
            hostname: Some("web-01".into()),
        }));
        let elsewhere = ReportedDirectory {
            host: Some("db-01".into()),
            path: "/srv".into(),
        };
        assert!(!card.report_directory(&elsewhere));
        let here = ReportedDirectory {
            host: Some("web-01".into()),
            path: "/var/www".into(),
        };
        assert!(card.report_directory(&here));
        assert_eq!(card.directory(), Some("/var/www"));
    }

    #[test]
    fn the_state_follows_the_terminal_and_says_why_it_ended() {
        let mut card = card(None);
        assert_eq!(
            card.state(&TerminalLifecycle::Starting),
            RemoteState::Connecting
        );
        assert_eq!(
            card.state(&TerminalLifecycle::Running),
            RemoteState::Connected
        );
        assert_eq!(
            card.state(&TerminalLifecycle::Failed("连接已断开".into())),
            RemoteState::Disconnected("连接已断开".into())
        );
        assert_eq!(
            card.state(&TerminalLifecycle::Exited {
                code: 3,
                signal: None
            }),
            RemoteState::Disconnected("shell 已退出（退出码 3）".into())
        );
        card.stopped();
        assert_eq!(
            card.state(&TerminalLifecycle::Exited {
                code: 0,
                signal: None
            }),
            RemoteState::Disconnected("已断开连接".into())
        );
        assert_eq!(card.endpoint().as_ref(), "ubuntu@10.0.1.21:22");
    }
}
