//! 共享的 tokio 运行时：所有 SSH 连接都在这里建、在这里跑会话任务。
//!
//! russh 把一条连接的会话任务 spawn 在调用 `connect` 的那个运行时上；运行时一退出连接就断。
//! 卡片的传输线程只是 `block_on` 进来，自己不建运行时，所以关掉一张卡片不会断掉别的卡片的通道。

use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle, Runtime};

/// 进程里唯一的那个运行时，第一次用到时建好。
pub(super) fn shared() -> Handle {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("micaterm-ssh")
                .enable_all()
                .build()
                .expect("the SSH runtime starts")
        })
        .handle()
        .clone()
}
