//! 本地卡片显示的 shell 名称和版本（新写，shell-rs 没有「shell 版本」）。
//!
//! 名称是程序的文件名（`pwsh`），卡片头部用；版本给卡片状态栏（「PowerShell 7.5.2」）。
//! Windows 上读 exe 的版本资源；读不到、或者别的平台，就没有版本，状态栏只写名称。

use crate::LocalShell;

/// 一个本地 shell 叫什么、是哪个版本。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalShellInfo {
    /// 卡片头部显示的名字：程序文件名去掉 `.exe`。
    pub name: String,
    /// 状态栏显示的版本，带产品名（「PowerShell 7.5.2」）；不知道时是 `None`。
    pub version: Option<String>,
}

impl LocalShellInfo {
    pub fn new(name: impl Into<String>, version: Option<&str>) -> Self {
        Self {
            name: name.into(),
            version: version.map(str::to_owned),
        }
    }

    /// 查一查 `shell`：名称取程序文件名，版本读程序的版本资源。阻塞：在后台线程调用。
    pub fn detect(shell: &LocalShell) -> Self {
        let program = shell.program();
        let name = shell.display_name();
        let version = program
            .as_deref()
            .and_then(read_version)
            .and_then(|resource| describe(&name, &resource));
        Self { name, version }
    }

    /// 状态栏写的：有版本写版本，否则写名称。
    pub fn label(&self) -> &str {
        self.version.as_deref().unwrap_or(&self.name)
    }
}

/// 版本资源里用得到的几项。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct VersionResource {
    product_name: String,
    product_version: String,
    file_description: String,
    /// 固定版本号的前三段。
    fixed: Option<(u16, u16, u16)>,
}

/// 从版本资源拼出状态栏的版本文字。
///
/// - 随 Windows 发的程序（产品名是「Microsoft® Windows® Operating System」）版本号就是系统的，
///   写「文件说明 + 系统版本」；Windows PowerShell 例外，它从 Windows 10 起一直是 5.1，资源里看不出来；
/// - 其余的写「产品名 + 产品版本的第一段」：pwsh 的产品版本是「7.5.2 SHA: …」；
/// - 没有字符串信息时，用固定版本号。
fn describe(name: &str, resource: &VersionResource) -> Option<String> {
    let fixed = resource
        .fixed
        .map(|(major, minor, build)| format!("{major}.{minor}.{build}"));
    let bundled = resource.product_name.contains("Windows")
        && resource.product_name.contains("Operating System");
    if bundled {
        if name.eq_ignore_ascii_case("powershell") {
            return Some(format!("{} 5.1", resource.file_description));
        }
        let title = non_empty(&resource.file_description).unwrap_or(name);
        return Some(match fixed {
            Some(version) => format!("{title} {version}"),
            None => title.to_string(),
        });
    }
    let version = resource
        .product_version
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .or(fixed)?;
    let title = non_empty(&resource.product_name)
        .or_else(|| non_empty(&resource.file_description))
        .unwrap_or(name);
    Some(format!("{title} {version}"))
}

fn non_empty(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty()).then_some(text)
}

#[cfg(windows)]
fn read_version(program: &std::path::Path) -> Option<VersionResource> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// `block` 里 `sub_block` 那一项的指针和长度。
    fn query(block: &[u8], sub_block: &str) -> Option<(*const u8, u32)> {
        let sub_block = wide(sub_block);
        let mut pointer = std::ptr::null_mut();
        let mut length = 0u32;
        // SAFETY: `block` 是 GetFileVersionInfoW 填好的版本资源，`sub_block` 以 0 结尾。
        let found = unsafe {
            VerQueryValueW(
                block.as_ptr().cast(),
                sub_block.as_ptr(),
                &mut pointer,
                &mut length,
            )
        };
        (found != 0 && !pointer.is_null() && length > 0)
            .then_some((pointer.cast_const().cast(), length))
    }

    fn string(block: &[u8], table: &str, name: &str) -> String {
        let Some((pointer, length)) = query(block, &format!(r"\StringFileInfo\{table}\{name}"))
        else {
            return String::new();
        };
        // SAFETY: 字符串值是 `length` 个 UTF-16 字符（含结尾的 0），在 `block` 里面。
        let text = unsafe { std::slice::from_raw_parts(pointer.cast::<u16>(), length as usize) };
        String::from_utf16_lossy(text)
            .trim_end_matches('\0')
            .trim()
            .to_string()
    }

    let path: Vec<u16> = program.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut handle = 0u32;
    // SAFETY: `path` 以 0 结尾。
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut handle) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0u8; size as usize];
    // SAFETY: `block` 有 `size` 字节。
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, block.as_mut_ptr().cast()) } == 0 {
        return None;
    }

    // 字符串表按语言和代码页命名，取资源里列的第一个；没有就用最常见的英文 Unicode。
    let table = query(&block, r"\VarFileInfo\Translation")
        .filter(|(_, length)| *length >= 4)
        .map(|(pointer, _)| {
            // SAFETY: 至少 4 字节：语言和代码页各一个 u16。
            let pair = unsafe { std::slice::from_raw_parts(pointer.cast::<u16>(), 2) };
            format!("{:04x}{:04x}", pair[0], pair[1])
        })
        .unwrap_or_else(|| "040904b0".to_string());

    let fixed = query(&block, r"\")
        .filter(|(_, length)| *length as usize >= std::mem::size_of::<VS_FIXEDFILEINFO>())
        .map(|(pointer, _)| {
            // SAFETY: 根块就是一个 VS_FIXEDFILEINFO，长度刚核对过；按字节读，不要求对齐。
            let info = unsafe { std::ptr::read_unaligned(pointer.cast::<VS_FIXEDFILEINFO>()) };
            (
                (info.dwProductVersionMS >> 16) as u16,
                (info.dwProductVersionMS & 0xffff) as u16,
                (info.dwProductVersionLS >> 16) as u16,
            )
        });

    Some(VersionResource {
        product_name: string(&block, &table, "ProductName"),
        product_version: string(&block, &table, "ProductVersion"),
        file_description: string(&block, &table, "FileDescription"),
        fixed,
    })
}

/// 别的平台先不读版本（v1 只在 Windows 上跑），状态栏写 shell 名。
#[cfg(not(windows))]
fn read_version(_: &std::path::Path) -> Option<VersionResource> {
    None
}

#[cfg(test)]
mod tests {
    use super::{LocalShellInfo, VersionResource, describe};

    fn resource(product: &str, version: &str, description: &str) -> VersionResource {
        VersionResource {
            product_name: product.into(),
            product_version: version.into(),
            file_description: description.into(),
            fixed: Some((10, 0, 26100)),
        }
    }

    const WINDOWS: &str = "Microsoft® Windows® Operating System";

    #[test]
    fn powershell_7_says_its_product_and_version() {
        let pwsh = resource("PowerShell", "7.5.2 SHA: 0123abcd", "pwsh");
        assert_eq!(describe("pwsh", &pwsh).as_deref(), Some("PowerShell 7.5.2"));
    }

    #[test]
    fn what_comes_with_windows_says_its_description() {
        let powershell = resource(WINDOWS, "10.0.26100.8972", "Windows PowerShell");
        assert_eq!(
            describe("powershell", &powershell).as_deref(),
            Some("Windows PowerShell 5.1")
        );
        let cmd = resource(WINDOWS, "10.0.26100.1", "Windows Command Processor");
        assert_eq!(
            describe("cmd", &cmd).as_deref(),
            Some("Windows Command Processor 10.0.26100")
        );
    }

    #[test]
    fn without_strings_the_fixed_version_and_the_name_do() {
        let bare = VersionResource {
            fixed: Some((2, 45, 1)),
            ..VersionResource::default()
        };
        assert_eq!(describe("bash", &bare).as_deref(), Some("bash 2.45.1"));
        assert_eq!(describe("bash", &VersionResource::default()), None);
    }

    #[test]
    fn the_label_is_the_version_or_else_the_name() {
        assert_eq!(
            LocalShellInfo::new("pwsh", Some("PowerShell 7.5.2")).label(),
            "PowerShell 7.5.2"
        );
        assert_eq!(LocalShellInfo::new("bash", None).label(), "bash");
    }

    /// 本机真实的 shell：Windows 上一定有 cmd.exe，读得出版本资源。
    #[cfg(windows)]
    #[test]
    fn the_command_prompt_of_this_computer_has_a_version() {
        let info = LocalShellInfo::detect(&crate::LocalShell::new("cmd.exe"));
        assert_eq!(info.name, "cmd");
        let version = info.version.expect("cmd.exe has a version resource");
        assert!(version.chars().any(|c| c.is_ascii_digit()), "{version}");
    }
}
