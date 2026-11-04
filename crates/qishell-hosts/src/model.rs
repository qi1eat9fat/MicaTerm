// 来自 since2006/shell-rs @ b2a307a: crates/shellrs-hosts/src/model.rs
// 裁剪：去掉分组（`GroupId`、`HostGroup`、`GroupDraft`、分组路径）、协议字段（Telnet、串口）、
// `Host.public_id` 和「复制 ID」、右侧栏工具字段、SFTP 书签、同步用的 `edited_ms`；
// 主机列表平铺，只按 `sort_order` 排。`PublicId` 改名 `SecretId`，只剩凭据在保险库里的条目名用它。
// `ProxyKind` 和上游一样用 qishell-ssh 的。

use gpui_shared_string::SharedString;
use rand::{Rng as _, distr::Alphanumeric};

use crate::secrets::SecretRef;

use super::CredentialId;

/// Stable identity of a host. Never reused within a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostId(pub u64);

/// The random name a credential's secrets go by in the vault, such as
/// `Jwg5rHvXCxw89paM`. Random letters and digits, not derived from
/// [`CredentialId`]: that one is an allocator detail that can come back after
/// a restart, while this one is never reused, so a new credential never finds
/// the secrets an old one left behind. It names a credential; it grants
/// nothing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SecretId(SharedString);

impl SecretId {
    const LEN: usize = 16;

    /// A new id: 16 characters from 62, about 95 random bits.
    pub fn generate() -> Self {
        let id: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(Self::LEN)
            .map(char::from)
            .collect();
        Self(id.into())
    }

    /// A new id that `in_use` does not already claim. Sixteen random
    /// characters practically never collide, but ids must be unique.
    pub fn generate_unused(in_use: impl Fn(&SecretId) -> bool) -> Self {
        loop {
            let id = Self::generate();
            if !in_use(&id) {
                return id;
            }
        }
    }

    /// Whether `text` looks like an id: sixteen ASCII letters and digits.
    pub fn has_shape(text: &str) -> bool {
        text.len() == Self::LEN && text.bytes().all(|byte| byte.is_ascii_alphanumeric())
    }

    /// An id read back from the database.
    pub(crate) fn from_stored(id: String) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SecretId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// How a host logs in on its own, without a credential. A key, or a
/// password shared by several hosts, is a credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AuthKind {
    /// A password typed into the host form, or asked for at each connection
    /// when none was saved.
    #[default]
    Password,
    /// Nothing typed: the server lets the user in as they are, or the SSH
    /// agent or a default key in `~/.ssh` does. A server that wants a
    /// password is refused rather than asked.
    NoPassword,
}

impl AuthKind {
    /// The stored spelling. Kept separate from any label so translating the
    /// UI cannot rewrite what is already in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            AuthKind::Password => "password",
            AuthKind::NoPassword => "no-password",
        }
    }

    /// Parse a stored spelling. Anything else is the default: `credential`,
    /// which a host using a credential stores, keeps the default here.
    pub fn from_stored(value: &str) -> Self {
        match value {
            "no-password" => AuthKind::NoPassword,
            _ => AuthKind::Password,
        }
    }
}

/// How a connection reaches a host, as the host form's 「连接方式」 offers it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Route {
    /// A TCP connection from this machine.
    #[default]
    Direct,
    /// Through other saved hosts, one after another: this machine logs in to
    /// the first, which opens a channel to the second, and so on to the
    /// host. Only this list counts; a jump host's own route does not.
    ///
    /// `None` is a jump host that has been deleted. It stays in its place
    /// so that the host fails to connect, saying why, rather than quietly
    /// skipping a hop or going direct.
    Jump(Vec<Option<HostId>>),
    /// Through an HTTP or SOCKS5 proxy.
    Proxy(ProxySettings),
}

impl Route {
    /// The jump hosts this route goes through, deleted ones left out.
    pub fn jump_hosts(&self) -> impl Iterator<Item = HostId> + '_ {
        let hops = match self {
            Route::Jump(hops) => hops.as_slice(),
            _ => &[],
        };
        hops.iter().flatten().copied()
    }
}

/// The proxy of a host that connects through one.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProxySettings {
    pub kind: ProxyKind,
    pub host: SharedString,
    pub port: u16,
    /// For a proxy that wants a user name and password; its password is in
    /// the vault.
    pub user: Option<SharedString>,
}

impl ProxySettings {
    pub fn new(kind: ProxyKind, host: impl Into<SharedString>, port: u16) -> Self {
        Self {
            kind,
            host: host.into(),
            port,
            user: None,
        }
    }

    /// Log in to the proxy as `user`. An empty name is no name.
    pub fn with_user(mut self, user: impl Into<SharedString>) -> Self {
        let user = user.into();
        self.user = (!user.is_empty()).then_some(user);
        self
    }

    /// Where the proxy's password lives in the vault. Only a proxy with
    /// a user name has one; hosts behind the same proxy as the same user
    /// share it.
    pub fn password_secret(&self) -> Option<SecretRef> {
        self.user
            .as_ref()
            .map(|user| SecretRef::proxy(user.as_ref(), self.host.as_ref(), self.port))
    }
}

/// The kind of proxy a host connects through: the SSH core's, whose stored
/// spelling the database keeps.
pub use qishell_ssh::ProxyKind;

/// The operating system running on a host, as reported by the host's own
/// shell after it connects.
///
/// Each variant owns four things that must stay in step: the spelling kept in
/// the database, the name shown to a person, the icon embedded in
/// `app/assets.rs`, and the brand's own colour. Adding a distribution means
/// one line here, one icon file, and one arm in `ssh/probe.rs`.
///
/// The colours are the published brand values, which is why they are literals
/// rather than theme tokens: here the colour *is* the data, the one case the
/// design guides allow. Keeping them in this table is what stops them from
/// leaking into render code.
macro_rules! host_os {
    ($($variant:ident => $stored:literal, $label:literal, $icon:literal, $brand:expr;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum HostOs {
            $($variant,)*
        }

        impl HostOs {
            /// Every variant, for tests that must cover the whole table.
            pub const ALL: &'static [HostOs] = &[$(HostOs::$variant,)*];

            /// How the database spells it. Kept separate from `label` so
            /// translating the UI cannot rewrite what is already stored.
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $stored,)* }
            }

            /// What a person sees, in a tooltip or the status bar.
            pub fn label(self) -> &'static str {
                match self { $(Self::$variant => $label,)* }
            }

            /// Path into the application asset bundle.
            pub fn icon_path(self) -> &'static str {
                match self { $(Self::$variant => concat!("icons/os/", $icon, ".svg"),)* }
            }

            /// The brand's published colour as `0xRRGGBB`, for the badge
            /// behind the mark.
            ///
            /// `None` means the mark is monochrome and should follow the
            /// theme instead. That is how Apple's is meant to be drawn, and
            /// the honest answer where a project publishes no single colour.
            pub fn brand_color(self) -> Option<u32> {
                match self { $(Self::$variant => $brand,)* }
            }

            /// What to draw the mark itself in, on top of [`Self::brand_color`].
            ///
            /// White, the way nearly every brand draws its own mark, except on
            /// the light ones where the brand uses a dark mark instead.
            pub fn brand_foreground(self) -> Option<u32> {
                self.brand_color().map(|color| {
                    if is_light(color) {
                        BRAND_DARK_MARK
                    } else {
                        BRAND_LIGHT_MARK
                    }
                })
            }

            /// Read back a stored spelling. An unknown one means the row was
            /// written by a newer build, so it is treated as undetected.
            pub fn from_stored(value: &str) -> Option<Self> {
                match value {
                    $($stored => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

host_os! {
    Ubuntu   => "ubuntu",    "Ubuntu",           "ubuntu",      Some(0xE9_5420);
    Debian   => "debian",    "Debian",           "debian",      Some(0xA8_1D33);
    Fedora   => "fedora",    "Fedora",           "fedora",      Some(0x51_A2DA);
    RedHat   => "rhel",      "Red Hat",          "redhat",      Some(0xEE_0000);
    CentOs   => "centos",    "CentOS",           "centos",      Some(0x26_2577);
    Rocky    => "rocky",     "Rocky Linux",      "rockylinux",  Some(0x10_B981);
    // AlmaLinux publishes no single flat colour, so its mark follows the theme.
    Alma     => "almalinux", "AlmaLinux",        "almalinux",   None;
    Arch     => "arch",      "Arch Linux",       "archlinux",   Some(0x17_93D1);
    Alpine   => "alpine",    "Alpine Linux",     "alpinelinux", Some(0x0D_597F);
    Suse     => "suse",      "openSUSE",         "opensuse",    Some(0x73_BA25);
    Gentoo   => "gentoo",    "Gentoo",           "gentoo",      Some(0x54_487A);
    Kali     => "kali",      "Kali Linux",       "kalilinux",   Some(0x55_7C94);
    Manjaro  => "manjaro",   "Manjaro",          "manjaro",     Some(0x35_BFA4);
    Raspbian => "raspbian",  "Raspberry Pi OS",  "raspberrypi", Some(0xA2_2846);
    Linux    => "linux",     "Linux",            "linux",       Some(0xFC_C624);
    MacOs    => "macos",     "macOS",            "apple",       None;
    Windows  => "windows",   "Windows",          "windows",     Some(0x00_78D4);
    FreeBsd  => "freebsd",   "FreeBSD",          "freebsd",     Some(0xAB_2B28);
    OpenBsd  => "openbsd",   "OpenBSD",          "openbsd",     Some(0xF2_CA30);
    NetBsd   => "netbsd",    "NetBSD",           "netbsd",      Some(0xFF_6600);
}

impl HostOs {
    /// Whether the host runs Linux, any distribution of it: what the system
    /// monitor can read.
    pub fn is_linux(self) -> bool {
        !matches!(
            self,
            HostOs::MacOs | HostOs::Windows | HostOs::FreeBsd | HostOs::OpenBsd | HostOs::NetBsd
        )
    }
}

/// Drawn on top of a light brand colour.
const BRAND_DARK_MARK: u32 = 0x00_0000;
/// Drawn on top of every other brand colour.
const BRAND_LIGHT_MARK: u32 = 0xFF_FFFF;

/// Whether a brand colour is light enough that the brand puts a dark mark on
/// it. The threshold sits well above the point where white text would fail,
/// because a logo silhouette is not body text and nearly every brand draws its
/// own mark in white; only the yellows go the other way.
fn is_light(color: u32) -> bool {
    super::mark::luminance(color) > 0.5
}

/// A saved SSH host. The host list is flat: hosts are listed by
/// `sort_order`, which dragging rewrites.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Host {
    pub id: HostId,
    pub name: SharedString,
    /// The host name or IP address it is reached at: 「地址」 in the form.
    pub address: SharedString,
    pub port: u16,
    /// The user it logs in as. For a host using a credential this is the
    /// credential's user, which the store keeps in step.
    pub user: SharedString,
    /// How a host logs in on its own. A host using a credential keeps
    /// the default here.
    pub auth: AuthKind,
    /// The credential it logs in with instead of `auth`, if any.
    pub credential: Option<CredentialId>,
    /// How a connection reaches it.
    pub route: Route,
    /// Whatever the user wants to remember about it; empty for none.
    pub notes: SharedString,
    /// 在平铺主机列表里的位置，从小到大。
    pub sort_order: i64,
    /// Detected on every successful connection and persisted, so the list can
    /// show the right mark before anyone connects. `None` until a probe
    /// succeeds, and the host list falls back to the name's first letter.
    pub os: Option<HostOs>,
}

impl Host {
    pub fn new(id: HostId, draft: HostDraft) -> Self {
        Self {
            id,
            name: draft.name,
            address: draft.address,
            port: draft.port,
            user: draft.user,
            auth: draft.auth,
            credential: draft.credential,
            route: draft.route,
            notes: draft.notes,
            sort_order: 0,
            os: None,
        }
    }

    /// `user@address:port`, as shown in the status bar.
    pub fn endpoint(&self) -> String {
        format!("{}@{}:{}", self.user, self.address, self.port)
    }

    /// Whether the address is a literal IP address rather than a name, so a
    /// command can say which of the two it copies (复制 IP / 复制主机名).
    pub fn address_is_ip(&self) -> bool {
        is_ip_address(&self.address)
    }

    /// Where this host's login password lives in the vault: by the host's
    /// id, so renaming the host or changing its address keeps the password,
    /// and a copy (a new host) starts without one.
    pub fn password_secret(&self) -> SecretRef {
        SecretRef::password(self.id.0)
    }

    /// The editable fields, for pre-filling the host form.
    pub fn draft(&self) -> HostDraft {
        HostDraft {
            name: self.name.clone(),
            address: self.address.clone(),
            port: self.port,
            user: self.user.clone(),
            auth: self.auth,
            credential: self.credential,
            route: self.route.clone(),
            notes: self.notes.clone(),
        }
    }
}

/// IPv6 literals are accepted with or without the brackets a URL puts around
/// them.
fn is_ip_address(host: &str) -> bool {
    let host = host.trim();
    let bare = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    bare.parse::<std::net::IpAddr>().is_ok()
}

/// 拖动主机时放下的位置：平铺列表里某台主机的前面或后面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostDrop {
    Before(HostId),
    After(HostId),
}

/// The values the host form commits.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct HostDraft {
    pub name: SharedString,
    pub address: SharedString,
    pub port: u16,
    pub user: SharedString,
    pub auth: AuthKind,
    pub credential: Option<CredentialId>,
    pub route: Route,
    pub notes: SharedString,
}

impl HostDraft {
    pub fn new(
        name: impl Into<SharedString>,
        address: impl Into<SharedString>,
        port: u16,
        user: impl Into<SharedString>,
        auth: AuthKind,
    ) -> Self {
        Self {
            name: name.into(),
            address: address.into(),
            port,
            user: user.into(),
            auth,
            credential: None,
            route: Route::Direct,
            notes: SharedString::default(),
        }
    }

    /// Log in with a saved credential instead of `auth`. The store takes the
    /// user name from the credential when the draft is saved.
    pub fn with_credential(mut self, credential: CredentialId) -> Self {
        self.credential = Some(credential);
        self
    }

    /// Reach the host some other way than directly.
    pub fn with_route(mut self, route: Route) -> Self {
        self.route = route;
        self
    }

    pub fn with_notes(mut self, notes: impl Into<SharedString>) -> Self {
        self.notes = notes.into();
        self
    }

    /// The draft with surrounding blanks removed, an empty name taken as
    /// the address and an empty user name as [`super::DEFAULT_USER`], or the
    /// first thing wrong with it, in the order the host form checks. What the
    /// form cannot leave out of a draft (a credential to pick, a proxy
    /// password without a user) is the caller's to check.
    pub fn validated(mut self) -> Result<Self, HostDraftError> {
        self.name = self.name.trim().to_string().into();
        self.address = self.address.trim().to_string().into();
        if self.address.is_empty() {
            return Err(HostDraftError::Address);
        }
        if self.name.is_empty() {
            self.name = self.address.clone();
        }
        if self.port == 0 {
            return Err(HostDraftError::Port);
        }
        let user = self.user.trim();
        self.user = if user.is_empty() {
            super::DEFAULT_USER.into()
        } else {
            user.to_string().into()
        };
        match &mut self.route {
            Route::Direct => {}
            Route::Jump(hops) if hops.is_empty() => return Err(HostDraftError::NoJumpHosts),
            Route::Jump(hops) if hops.contains(&None) => {
                return Err(HostDraftError::DeletedJumpHost);
            }
            Route::Jump(_) => {}
            Route::Proxy(proxy) => {
                proxy.host = proxy.host.trim().to_string().into();
                if proxy.host.is_empty() {
                    return Err(HostDraftError::ProxyAddress);
                }
                if proxy.port == 0 {
                    return Err(HostDraftError::ProxyPort);
                }
                let user = proxy.user.as_deref().map(str::trim).unwrap_or_default();
                proxy.user = (!user.is_empty()).then(|| user.to_string().into());
            }
        }
        self.notes = self.notes.trim().to_string().into();
        Ok(self)
    }
}

/// The port of an SSH host that names none.
pub const SSH_PORT: u16 = 22;

/// Why a host draft cannot be saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostDraftError {
    Address,
    Port,
    NoJumpHosts,
    DeletedJumpHost,
    ProxyAddress,
    ProxyPort,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_host_name_uses_the_trimmed_address() {
        for (name, expected) in [
            ("", "example.com"),
            (" \t ", "example.com"),
            (" web-01 ", "web-01"),
        ] {
            let draft = HostDraft::new(name, " example.com ", 2222, "", AuthKind::Password)
                .validated()
                .unwrap();
            assert_eq!(draft.name.as_ref(), expected);
            assert_eq!(draft.address.as_ref(), "example.com");
            assert_eq!(draft.user.as_ref(), super::super::DEFAULT_USER);
            assert_eq!(draft.port, 2222);
        }
    }

    #[test]
    fn a_host_still_requires_an_address_with_or_without_a_name() {
        for name in ["", " ", "web-01"] {
            for address in ["", " \t "] {
                let error = HostDraft::new(name, address, 22, "root", AuthKind::Password)
                    .validated()
                    .unwrap_err();
                assert_eq!(error, HostDraftError::Address);
            }
        }
        let error = HostDraft::new("", "example.com", 0, "root", AuthKind::Password)
            .validated()
            .unwrap_err();
        assert_eq!(error, HostDraftError::Port);
    }

    #[test]
    fn a_route_is_checked_and_tidied_with_the_draft() {
        let base = || HostDraft::new("web", "10.0.0.1", 22, "root", AuthKind::Password);
        assert_eq!(
            base().with_route(Route::Jump(Vec::new())).validated(),
            Err(HostDraftError::NoJumpHosts)
        );
        assert_eq!(
            base()
                .with_route(Route::Jump(vec![Some(HostId(1)), None]))
                .validated(),
            Err(HostDraftError::DeletedJumpHost)
        );
        assert_eq!(
            base()
                .with_route(Route::Proxy(ProxySettings::new(ProxyKind::Http, " ", 8080)))
                .validated(),
            Err(HostDraftError::ProxyAddress)
        );
        assert_eq!(
            base()
                .with_route(Route::Proxy(ProxySettings::new(ProxyKind::Http, "p", 0)))
                .validated(),
            Err(HostDraftError::ProxyPort)
        );
        let proxy = ProxySettings::new(ProxyKind::Socks5, " 127.0.0.1 ", 7890).with_user(" ");
        let draft = base()
            .with_route(Route::Proxy(proxy))
            .with_notes(" 机房 A ")
            .validated()
            .unwrap();
        let Route::Proxy(proxy) = &draft.route else {
            panic!("not a proxy: {:?}", draft.route);
        };
        assert_eq!(
            (proxy.host.as_ref(), proxy.user.clone()),
            ("127.0.0.1", None)
        );
        assert_eq!(proxy.password_secret(), None);
        assert_eq!(draft.notes.as_ref(), "机房 A");
    }

    #[test]
    fn a_host_shows_its_endpoint() {
        let host = Host::new(
            HostId(1),
            HostDraft::new("web", "10.0.0.1", 22, "root", AuthKind::Password),
        );
        assert_eq!(host.endpoint(), "root@10.0.0.1:22");
        assert_eq!(host.password_secret(), SecretRef::password(1));
    }

    #[test]
    fn the_distributions_are_linux_and_the_rest_are_not() {
        let not_linux: Vec<_> = HostOs::ALL
            .iter()
            .filter(|os| !os.is_linux())
            .map(|os| os.label())
            .collect();
        assert_eq!(
            not_linux,
            ["macOS", "Windows", "FreeBSD", "OpenBSD", "NetBSD"]
        );
    }

    #[test]
    fn secret_ids_are_sixteen_letters_and_digits() {
        let id = SecretId::generate();
        assert_eq!(id.as_str().len(), 16);
        assert!(SecretId::has_shape(id.as_str()));
        assert_ne!(SecretId::generate(), id);

        let taken = [id.clone()];
        assert_ne!(SecretId::generate_unused(|id| taken.contains(id)), id);
    }

    #[test]
    fn ip_literals_are_told_apart_from_host_names() {
        assert!(is_ip_address("10.0.1.21"));
        assert!(is_ip_address("::1"));
        assert!(is_ip_address("[fe80::1]"));
        assert!(!is_ip_address("web-01.example.com"));
        assert!(!is_ip_address("localhost"));
        assert!(!is_ip_address(""));
    }

    #[test]
    fn every_operating_system_round_trips_through_the_database_spelling() {
        for os in HostOs::ALL {
            assert_eq!(HostOs::from_stored(os.as_str()), Some(*os));
            assert!(!os.label().is_empty());
            assert!(os.icon_path().starts_with("icons/os/"));
        }
    }

    #[test]
    fn the_light_brands_get_a_dark_mark_and_the_rest_get_white() {
        // Tux is black on yellow, and OpenBSD's Puffy likewise.
        assert_eq!(HostOs::Linux.brand_foreground(), Some(0x00_0000));
        assert_eq!(HostOs::OpenBsd.brand_foreground(), Some(0x00_0000));
        // Everything else follows the usual white-on-brand treatment.
        assert_eq!(HostOs::Ubuntu.brand_foreground(), Some(0xFF_FFFF));
        assert_eq!(HostOs::Debian.brand_foreground(), Some(0xFF_FFFF));
        assert_eq!(HostOs::Windows.brand_foreground(), Some(0xFF_FFFF));
    }

    #[test]
    fn a_monochrome_mark_leaves_the_colour_to_the_theme() {
        assert_eq!(HostOs::MacOs.brand_color(), None);
        assert_eq!(HostOs::MacOs.brand_foreground(), None);
    }

    #[test]
    fn stored_spellings_and_icons_are_unique() {
        let mut spellings: Vec<_> = HostOs::ALL.iter().map(|os| os.as_str()).collect();
        spellings.sort_unstable();
        let count = spellings.len();
        spellings.dedup();
        assert_eq!(spellings.len(), count, "两个变体用了同一个存储拼写");
    }

    #[test]
    fn an_unknown_spelling_reads_as_undetected() {
        assert_eq!(HostOs::from_stored("plan9"), None);
        assert_eq!(HostOs::from_stored(""), None);
    }

    #[test]
    fn a_new_host_has_not_been_probed_yet() {
        let host = Host::new(
            HostId(1),
            HostDraft::new("s", "h", 22, "root", AuthKind::Password),
        );
        assert_eq!(host.os, None);
    }

    #[test]
    fn a_password_is_the_default_and_spellings_round_trip() {
        assert_eq!(AuthKind::default(), AuthKind::Password);
        for auth in [AuthKind::Password, AuthKind::NoPassword] {
            assert_eq!(AuthKind::from_stored(auth.as_str()), auth);
        }
        assert_eq!(AuthKind::from_stored("credential"), AuthKind::Password);
        for kind in ProxyKind::ALL {
            assert_eq!(ProxyKind::from_stored(kind.as_str()), Some(kind));
        }
    }
}
