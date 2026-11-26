// 来自 since2006/shell-rs @ b2a307a: src/cli/install.rs（Agent CLI 工单 05）
// 改动：程序名 `micaterm`；Skill 装 Claude Code、Codex、Antigravity CLI 和通用的
// `~/.agents/skills`（规格第 2 节）；
// Windows 上 MicaTerm 是便携的，命令一律放在程序目录下的 `bin`，拷的是旁边的 `micaterm-cli.exe`
// （`bin` 里的副本自己认得上一级的数据目录，见 `app::data_dir_of`）。

//! Putting the `micaterm` command on the PATH and the agent skill where
//! agents look for it. Plain file system work, blocking, for a background
//! thread. Every path comes from [`IntegrationPaths`], so tests point it at
//! a temporary directory instead of the user's home.

use std::path::Path;
use std::{fs, io, path::PathBuf};

use gpui_kit::SharedString;

use crate::i18n::t;

/// The skill that teaches an agent to use `micaterm`.
pub const SKILL: &str = include_str!("SKILL.md");

/// What the command is called on the PATH, and what the skill folder is
/// called in an agent's skills directory.
pub(super) const COMMAND_NAME: &str = "micaterm";
const SKILL_FILE: &str = "SKILL.md";

/// An agent the skill can be installed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentKind {
    /// The shared Agent Skills directory that several agents read.
    Generic,
    Codex,
    ClaudeCode,
    /// Antigravity CLI（`agy`，Google Antigravity 的命令行版）：Skill 装进它自己的
    /// `~/.gemini/antigravity-cli/skills`，让 agy 在终端里能用 `micaterm`。
    Antigravity,
}

impl AgentKind {
    /// In the order the settings page lists them.
    pub const ALL: [AgentKind; 4] = [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::Antigravity,
        AgentKind::Generic,
    ];

    pub fn label(self) -> SharedString {
        match self {
            AgentKind::Generic => t!("cli.agent.generic"),
            AgentKind::Codex => "Codex".into(),
            AgentKind::ClaudeCode => "Claude Code".into(),
            AgentKind::Antigravity => "Antigravity".into(),
        }
    }

    pub fn description(self) -> Option<SharedString> {
        match self {
            AgentKind::Generic => Some(t!("cli.agent.generic_description")),
            _ => None,
        }
    }

    /// Stable, for element ids.
    pub fn key(self) -> &'static str {
        match self {
            AgentKind::Generic => "agents",
            AgentKind::Codex => "codex",
            AgentKind::ClaudeCode => "claude",
            AgentKind::Antigravity => "antigravity",
        }
    }

    /// The agent's skills directory, relative to the home directory, one
    /// name at a time so Windows shows it with its own separators.
    fn skills_dir(self) -> &'static [&'static str] {
        match self {
            AgentKind::Generic => &[".agents", "skills"],
            AgentKind::Codex => &[".codex", "skills"],
            AgentKind::ClaudeCode => &[".claude", "skills"],
            // Antigravity CLI（`agy`）读自己的全局技能目录。
            AgentKind::Antigravity => &[".gemini", "antigravity-cli", "skills"],
        }
    }
}

/// Where everything goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrationPaths {
    /// Holds the agents' skill directories.
    pub home: PathBuf,
    /// What puts `micaterm` on the PATH: a link to [`Self::exe`] on macOS
    /// and Linux, a copy of it on Windows.
    pub bin_link: PathBuf,
    /// The command itself: this program on macOS and Linux, the
    /// `micaterm-cli.exe` beside it on Windows.
    pub exe: PathBuf,
    /// Windows: the PATH that [`Self::bin_link`]'s folder is put on. `None`
    /// leaves every PATH alone; tests use it, and macOS and Linux install
    /// into a folder that is on the PATH already.
    pub user_path: Option<UserPath>,
}

/// A PATH the command's folder can be added to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserPath {
    /// The user's `Path` in the registry, which consoles opened from now on
    /// start with.
    Registry,
    /// A file holding the value, so tests leave the registry alone.
    File(PathBuf),
}

impl IntegrationPaths {
    /// The real locations, or `None` where the command cannot be installed.
    /// macOS gets `/usr/local/bin`, which is on every PATH and may need an
    /// administrator; Linux gets `~/.local/bin`, which needs nobody.
    /// Windows gets the `bin` folder beside the program, added to the
    /// user's PATH: MicaTerm is portable, and its data is beside it too.
    pub fn system() -> Option<Self> {
        let home = dirs::home_dir()?;
        let exe = program(
            std::env::current_exe().ok()?,
            std::env::var_os("APPIMAGE").map(PathBuf::from),
        );
        let (bin_link, exe, user_path) = if cfg!(target_os = "macos") {
            (
                PathBuf::from("/usr/local/bin").join(COMMAND_NAME),
                exe,
                None,
            )
        } else if cfg!(unix) {
            (home.join(".local/bin").join(COMMAND_NAME), exe, None)
        } else if cfg!(windows) {
            let (bin_link, exe) = windows_locations(&exe);
            (bin_link, exe, Some(UserPath::Registry))
        } else {
            return None;
        };
        Some(Self {
            home,
            bin_link,
            exe,
            user_path,
        })
    }

    pub fn skill_file(&self, agent: AgentKind) -> PathBuf {
        let mut file = self.home.clone();
        file.extend(agent.skills_dir());
        file.join(COMMAND_NAME).join(SKILL_FILE)
    }
}

/// 命令在 Windows 上放在哪，拷的是哪个控制台程序：程序目录下的 `bin`，拷旁边的
/// `micaterm-cli.exe`。`bin` 里的副本认得上一级的数据目录（`app::data_dir_of`）。
fn windows_locations(exe: &Path) -> (PathBuf, PathBuf) {
    let root = exe.parent().unwrap_or(Path::new("."));
    (
        root.join(crate::app::CLI_BIN_DIR)
            .join(format!("{COMMAND_NAME}.exe")),
        root.join(WINDOWS_CLI),
    )
}

/// The console program beside `micaterm.exe` on Windows.
const WINDOWS_CLI: &str = "micaterm-cli.exe";

/// The file the `micaterm` command runs. Inside an AppImage the executable
/// lives in a mount that is gone once the app exits, so the command runs
/// the AppImage itself, which `$APPIMAGE` names.
fn program(exe: PathBuf, appimage: Option<PathBuf>) -> PathBuf {
    match appimage {
        Some(appimage) if cfg!(target_os = "linux") || cfg!(test) => appimage,
        _ => exe,
    }
}

/// What is at [`IntegrationPaths::bin_link`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BinaryStatus {
    /// A link to this program, or on Windows a current copy on the PATH.
    Installed,
    Missing,
    /// A link to another copy of `micaterm`, or to something no longer
    /// there. Installing replaces it.
    Stale {
        target: PathBuf,
    },
    /// Something that is not ours: a file, or a link to another program.
    /// Left alone.
    Occupied {
        target: Option<PathBuf>,
    },
    /// Windows: a copy from another build. Installing updates it.
    Outdated,
    /// Windows: the copy is there but its folder is not on the PATH.
    NotOnPath,
    /// Windows: there is no `micaterm-cli.exe` beside the app to install.
    Unavailable,
}

pub fn binary_status(paths: &IntegrationPaths) -> BinaryStatus {
    #[cfg(windows)]
    return super::install_windows::binary_status(paths);
    #[cfg(not(windows))]
    link_status(paths)
}

/// Put `micaterm` on the PATH. Where the directory is not writable, macOS
/// asks for an administrator the way other apps install their command; the
/// user can cancel that.
pub fn install_binary(paths: &IntegrationPaths) -> io::Result<()> {
    #[cfg(windows)]
    return super::install_windows::install_binary(paths);
    #[cfg(not(windows))]
    install_link(paths)
}

/// Take `micaterm` off the PATH, but only ours: someone else's file of the
/// same name stays.
pub fn remove_binary(paths: &IntegrationPaths) -> io::Result<()> {
    #[cfg(windows)]
    return super::install_windows::remove_binary(paths);
    #[cfg(not(windows))]
    remove_link(paths)
}

/// Bring an installed command up to this build, when it is a copy from
/// another one; whether it was. A link always leads to the program it
/// points at, so this is Windows's alone.
pub fn update_outdated_binary(paths: &IntegrationPaths) -> io::Result<bool> {
    if binary_status(paths) != BinaryStatus::Outdated {
        return Ok(false);
    }
    install_binary(paths).map(|()| true)
}

#[cfg(not(windows))]
fn link_status(paths: &IntegrationPaths) -> BinaryStatus {
    let link = &paths.bin_link;
    let Ok(metadata) = fs::symlink_metadata(link) else {
        return BinaryStatus::Missing;
    };
    if !metadata.file_type().is_symlink() {
        return BinaryStatus::Occupied { target: None };
    }
    let Ok(target) = fs::read_link(link) else {
        return BinaryStatus::Occupied { target: None };
    };
    let resolved = link.parent().unwrap_or(Path::new("/")).join(&target);
    match (fs::canonicalize(&resolved), fs::canonicalize(&paths.exe)) {
        (Ok(linked), Ok(exe)) if linked == exe => BinaryStatus::Installed,
        _ if is_ours(&target) || !resolved.exists() => BinaryStatus::Stale { target },
        _ => BinaryStatus::Occupied {
            target: Some(target),
        },
    }
}

/// Whether a link target is a `micaterm` binary, this build or another.
#[cfg(not(windows))]
fn is_ours(target: &Path) -> bool {
    target.file_name().is_some_and(|name| name == COMMAND_NAME)
}

/// Link `micaterm` to this program.
#[cfg(not(windows))]
fn install_link(paths: &IntegrationPaths) -> io::Result<()> {
    if let BinaryStatus::Occupied { .. } = link_status(paths) {
        return Err(occupied(&paths.bin_link));
    }
    match link_binary(paths) {
        Err(error) if needs_administrator(&error) => run_as_administrator(&format!(
            "mkdir -p {} && ln -sfn {} {}",
            shell_quote(paths.bin_link.parent().unwrap_or(Path::new("/"))),
            shell_quote(&paths.exe),
            shell_quote(&paths.bin_link),
        )),
        result => result,
    }
}

#[cfg(not(windows))]
fn link_binary(paths: &IntegrationPaths) -> io::Result<()> {
    if let Some(dir) = paths.bin_link.parent() {
        fs::create_dir_all(dir)?;
    }
    match fs::remove_file(&paths.bin_link) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    #[cfg(unix)]
    return std::os::unix::fs::symlink(&paths.exe, &paths.bin_link);
    #[cfg(not(unix))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        t!("cli.install.unsupported").to_string(),
    ))
}

/// Remove the link, but only a link to `micaterm`.
#[cfg(not(windows))]
fn remove_link(paths: &IntegrationPaths) -> io::Result<()> {
    match link_status(paths) {
        BinaryStatus::Missing => Ok(()),
        BinaryStatus::Occupied { .. } => Err(occupied(&paths.bin_link)),
        _ => match fs::remove_file(&paths.bin_link) {
            Err(error) if needs_administrator(&error) => {
                run_as_administrator(&format!("rm -f {}", shell_quote(&paths.bin_link)))
            }
            result => result,
        },
    }
}

#[cfg(not(windows))]
fn occupied(link: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        t!("cli.install.occupied", path = link.display()).to_string(),
    )
}

#[cfg(not(windows))]
fn needs_administrator(error: &io::Error) -> bool {
    cfg!(target_os = "macos") && error.kind() == io::ErrorKind::PermissionDenied
}

/// Run a shell command as an administrator through the system's own
/// password prompt.
#[cfg(not(windows))]
fn run_as_administrator(command: &str) -> io::Result<()> {
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(administrator_script(command))
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr);
    // AppleScript's "User canceled." is error -128.
    if message.contains("-128") {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            t!("cli.install.cancelled").to_string(),
        ));
    }
    Err(io::Error::other(message.trim().to_string()))
}

/// `command` as an AppleScript `do shell script` with administrator
/// privileges: a string literal, so backslashes and quotes are escaped.
#[cfg(not(windows))]
fn administrator_script(command: &str) -> String {
    let literal = command.replace('\\', "\\\\").replace('"', "\\\"");
    format!("do shell script \"{literal}\" with administrator privileges")
}

/// A path as one single-quoted shell word.
#[cfg(not(windows))]
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

/// The skill file of one agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillStatus {
    Installed,
    Missing,
    /// A skill from another version of MicaTerm. Installing updates it.
    Outdated,
}

pub fn skill_status(paths: &IntegrationPaths, agent: AgentKind) -> SkillStatus {
    match fs::read_to_string(paths.skill_file(agent)) {
        Ok(content) if content == SKILL => SkillStatus::Installed,
        Ok(_) => SkillStatus::Outdated,
        Err(_) => SkillStatus::Missing,
    }
}

/// Write the skill, replacing an older one in a single step.
pub fn install_skill(paths: &IntegrationPaths, agent: AgentKind) -> io::Result<()> {
    let file = paths.skill_file(agent);
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let temporary = file.with_extension("md.tmp");
    fs::write(&temporary, SKILL)?;
    fs::rename(&temporary, &file)
}

/// Rewrite the skills that are installed but come from another version of
/// MicaTerm, so that an update reaches the agents too. Skills nobody
/// installed stay uninstalled. Returns how many were rewritten.
pub fn update_outdated_skills(paths: &IntegrationPaths) -> io::Result<usize> {
    let mut updated = 0;
    for agent in AgentKind::ALL {
        if skill_status(paths, agent) == SkillStatus::Outdated {
            install_skill(paths, agent)?;
            updated += 1;
        }
    }
    Ok(updated)
}

/// Delete the skill, and its folder once empty. The agent's own
/// directories stay.
pub fn remove_skill(paths: &IntegrationPaths, agent: AgentKind) -> io::Result<()> {
    let file = paths.skill_file(agent);
    match fs::remove_file(&file) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    if let Some(dir) = file.parent() {
        // Fails when something else is in there, which is fine.
        let _ = fs::remove_dir(dir);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    #[test]
    fn on_windows_the_command_goes_into_the_program_s_bin() {
        let exe = Path::new("D:/Tools/MicaTerm/micaterm.exe");
        let (bin_link, cli) = windows_locations(exe);
        assert_eq!(bin_link, Path::new("D:/Tools/MicaTerm/bin/micaterm.exe"));
        assert_eq!(cli, Path::new("D:/Tools/MicaTerm/micaterm-cli.exe"));
    }

    #[test]
    fn inside_an_appimage_the_command_links_to_the_appimage() {
        let mounted = PathBuf::from("/tmp/.mount_MicaTabc/usr/bin/micaterm");
        let appimage = PathBuf::from("/home/me/Apps/MicaTerm-x86_64.AppImage");
        assert_eq!(program(mounted.clone(), Some(appimage.clone())), appimage);
        assert_eq!(program(mounted.clone(), None), mounted);
    }

    fn paths(root: &Path) -> IntegrationPaths {
        let exe = root.join("app").join("micaterm");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "binary").unwrap();
        IntegrationPaths {
            home: root.join("home"),
            bin_link: root.join("bin").join("micaterm"),
            exe,
            user_path: None,
        }
    }

    #[test]
    fn skills_go_where_each_agent_looks() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let file = |dirs: &[&str]| {
            let mut file = paths.home.clone();
            file.extend(dirs);
            file.join("micaterm").join("SKILL.md")
        };
        assert_eq!(
            paths.skill_file(AgentKind::Generic),
            file(&[".agents", "skills"])
        );
        assert_eq!(
            paths.skill_file(AgentKind::ClaudeCode),
            file(&[".claude", "skills"])
        );
        assert_eq!(
            paths.skill_file(AgentKind::Codex),
            file(&[".codex", "skills"])
        );
        assert_eq!(
            paths.skill_file(AgentKind::Antigravity),
            file(&[".gemini", "antigravity-cli", "skills"])
        );
    }

    #[test]
    fn a_skill_installs_updates_and_removes_without_touching_its_neighbours() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let agent = AgentKind::ClaudeCode;
        assert_eq!(skill_status(&paths, agent), SkillStatus::Missing);

        install_skill(&paths, agent).unwrap();
        assert_eq!(skill_status(&paths, agent), SkillStatus::Installed);
        assert_eq!(fs::read_to_string(paths.skill_file(agent)).unwrap(), SKILL);

        fs::write(paths.skill_file(agent), "an older skill").unwrap();
        assert_eq!(skill_status(&paths, agent), SkillStatus::Outdated);
        install_skill(&paths, agent).unwrap();
        assert_eq!(skill_status(&paths, agent), SkillStatus::Installed);

        let other = paths
            .home
            .join(".claude")
            .join("skills")
            .join("other")
            .join("SKILL.md");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        fs::write(&other, "someone else's").unwrap();
        remove_skill(&paths, agent).unwrap();
        assert_eq!(skill_status(&paths, agent), SkillStatus::Missing);
        assert!(!paths.skill_file(agent).parent().unwrap().exists());
        assert!(other.exists());
        // Removing what is not there is not an error.
        remove_skill(&paths, agent).unwrap();
    }

    #[test]
    fn outdated_skills_are_rewritten_and_missing_ones_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let [first, second, ..] = AgentKind::ALL;
        install_skill(&paths, first).unwrap();
        fs::write(paths.skill_file(first), "an older skill").unwrap();

        assert_eq!(update_outdated_skills(&paths).unwrap(), 1);
        assert_eq!(skill_status(&paths, first), SkillStatus::Installed);
        assert_eq!(skill_status(&paths, second), SkillStatus::Missing);
        assert_eq!(update_outdated_skills(&paths).unwrap(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn the_command_links_to_this_program_and_is_removed_again() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        assert_eq!(binary_status(&paths), BinaryStatus::Missing);

        install_binary(&paths).unwrap();
        assert_eq!(binary_status(&paths), BinaryStatus::Installed);
        assert_eq!(fs::read_link(&paths.bin_link).unwrap(), paths.exe);
        // Again is harmless.
        install_binary(&paths).unwrap();
        assert_eq!(binary_status(&paths), BinaryStatus::Installed);

        remove_binary(&paths).unwrap();
        assert_eq!(binary_status(&paths), BinaryStatus::Missing);
        assert!(paths.exe.exists());
    }

    #[cfg(unix)]
    #[test]
    fn another_copy_is_replaced_but_someone_elses_file_is_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        fs::create_dir_all(paths.bin_link.parent().unwrap()).unwrap();

        // A link to a build that has since moved.
        std::os::unix::fs::symlink(root.path().join("gone/micaterm"), &paths.bin_link).unwrap();
        assert!(matches!(binary_status(&paths), BinaryStatus::Stale { .. }));
        install_binary(&paths).unwrap();
        assert_eq!(binary_status(&paths), BinaryStatus::Installed);

        // A different program under the same name.
        fs::remove_file(&paths.bin_link).unwrap();
        let other = root.path().join("other-tool");
        fs::write(&other, "other").unwrap();
        std::os::unix::fs::symlink(&other, &paths.bin_link).unwrap();
        assert_eq!(
            binary_status(&paths),
            BinaryStatus::Occupied {
                target: Some(other.clone())
            }
        );
        assert!(install_binary(&paths).is_err());
        assert!(remove_binary(&paths).is_err());
        assert_eq!(fs::read_link(&paths.bin_link).unwrap(), other);

        // A plain file.
        fs::remove_file(&paths.bin_link).unwrap();
        fs::write(&paths.bin_link, "script").unwrap();
        assert_eq!(
            binary_status(&paths),
            BinaryStatus::Occupied { target: None }
        );
        assert!(remove_binary(&paths).is_err());
        assert_eq!(fs::read_to_string(&paths.bin_link).unwrap(), "script");
    }

    #[cfg(not(windows))]
    #[test]
    fn administrator_commands_survive_spaces_and_quotes() {
        assert_eq!(
            shell_quote(Path::new("/Applications/Shell RS.app/it's")),
            r"'/Applications/Shell RS.app/it'\''s'"
        );
        assert_eq!(
            administrator_script(r#"ln -s 'a "b"' 'c\d'"#),
            r#"do shell script "ln -s 'a \"b\"' 'c\\d'" with administrator privileges"#
        );
    }
}
