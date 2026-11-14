//! 单元测试共用的东西。
// 来自 since2006/shell-rs @ b2a307a: src/testing.rs（只留 `no_forks`）

use std::sync::{Mutex, MutexGuard};

static FORKS: Mutex<()> = Mutex::new(());

/// 起子进程时拿着它。
///
/// macOS 上建套接字和标记「不交给子进程」是两步；另一个测试在这中间起的子进程会拿到一份副本，
/// 测试关掉的监听套接字照样能连上。拿着它起子进程，两件事就错开了。
#[allow(dead_code)]
pub fn no_forks() -> MutexGuard<'static, ()> {
    FORKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
