//! Windows 上把标志嵌进可执行文件当图标：资源管理器和任务栏显示它，
//! GPUI 也把图标资源 1 交给自己的窗口。其他平台的图标来自各自的打包方式。
// 来自 since2006/shell-rs @ b2a307a: build.rs（产品名改为 MicaTerm，去掉 OriginalFilename）

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // `rust_i18n::i18n!` 编译时读 locales/，但它不会告诉 Cargo。
    println!("cargo:rerun-if-changed=locales");
    println!("cargo:rerun-if-changed=assets/logo/micaterm.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/logo/micaterm.ico")
            .set("ProductName", "MicaTerm")
            .set("FileDescription", "MicaTerm");
        if let Err(error) = resource.compile() {
            panic!("cannot embed the icon: {error}");
        }
    }
}
