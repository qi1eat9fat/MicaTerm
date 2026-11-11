// 参考 since2006/shell-rs @ b2a307a: src/terminal/terminal_panel.rs（远程面板的连接状态、
// `remote_menu_items`、主机快照）重写：外壳是 QiShell 的卡片，连接的事件由窗口壳转来。

//! 远程卡片的那一部分：连到哪台主机（打开时的快照，编辑或删除主机不影响已开的卡片）、
//! 连接的状态、延迟、探测出的系统和 `uname -n`、远程 shell 报告的目录。
//!
//! 扩展点：
//! - 工单 16（分屏复用连接）：`connection()` 是卡片所在的连接，分屏时据此复用；
//! - 工单 17（被动解析）：终端 crate 的 `ShellActivity` 把远程报告的目录、命令、窗口标题和
//!   全屏状态合起来（优先级、主机名核对都在那里），结果放进 `RemoteActivity`；拿不到程序名时
//!   图块用 `badge`。SFTP 标签和分屏回到原目录用 `RemoteActivity::start_directory`。

use gpui_kit::SharedString;
use qishell_hosts::{Host, HostOs, mark_color};
use qishell_terminal::{
    ReportedDirectory, SharedTerminalTransportFactory, ShellActivity, ShellCommand,
    TerminalLifecycle,
};

use crate::backend::{ConnectionId, Latency, ProbedHost, RemoteTarget, latency_label};
use crate::i18n::t;
use crate::programs::{Program, TERMINAL_GLYPH, program_of};

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

/// 远程卡片被动解析出的东西（工单 17）：只读远程自己发出的信息，不往远程写，也不另开通道。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteActivity {
    /// 正在跑的程序的命令名（可能是个 shell）；拿不到名字时是 `None`。
    pub program: Option<String>,
    /// 有程序在跑：知道名字的、shell 集成说在跑的，或者在全屏里的。
    pub running: bool,
    /// 显示用的目录：远程报告的（OSC 7 等）优先，其次是窗口标题里的。
    pub directory: Option<String>,
    /// 能用作 SFTP 标签和分屏初始目录的：报告的；标题里的只有 `/` 或 `~/` 开头才算。
    pub start_directory: Option<String>,
    /// 现在的窗口标题：悬停卡片头部时显示完整的。
    pub title: Option<String>,
}

impl RemoteActivity {
    fn of(shell: &ShellActivity) -> Self {
        Self {
            program: shell.program().map(str::to_string),
            running: shell.running(),
            directory: shell.directory().map(str::to_string),
            start_directory: shell.start_directory().map(str::to_string),
            title: shell.title().map(str::to_string),
        }
    }
}

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
    /// 被动解析的状态（终端 crate 的纯状态，一条连接一份）。
    shell: ShellActivity,
    /// 用户点了「断开连接」：断开的原因就是它，不是 shell 退出。
    stopped: bool,
    /// 上次从 `shell` 取出的结果：变了才告诉窗口壳。
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
            shell: ShellActivity::default(),
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
        self.shell = ShellActivity::default();
        self.activity = RemoteActivity::default();
        self.stopped = false;
    }

    /// 这台主机不再探测系统（工单 34）：探测把连接弄断了。主机删了时重连按这里记着的来。
    pub(super) fn stop_probing(&mut self) {
        self.host.probe_os = false;
        self.target.probe_os = false;
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
    /// 返回解析结果变没变。
    pub(super) fn report_directory(&mut self, directory: &ReportedDirectory) -> bool {
        self.shell
            .report_directory(directory, self.hostname.as_deref());
        self.refresh()
    }

    /// shell 集成说命令开始跑了、回到提示符了。返回解析结果变没变。
    pub(super) fn shell_command(&mut self, command: ShellCommand) -> bool {
        self.shell.shell_command(command);
        self.refresh()
    }

    /// 终端重画了：看看窗口标题和全屏状态。返回解析结果变没变。
    pub(super) fn observe_screen(&mut self, title: Option<&str>, full_screen: bool) -> bool {
        if self.shell.title() != title {
            self.shell.set_title(title, self.hostname.as_deref());
        }
        self.shell.set_full_screen(full_screen);
        self.refresh()
    }

    /// 探测出了 `uname -n`：终端记着的最后一次目录报告和现在的标题按它重新核对（之前带主机名的
    /// 报告一律没信）。返回解析结果变没变。
    pub(super) fn recheck(
        &mut self,
        directory: Option<&ReportedDirectory>,
        title: Option<&str>,
    ) -> bool {
        let hostname = self.hostname.as_deref();
        if let Some(directory) = directory {
            self.shell.report_directory(directory, hostname);
        }
        self.shell.set_title(title, hostname);
        self.refresh()
    }

    fn refresh(&mut self) -> bool {
        let activity = RemoteActivity::of(&self.shell);
        let changed = activity != self.activity;
        self.activity = activity;
        changed
    }

    pub(super) fn directory(&self) -> Option<&str> {
        self.activity.directory.as_deref()
    }

    /// 在跑的程序的图块：知道名字的查程序名表（是 shell 的不算，比如在卡片里又开了 `bash`）；
    /// 只知道在跑时是系统徽章（图块不变，有输出时照常亮绿点）。在提示符下是 `None`。
    pub(super) fn running_program(&self) -> Option<Program> {
        match &self.activity.program {
            Some(name) => Some(program_of(name)).filter(|program| !program.shell),
            None if self.activity.running => Some(Program {
                shell: false,
                ..self.badge()
            }),
            None => None,
        }
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
        host_badge(&self.host, self.os)
    }
}

/// 主机的系统徽章：`os` 不知道时是名称的首字，底色按地址取（和主机栏的徽章一样）。
/// 远程卡片和 SFTP 标签的远程文件卡片共用。
pub fn host_badge(host: &Host, os: Option<HostOs>) -> Program {
    let own = mark_color(&host.address);
    match os {
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
            let letter = host
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
        RemoteCard::new(host, RemoteTarget::new(3, login))
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
    fn a_named_program_replaces_the_badge_and_an_unnamed_one_keeps_it() {
        let mut card = card(Some(HostOs::Debian));
        assert_eq!(card.running_program(), None);
        assert!(card.shell_command(ShellCommand::Running(Some("htop".into()))));
        assert_eq!(card.running_program(), Some(program_of("htop")));
        // 卡片里又开了个 shell：在提示符下一样，用系统徽章。
        card.shell_command(ShellCommand::Running(Some("bash".into())));
        assert_eq!(card.running_program(), None);
        // 只知道在跑：图块还是系统徽章，但算在跑（有输出时亮绿点）。
        card.shell_command(ShellCommand::Prompt);
        card.shell_command(ShellCommand::Running(None));
        let running = card.running_program().unwrap();
        assert_eq!(running.name, card.badge().name);
        assert!(!running.shell);
        assert!(card.shell_command(ShellCommand::Prompt));
        assert_eq!(card.running_program(), None);
        // 没变的不算变。
        assert!(!card.shell_command(ShellCommand::Prompt));
    }

    #[test]
    fn the_hostname_probed_later_rechecks_what_was_reported_before() {
        let mut card = card(None);
        let report = ReportedDirectory {
            host: Some("web-01".into()),
            path: "/srv".into(),
        };
        assert!(!card.report_directory(&report));
        assert!(card.observe_screen(Some("root@web-01: ~/x"), false));
        assert_eq!(card.directory(), None);
        card.apply(RemoteUpdate::Probed(ProbedHost {
            os: "ubuntu",
            hostname: Some("web-01".into()),
        }));
        assert!(card.recheck(Some(&report), Some("root@web-01: ~/x")));
        assert_eq!(card.directory(), Some("/srv"));
        assert_eq!(card.activity.start_directory.as_deref(), Some("/srv"));
        assert_eq!(card.activity.title.as_deref(), Some("root@web-01: ~/x"));
        // 重连：上一条连接说的都忘掉。
        let (host, target) = (card.host().clone(), card.target().clone());
        card.reconnecting(host, target);
        assert_eq!(card.activity, RemoteActivity::default());
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
