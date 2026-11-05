//! 程序名表：命令名（进程名，或远程报告的命令）映射到程序图块的图标和颜色。
//! 本地卡片和远程卡片共用的纯函数模块，不依赖 gpui，颜色用 `0xRRGGBB`。
//!
//! 判断顺序：去掉路径和前导 `-` → 按小写查表 → 是 shell 就用终端图块 →
//! 其余用终端图标，底色按名称的 FNV-1a 哈希从 8 种颜色里取一种（同一个名字每次都一样）。
// 参考 gorex 的模型重写：programs.go（程序表的数值取自它，图标只用 Simple Icons 和 Lucide）。

/// 一个程序在界面上怎么显示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// 显示名（「Neovim」）；表里没有的就是它自己的命令名。
    pub name: String,
    /// 图块上单色图标的资源路径（`icons/…svg`、`icons/brands/…svg`）。
    pub glyph: &'static str,
    /// 图块底色和图标色。
    pub tile_bg: u32,
    pub tile_fg: u32,
    /// 浅色底的图块描一圈细边，不然和白底分不开。
    pub border: bool,
    /// 是 shell（在提示符下）：用终端图块，不亮绿点。
    pub shell: bool,
    /// 图块上写字而不是画图标：远程卡片还没探测出系统时，系统徽章是主机名称的首字（工单 12）。
    pub letter: Option<String>,
    /// 连接断开了（或还没连上）：图块变灰（原型 `.tile i.off`，工单 12）。
    pub off: bool,
}

impl Program {
    /// 同一个图块，变灰。
    pub fn offline(mut self) -> Self {
        self.off = true;
        self
    }
}

/// 表里的一项：命令名（小写），显示名，图标，底色，图标色，要不要描边。
type Entry = (&'static str, &'static str, &'static str, u32, u32, bool);

/// 终端图标（Lucide `terminal`）：shell 和表里没有的程序都用它。
pub const TERMINAL_GLYPH: &str = "icons/terminal.svg";

/// shell 图块的颜色：取全部界面原型的 `PROG.pwsh`。
const SHELL_BG: u32 = 0x334155;
const SHELL_FG: u32 = 0x86efac;

/// 在提示符下的 shell。
const SHELLS: &[&str] = &[
    "zsh",
    "bash",
    "fish",
    "sh",
    "dash",
    "nu",
    "pwsh",
    "elvish",
    "xonsh",
    "tcsh",
    "csh",
    "ksh",
    "cmd",
    "powershell",
];

/// 已知程序。数值取自 gorex 的 programs.go；同名的别名（python3、vi、man……）单列一行。
const PROGRAMS: &[Entry] = &[
    (
        "codex",
        "Codex",
        "icons/brands/openai.svg",
        0xf7f7f7,
        0x1a1a1a,
        true,
    ),
    (
        "claude",
        "Claude Code",
        "icons/brands/claude.svg",
        0xd97757,
        0xffffff,
        false,
    ),
    (
        "node",
        "Node",
        "icons/brands/nodedotjs.svg",
        0x5fa04e,
        0xffffff,
        false,
    ),
    (
        "bun",
        "Bun",
        "icons/brands/bun.svg",
        0xfbf0df,
        0x3b2a20,
        true,
    ),
    (
        "deno",
        "Deno",
        "icons/brands/deno.svg",
        0x1b1b1b,
        0xffffff,
        false,
    ),
    (
        "python",
        "Python",
        "icons/brands/python.svg",
        0x3776ab,
        0xffd43b,
        false,
    ),
    (
        "python3",
        "Python",
        "icons/brands/python.svg",
        0x3776ab,
        0xffd43b,
        false,
    ),
    (
        "ipython",
        "Python",
        "icons/brands/python.svg",
        0x3776ab,
        0xffd43b,
        false,
    ),
    (
        "lazygit",
        "Git Changes",
        "icons/diff.svg",
        0x3f8f4f,
        0xffffff,
        false,
    ),
    (
        "tig",
        "Git Log",
        "icons/git-branch.svg",
        0xf05032,
        0xffffff,
        false,
    ),
    (
        "git",
        "Git",
        "icons/brands/git.svg",
        0xf05032,
        0xffffff,
        false,
    ),
    (
        "vim",
        "Vim",
        "icons/brands/vim.svg",
        0x019733,
        0xffffff,
        false,
    ),
    (
        "vi",
        "Vim",
        "icons/brands/vim.svg",
        0x019733,
        0xffffff,
        false,
    ),
    (
        "view",
        "Vim",
        "icons/brands/vim.svg",
        0x019733,
        0xffffff,
        false,
    ),
    (
        "nvim",
        "Neovim",
        "icons/brands/neovim.svg",
        0x2b7a3d,
        0xffffff,
        false,
    ),
    (
        "hx",
        "Helix",
        "icons/brands/helix.svg",
        0x281733,
        0xc7a3f5,
        false,
    ),
    (
        "emacs",
        "Emacs",
        "icons/brands/gnuemacs.svg",
        0x7f5ab6,
        0xffffff,
        false,
    ),
    (
        "htop",
        "Activity",
        "icons/activity.svg",
        0x1f6f43,
        0xa7f3c4,
        false,
    ),
    (
        "btop",
        "Activity",
        "icons/activity.svg",
        0x1f6f43,
        0xa7f3c4,
        false,
    ),
    (
        "top",
        "Activity",
        "icons/activity.svg",
        0x1f6f43,
        0xa7f3c4,
        false,
    ),
    ("ssh", "SSH", "icons/globe.svg", 0x5b4bd6, 0xffffff, false),
    ("mosh", "Mosh", "icons/globe.svg", 0x5b4bd6, 0xffffff, false),
    (
        "docker",
        "Docker",
        "icons/brands/docker.svg",
        0x2496ed,
        0xffffff,
        false,
    ),
    ("go", "Go", "icons/brands/go.svg", 0x00add8, 0xffffff, false),
    (
        "cargo",
        "Cargo",
        "icons/brands/rust.svg",
        0x2b2b2b,
        0xf4a261,
        false,
    ),
    (
        "rustc",
        "Rust",
        "icons/brands/rust.svg",
        0x2b2b2b,
        0xf4a261,
        false,
    ),
    (
        "npm",
        "npm",
        "icons/brands/npm.svg",
        0xcb3837,
        0xffffff,
        false,
    ),
    (
        "pnpm",
        "pnpm",
        "icons/brands/pnpm.svg",
        0xf69220,
        0xffffff,
        false,
    ),
    (
        "yarn",
        "Yarn",
        "icons/brands/yarn.svg",
        0x2c8ebb,
        0xffffff,
        false,
    ),
    (
        "ruby",
        "Ruby",
        "icons/brands/ruby.svg",
        0xcc342d,
        0xffffff,
        false,
    ),
    (
        "irb",
        "Ruby",
        "icons/brands/ruby.svg",
        0xcc342d,
        0xffffff,
        false,
    ),
    (
        "lua",
        "Lua",
        "icons/brands/lua.svg",
        0x2c2d72,
        0xffffff,
        false,
    ),
    (
        "php",
        "PHP",
        "icons/brands/php.svg",
        0x777bb4,
        0xffffff,
        false,
    ),
    (
        "psql",
        "PostgreSQL",
        "icons/brands/postgresql.svg",
        0x336791,
        0xffffff,
        false,
    ),
    (
        "mysql",
        "MySQL",
        "icons/brands/mysql.svg",
        0x00758f,
        0xffffff,
        false,
    ),
    (
        "redis-cli",
        "Redis",
        "icons/brands/redis.svg",
        0xd82c20,
        0xffffff,
        false,
    ),
    (
        "tmux",
        "tmux",
        "icons/brands/tmux.svg",
        0x1bb91f,
        0xffffff,
        false,
    ),
    ("make", "Make", "icons/cpu.svg", 0x6b7280, 0xffffff, false),
    (
        "swift",
        "Swift",
        "icons/brands/swift.svg",
        0xf05138,
        0xffffff,
        false,
    ),
    (
        "kotlin",
        "Kotlin",
        "icons/brands/kotlin.svg",
        0x7f52ff,
        0xffffff,
        false,
    ),
    (
        "less",
        "Pager",
        "icons/file-code.svg",
        0x475569,
        0xffffff,
        false,
    ),
    (
        "man",
        "Pager",
        "icons/file-code.svg",
        0x475569,
        0xffffff,
        false,
    ),
];

/// 表里没有的程序取色的 8 种颜色。
const PALETTE: [u32; 8] = [
    0x6d5bd0, 0x2f80ed, 0x0f9d8a, 0xd9822b, 0xc2417d, 0x3a7d44, 0x8a5a44, 0x4e5d94,
];

/// 命令名 `name` 怎么显示。`name` 可以带路径、`.exe`、登录 shell 的前导 `-`，不分大小写：
/// 查表、取色、显示名都用小写的名字（Windows 上 `PING.EXE` 和 `ping` 是同一个程序）。
pub fn program_of(name: &str) -> Program {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let base = base.trim_start_matches('-');
    let base = match base.len().checked_sub(4) {
        Some(at) if base.is_char_boundary(at) && base[at..].eq_ignore_ascii_case(".exe") => {
            &base[..at]
        }
        _ => base,
    };
    let key = base.to_ascii_lowercase();
    if let Some((_, display, glyph, tile_bg, tile_fg, border)) =
        PROGRAMS.iter().find(|entry| entry.0 == key)
    {
        return Program {
            name: display.to_string(),
            glyph,
            tile_bg: *tile_bg,
            tile_fg: *tile_fg,
            border: *border,
            shell: false,
            letter: None,
            off: false,
        };
    }
    if SHELLS.contains(&key.as_str()) {
        return shell_program(&key);
    }
    Program {
        tile_bg: hash_color(&key),
        name: key,
        glyph: TERMINAL_GLYPH,
        tile_fg: 0xffffff,
        border: false,
        shell: false,
        letter: None,
        off: false,
    }
}

/// 在提示符下的 shell `name`：不管表里有没有它，都是终端图块（卡片的 shell 自己在前台时用）。
pub fn shell_program(name: &str) -> Program {
    Program {
        name: name.to_string(),
        glyph: TERMINAL_GLYPH,
        tile_bg: SHELL_BG,
        tile_fg: SHELL_FG,
        border: false,
        shell: true,
        letter: None,
        off: false,
    }
}

/// 名字的颜色，每次都一样：FNV-1a（32 位）对 8 取模。
fn hash_color(name: &str) -> u32 {
    let hash = name.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    PALETTE[hash as usize % PALETTE.len()]
}

/// 全部图标的资源路径：资源来源要能把它们都加载出来（`app::assets` 的测试核对）。
pub fn glyphs() -> impl Iterator<Item = &'static str> {
    std::iter::once(TERMINAL_GLYPH).chain(PROGRAMS.iter().map(|entry| entry.2))
}

#[cfg(test)]
mod tests {
    use super::program_of;

    #[test]
    fn a_known_command_shows_its_own_tile() {
        let vim = program_of("vim");
        assert_eq!(vim.name, "Vim");
        assert_eq!(vim.glyph, "icons/brands/vim.svg");
        assert_eq!((vim.tile_bg, vim.tile_fg), (0x019733, 0xffffff));
        assert!(!vim.shell);
    }

    #[test]
    fn a_command_is_looked_up_without_its_path_case_or_login_dash() {
        assert_eq!(program_of("/usr/bin/NVIM").name, "Neovim");
        assert_eq!(program_of(r"C:\tools\Git.exe").name, "Git");
        // 别名：python3、vi、man。
        assert_eq!(program_of("python3"), program_of("python"));
        assert_eq!(program_of("vi").name, "Vim");
        assert_eq!(program_of("man").name, "Pager");
        // 解释器跑的程序按脚本名查（`foreground::program_name` 给出的名字）。
        let codex = program_of("codex");
        assert_eq!(codex.glyph, "icons/brands/openai.svg");
        assert!(codex.border, "浅色底描边");
    }

    #[test]
    fn a_shell_shows_the_terminal_tile_under_its_own_name() {
        for shell in ["pwsh", "powershell", "cmd", "bash", "-zsh", "fish", "nu"] {
            let program = program_of(shell);
            assert!(program.shell, "{shell}");
            assert_eq!(program.glyph, "icons/terminal.svg");
            assert_eq!((program.tile_bg, program.tile_fg), (0x334155, 0x86efac));
        }
        assert_eq!(program_of("-zsh").name, "zsh");
        assert_eq!(program_of("pwsh.exe").name, "pwsh");
    }

    #[test]
    fn an_unknown_command_gets_a_color_of_its_own_every_time() {
        let first = program_of("my-tool");
        assert_eq!(first, program_of("my-tool"));
        assert_eq!(first.name, "my-tool");
        assert_eq!(first.glyph, "icons/terminal.svg");
        assert_eq!(first.tile_fg, 0xffffff);
        assert!(!first.shell);
        // FNV-1a（偏移基 2166136261、乘数 16777619）对 8 取模：这几个值是事先算好的。
        assert_eq!(program_of("my-tool").tile_bg, 0xc2417d);
        assert_eq!(program_of("a").tile_bg, 0xc2417d);
        assert_eq!(program_of("ping").tile_bg, 0x2f80ed);
    }

    /// 表里没有的命令也不分大小写：Windows 上 `PING.EXE` 和 `ping` 是同一个程序，同色同名。
    #[test]
    fn an_unknown_command_looks_the_same_in_any_case() {
        assert_eq!(program_of("PING.EXE"), program_of("ping"));
        assert_eq!(program_of(r"C:\Windows\System32\PING.EXE").name, "ping");
        assert_eq!(program_of("My-Tool").tile_bg, 0xc2417d);
        // shell 的名字也规范成小写。
        assert_eq!(program_of("PWSH.EXE").name, "pwsh");
    }
}
