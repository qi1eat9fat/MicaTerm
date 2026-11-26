//! 程序名表：命令名（进程名，或远程报告的命令）映射到程序图块的图标和颜色。
//! 本地卡片和远程卡片共用的纯函数模块，不依赖 gpui，颜色用 `0xRRGGBB`。
//!
//! 判断顺序：去掉路径和前导 `-` → 按小写查表 → 是 shell 就用同一个终端图块（同一个图标、同一种
//! 中性底色，不随 shell 品牌变色）→ 其余用终端图标，底色按名称的 FNV-1a 哈希从 8 种颜色里取一种
//! （同一个名字每次都一样）。
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

/// 终端图标（Lucide `terminal`）：所有 shell 和表里没有的程序都用它。
pub const TERMINAL_GLYPH: &str = "icons/terminal.svg";
/// 文件夹图标（Lucide `folder`）：SFTP 标签的图块。
pub const FOLDER_GLYPH: &str = "icons/folder.svg";
/// 显示器图标（Lucide `monitor`）：SFTP 标签里「本机」那张卡片。
pub const MONITOR_GLYPH: &str = "icons/monitor.svg";

/// 所有 shell 共用的图块颜色：不论 PowerShell、bash 还是 cmd，本地终端都长一样，
/// 不随品牌变色。取值沿用原型 `PROG.pwsh` 的深灰底。
const SHELL_BG: u32 = 0x334155;
const SHELL_FG: u32 = 0xffffff;

/// Python 图块配色（Simple Icons 的 `python`）。
const PYTHON_BG: u32 = 0x3776AB;
const PYTHON_FG: u32 = 0xffd43b;

/// 本机终端图块的蓝底（原型 `fcard` 的显示器卡片）；`card::picker` 也用它。
pub const LOCAL_FILES_BG: u32 = 0x3b82f6;

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
        PYTHON_BG,
        PYTHON_FG,
        false,
    ),
    (
        "python3",
        "Python",
        "icons/brands/python.svg",
        PYTHON_BG,
        PYTHON_FG,
        false,
    ),
    (
        "ipython",
        "Python",
        "icons/brands/python.svg",
        PYTHON_BG,
        PYTHON_FG,
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

/// 在提示符下的 shell `name`：所有 shell 都用同一个终端图块——同一个终端图标、同一种中性底色，
/// 不随品牌变色。和标签、卡片标题行上的实心方块用的是同一套颜色。
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

/// SFTP 标签里「本机」那张卡片的图块（原型 `fcard` 的蓝底显示器）。
pub fn local_files_program() -> Program {
    Program {
        name: "Local".to_string(),
        glyph: MONITOR_GLYPH,
        tile_bg: LOCAL_FILES_BG,
        tile_fg: 0xffffff,
        border: false,
        shell: false,
        letter: None,
        off: false,
    }
}

/// SFTP 标签的图块：固定颜色的文件夹（原型 `PROG.sftp`）。
pub fn sftp_program() -> Program {
    Program {
        name: "SFTP".to_string(),
        glyph: FOLDER_GLYPH,
        tile_bg: 0xE8A33D,
        tile_fg: 0xffffff,
        border: false,
        shell: false,
        letter: None,
        off: false,
    }
}

/// 编辑器标签的图块：按文件类型取色，写一两个字（原型 `PROG.yaml`、`md`、`sh`、`conf`）。
/// 没有类型的文件写文件名的首字，底色按名字取。
pub fn editor_program(name: &str) -> Program {
    let lower = name.to_ascii_lowercase();
    let extension = lower
        .rsplit_once('.')
        .map_or("", |(_, extension)| extension);
    let tile = match crate::editor::language_for(name) {
        "bash" => Some(("$", 0x4EAA25, 0xffffff)),
        "yaml" => Some(("Y", 0xCB171E, 0xffffff)),
        "toml" => Some(("T", 0x9C4221, 0xffffff)),
        "json" => Some(("{}", 0xCBCB41, 0x1a1a1a)),
        "python" => Some(("py", PYTHON_BG, PYTHON_FG)),
        "javascript" => Some(("js", 0xF7DF1E, 0x1a1a1a)),
        "html" => Some(("<>", 0xE34F26, 0xffffff)),
        "css" => Some(("#", 0x1572B6, 0xffffff)),
        "markdown" => Some(("M", 0x4B5563, 0xffffff)),
        "sql" => Some(("S", 0x336791, 0xffffff)),
        "php" => Some(("P", 0x777BB4, 0xffffff)),
        "go" => Some(("Go", 0x00ADD8, 0xffffff)),
        "lua" => Some(("L", 0x2C2D72, 0xffffff)),
        "make" => Some(("M", 0x6B7280, 0xffffff)),
        "diff" => Some(("\u{b1}", 0x3F8F4F, 0xffffff)),
        _ => match extension {
            "conf" | "cfg" | "cnf" | "ini" | "service" => Some(("\u{2699}", 0x6D8086, 0xffffff)),
            "txt" | "log" => Some(("T", 0x9E9E9E, 0xffffff)),
            _ => None,
        },
    };
    let (letter, tile_bg, tile_fg) = match tile {
        Some((letter, background, foreground)) => {
            (Some(letter.to_string()), background, foreground)
        }
        None => (
            name.chars().next().map(|first| first.to_string()),
            hash_color(name),
            0xffffff,
        ),
    };
    Program {
        name: name.to_string(),
        glyph: TERMINAL_GLYPH,
        tile_bg,
        tile_fg,
        border: false,
        shell: false,
        letter,
        off: false,
    }
}

/// 全部图标的资源路径：资源来源要能把它们都加载出来（`app::assets` 的测试核对）。
pub fn glyphs() -> impl Iterator<Item = &'static str> {
    [TERMINAL_GLYPH, FOLDER_GLYPH, MONITOR_GLYPH]
        .into_iter()
        .chain(PROGRAMS.iter().map(|entry| entry.2))
}

#[cfg(test)]
mod tests {
    use super::{
        LOCAL_FILES_BG, PYTHON_BG, PYTHON_FG, editor_program, local_files_program, program_of,
    };

    /// 重复出现的品牌色都从命名常量取，值不变。
    #[test]
    fn duplicate_brand_colors_share_named_constants() {
        let python = editor_program("script.py");
        assert_eq!((python.tile_bg, python.tile_fg), (PYTHON_BG, PYTHON_FG));
        let table_python = program_of("python");
        assert_eq!(
            (table_python.tile_bg, table_python.tile_fg),
            (PYTHON_BG, PYTHON_FG)
        );
        assert_eq!(local_files_program().tile_bg, LOCAL_FILES_BG);
    }

    /// 颜色和字取自原型的 `PROG`。
    #[test]
    fn an_editor_tab_gets_the_tile_of_its_file_type() {
        let tile = |name: &str| {
            let program = editor_program(name);
            (program.letter.unwrap(), program.tile_bg)
        };
        assert_eq!(tile("docker-compose.yml"), ("Y".to_string(), 0xCB171E));
        assert_eq!(tile("README.md"), ("M".to_string(), 0x4B5563));
        assert_eq!(tile("deploy.sh"), ("$".to_string(), 0x4EAA25));
        assert_eq!(tile("nginx.conf"), ("\u{2699}".to_string(), 0x6D8086));
        // 同一种类型的文件同色；没有类型的写首字。
        assert_eq!(tile("a.yaml").1, tile("b.yml").1);
        assert_eq!(tile("hosts").0, "h");
        assert_eq!(editor_program("hosts").name, "hosts");
    }

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

    /// 所有 shell（不论品牌）都用同一个终端图块：同一个图标、同一种中性底色，不随品牌变色。
    #[test]
    fn every_shell_shows_the_same_neutral_terminal_tile() {
        for shell in [
            "cmd",
            "sh",
            "dash",
            "ksh",
            "tcsh",
            "csh",
            "elvish",
            "xonsh",
            "pwsh",
            "powershell",
            "PWSh.EXE",
            "PowerShell",
            "bash",
            "zsh",
            "fish",
            "nu",
        ] {
            let program = program_of(shell);
            assert!(program.shell, "{shell}");
            assert_eq!(program.glyph, "icons/terminal.svg", "{shell}");
            assert_eq!((program.tile_bg, program.tile_fg), (0x334155, 0xffffff));
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
