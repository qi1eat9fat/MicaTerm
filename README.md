# MicaTerm

以 SSH 主机管理为核心的桌面终端。Rust + [GPUI](https://github.com/zed-industries/zed)（gpui-kit）编写，Windows 优先。

一个标签放一棵卡片分屏树，同一台主机分屏时复用已有连接、新开 shell 通道，不用重新登录、不用再输验证码；终端、WinSCP 式双栏 SFTP、远程文件编辑都在一个窗口里完成。全部配置放在程序旁边的 `data/` 文件夹，拷走文件夹就带走全部主机、凭据和设置。

> 当前是 0.1 版，只在 Windows 11 上验证过。macOS 和 Linux 的代码路径在，但还没实际跑通。

## 功能

**主机与连接**

- 平铺的主机列表：系统徽章（自动探测远程系统）、搜索（名称 / 地址 / 用户名，多词同时匹配）、拖动排序、连接状态
- 认证：密码、密钥、SSH Agent、无密码；键盘交互式认证自动回答密码，验证码等问题原样转给你
- 连接方式：直连、跳板机、SOCKS5 / HTTP 代理
- 自己的 known_hosts：首次连接确认主机密钥，密钥变化时告警
- 断开后在卡片里按 Enter 原地重连；错误信息是中文的，写明原因

**标签与卡片**

- 每个终端标签是一棵分屏树：向右 / 向下分屏，拖动分隔线调整大小，单张卡片放大到整个标签
- 远程卡片分屏复用同一条连接；只允许一个通道的堡垒机自动改为新建连接
- 本地卡片分屏在同一目录打开新终端
- 卡片顶行显示名称和前台程序，底部状态栏显示连接状态、延迟、当前目录、系统和尺寸
- 只做被动解析：从远程发出的 OSC 7 / 633 / 1337 / 3008 / 9;9 等序列和窗口标题读出目录与程序，**从不往远程写 rc 文件，也不另开通道查进程**
- 后台卡片有输出时标签亮绿点，结束或响铃时亮橙点

**终端**

- 10000 行回滚、智能大小写查找、Ctrl+单击打开链接、OSC 52 剪贴板、kitty 键盘协议、OSC 9/777 通知
- 本地 shell 自动检测（Windows 优先 PowerShell 7），也可手动指定
- 内置 MicaTerm、Tokyo、Catppuccin、Rosé Pine 四对浅色 / 深色主题；应用外观跟随系统或固定
- Win11 22H2 及以上可开 Mica 背景

**SFTP 与编辑**

- WinSCP 式本地 / 远程双栏，拖放上传下载（含从资源管理器拖入）、传输队列、断点续传、断线自动重连
- 冲突时覆盖 / 跳过、属性里改权限（可递归）
- 内置编辑器：行号、查找替换、常见语言语法高亮，Ctrl+S 经原连接写回，保留所有者、权限、BOM 和 CRLF；保存前检查文件是否被别人改过
- 图片预览、Markdown 预览

**凭据与保险库**

- 凭据可被多台主机共用；密钥可来自文件、粘贴，或现场生成 Ed25519 / RSA 4096
- 密码、口令、私钥加密存进数据目录里的保险库（Argon2id + ChaCha20-Poly1305），用主密码保护，可选「在本机记住」（数据密钥存进系统钥匙串）
- 主密码可以不设：此时秘密只靠本机钥匙串保护，拷到别的电脑需要重新输入

**命令片段**

- 所有主机共用的命令文本，单击输入或直接执行，发往焦点卡片

## 给 AI Agent 用

Claude Code、Codex 这类 Agent 可以用 `micaterm` 命令操作你保存的主机：列出和查看主机、在主机上执行命令、上传 / 下载 / 同步文件、增删改主机和凭据。命令经正在运行的 MicaTerm 登录，密码和私钥一直留在保险库里，Agent 拿不到；保险库锁着时它会请你解锁。

- **打开**：设置 › Agent › 打开「启用外部 Agent」。同一页可以把命令行工具装到 PATH（便携版装在程序旁的 `bin\` 并加进用户 PATH），给 Claude Code、Codex 装 Skill（教 Agent 怎么用这个命令）。
- **Agent 面板**：右侧图标栏最下面的「Agent」在 MicaTerm 里直接运行 Claude Code 或 Codex（设置里的「Agent 命令」，留空自动检测）。第一次点会列出还缺什么（总开关、命令行工具、Skill），确认后补上再启动。
- **交给 Agent**：远程卡片右键「交给 Agent」，把你正在用的终端交给面板里的 Agent。它用 `exec --card` 在这张卡片里执行命令（只许登录一次的堡垒机也能用），用 `cards read` 看屏幕；「收回」后它就碰不到了。Agent 跑在 MicaTerm 外面的终端里也行：没设 Agent 命令时卡片照样交出去，它用 `micaterm cards list` 找到这张卡片。
- **我在哪**：MicaTerm 里的本地终端都带着 `TERM_PROGRAM=micaterm`、`MICATERM_SELF_CARD`、`MICATERM_CLI`，Agent 跑 `micaterm env` 就知道自己在哪、能做什么。

```bash
micaterm env
micaterm hosts list
micaterm exec <主机 ID> "df -h /"
micaterm upload <主机 ID> ./dist /tmp/dist
micaterm cards list
micaterm exec --card <卡片 ID> "uptime"
micaterm cards read <卡片 ID> --lines 50
```

主机 ID 在主机右键菜单的「复制 ID」里，卡片 ID 在 `micaterm cards list` 里。每条命令的用法看 `micaterm <命令> --help`；输出给程序读时（管道或 `--json`）是 JSON。Windows 上命令是压缩包里的 `micaterm-cli.exe`，装进 PATH 后叫 `micaterm`。

## 下载与使用

到 [Releases](../../releases) 下载 `MicaTerm-<版本>-windows-x86_64-portable.zip`，解压到任意**有写入权限**的文件夹，运行 `micaterm.exe`。

- 压缩包里有一个空的 `data/` 文件夹，所有配置都写在这里。整个文件夹拷到别的电脑就能继续用。
- `micaterm-cli.exe` 是给 AI Agent 用的命令行（见上一节），不用 Agent 的话可以不管它。
- 不要放进 `C:\Program Files` 这类普通权限写不进去的位置。
- 程序静态链接了 C 运行库，不需要另装 VC++ 运行库（LTSC 版 Windows 也能直接运行）。
- 程序暂未签名，首次运行时 SmartScreen 可能提示「未知发布者」，点「更多信息 → 仍要运行」即可。

用环境变量 `MICATERM_DATA_DIR` 可以把数据目录指到别处。

### 常用快捷键

Windows / Linux 上用 Ctrl+Shift 系，只按 Ctrl 的组合全部留给终端；macOS 上用 ⌘。

| 操作 | Windows / Linux | macOS |
|---|---|---|
| 新建本地终端标签 | Ctrl+Shift+T | ⌘T |
| 新建主机 | Ctrl+Shift+N | ⌘N |
| 搜索主机 | Ctrl+Shift+H | ⌘⇧H |
| 显示 / 隐藏主机栏 | Ctrl+Shift+B | ⌘B |
| 打开 SFTP | Ctrl+Shift+U | ⌘⇧U |
| 命令片段 | Ctrl+Shift+S | ⌘⇧S |
| 设置 | Ctrl+, | ⌘, |
| 切换标签 | Ctrl+Tab / Ctrl+Shift+Tab | 同左 |
| 跳到第 N 个标签 | Ctrl+Shift+1…9 | — |
| 向右 / 向下分屏 | Ctrl+Shift+D / Ctrl+Shift+E | ⌘D / ⌘⇧D |
| 关闭卡片 | Ctrl+Shift+W | ⌘W |
| 放大 / 还原卡片 | Ctrl+Shift+Enter | ⌘⇧Enter |
| 在卡片间移动焦点 | Alt+方向键 | ⌥⌘+方向键 |
| 移动分隔线 | Alt+Shift+方向键 | ⌃⌘+方向键 |
| 复制 / 粘贴 | Ctrl+Shift+C / Ctrl+Shift+V | ⌘C / ⌘V |
| 终端内查找 | Ctrl+Shift+F，F3 / Shift+F3 | ⌘F，⌘G / ⌘⇧G |
| 清屏 | Ctrl+Shift+K | ⌘K |
| 终端字号 | Ctrl+= / Ctrl+- / Ctrl+0 | ⌘= / ⌘- / ⌘0 |

## 从源码构建

需要：

- Rust：版本由 [`rust-toolchain.toml`](rust-toolchain.toml) 固定，装好 [rustup](https://rustup.rs) 后首次构建会自动下载
- Windows：Visual Studio 2022 生成工具（「使用 C++ 的桌面开发」）、[CMake](https://cmake.org)、[NASM](https://www.nasm.us)（`aws-lc-sys` 需要）

```bash
cargo run
```

```bash
cargo build --release
```

产物在 `target/release/micaterm.exe`，开发版的数据自然落在 `target/debug/data/`，不会和日常用的那份混在一起。

运行测试（含用 GPUI 测试框架驱动真实窗口壳的界面测试，以及进程内 SSH 测试服务器）：

```bash
cargo test --workspace
```

## 发布

推送 `v<版本号>` 标签即触发 [GitHub Actions 发布流程](.github/workflows/release.yml)：

1. 核对标签和 `Cargo.toml` 的版本号一致
2. 在 Windows 上构建 release 版，检查两个程序都不依赖 VC++ 运行库
3. 打包便携版 zip（`micaterm.exe` + `micaterm-cli.exe` + 空 `data/` 文件夹）并生成 SHA-256 校验文件
4. 创建 GitHub Release 并上传；版本号带 `-`（如 `0.2.0-beta.1`）的标为预发布

```bash
git tag v0.1.0
```

```bash
git push origin v0.1.0
```

在 Actions 页面手动运行该流程时，只构建打包、把产物作为 artifact 上传，不创建 Release。

## 项目结构

```
src/                    窗口壳、卡片分屏、主机、SFTP 标签、编辑器、设置、保险库……
crates/
  micaterm-terminal/    终端引擎、终端视图、本地 PTY
  micaterm-ssh/         SSH 连接、登录、代理、SFTP 传输
  micaterm-hosts/       主机 / 凭据 / 命令片段的模型与数据库、保险库加密
  micaterm-ui/          应用和终端共用的界面组件
locales/                界面文案
tests/ui/               界面测试
```

## 许可

[GPL-3.0](LICENSE)。

终端、SSH、主机等模块拷自 [shell-rs](https://github.com/since2006/shell-rs) 并裁剪改造，原 MIT 部分的许可和版权声明保留在各 crate 的 `LICENSE` 里。内置字体 JetBrains Mono 采用 [SIL OFL 1.1](assets/fonts/OFL.txt)，系统徽章来自 [Simple Icons](https://simpleicons.org)（CC0）。
