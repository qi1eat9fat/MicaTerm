// 来自 since2006/shell-rs @ b2a307a: src/host/store.rs
// 裁剪：只留命令片段（工单 19）要的写穿方法和秘密写入；分组、书签、端口转发、项目终端、同步、
// 外部链接、私钥目录都去掉。主机和凭据的写穿方法由工单 11、14 照上游补进 `settled!`。

//! 主机数据的 GPUI 实体：窗口壳建一次，各面板和对话框观察它。[`HostData`] 持有数据并写穿到
//! 数据库；这里把每次改动的结果说出来——通知观察者、写不进去时发 [`HostStoreEvent::PersistFailed`]，
//! 保险库的写入放到后台执行器上跑。

use std::ops::{Deref, DerefMut};

use gpui_kit::{Context, EventEmitter, SharedString};

use super::{Failure, HostData, HostEvent, SecretError, SecretJob, SnippetDraft, SnippetId};
use crate::i18n::t;

/// 主机和随主机保存的一切（见 [`HostData`]）的唯一来源，各面板和对话框共用、观察它。
///
/// 改数据的方法都带 `cx`：改完（写穿到数据库）就通知观察者，把没保存下来的报成 `PersistFailed`。
/// 只读的方法和数据自己的 `*_unnotified` 经 `Deref` 直接用。
pub struct HostStore {
    data: HostData,
}

/// 除了「变了」之外，存储要告诉窗口壳的事。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostStoreEvent {
    /// 改动留在内存里了，但没保存下来：数据库或保险库拒绝了。已经写成给用户看的话。
    PersistFailed(SharedString),
}

impl EventEmitter<HostStoreEvent> for HostStore {}

impl Deref for HostStore {
    type Target = HostData;

    fn deref(&self) -> &HostData {
        &self.data
    }
}

impl DerefMut for HostStore {
    fn deref_mut(&mut self) -> &mut HostData {
        &mut self.data
    }
}

/// 数据的改动方法，每个都多带一个 `cx`，改完把结果说出来。
macro_rules! settled {
    ($(fn $name:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $(
            pub fn $name(&mut self, $($arg: $ty,)* cx: &mut Context<Self>) $(-> $ret)? {
                let result = self.data.$name($($arg),*);
                self.settle(cx);
                result
            }
        )*
    };
}

impl HostStore {
    pub fn new(data: HostData) -> Self {
        Self { data }
    }

    /// 把数据做了什么说出来：没保存下来的报成 `PersistFailed`，有变化就通知一次；
    /// 排着的保险库写入放到后台去跑。
    pub fn settle(&mut self, cx: &mut Context<Self>) {
        for job in self.data.take_secret_jobs() {
            self.run_secret_job(job, cx);
        }
        let mut changed = false;
        for event in self.data.take_events() {
            match event {
                HostEvent::Failed(failure) => {
                    cx.emit(HostStoreEvent::PersistFailed(describe_failure(&failure)));
                }
                HostEvent::Changed => changed = true,
            }
        }
        if changed {
            cx.notify();
        }
    }

    settled! {
        fn insert_snippet(draft: SnippetDraft) -> SnippetId;
        fn update_snippet(id: SnippetId, draft: SnippetDraft) -> bool;
        fn remove_snippet(id: SnippetId) -> bool;
    }

    /// 在后台执行器上写保险库（调用会阻塞），写完把结果说出来。
    fn run_secret_job(&self, job: SecretJob, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { job.run() })
                .await;
            this.update(cx, |store, cx| {
                store.data.secret_done(outcome);
                store.settle(cx);
            })
            .ok();
        })
        .detach();
    }
}

/// 没保存下来的改动，写成给用户看的话。
fn describe_failure(failure: &Failure) -> SharedString {
    match failure {
        Failure::NotSaved { error, .. } => t!("host.store.not_saved", error = error),
        Failure::Secret { error, .. } => {
            let error = match error {
                SecretError::Unavailable => t!("host.store.secrets_unavailable"),
                SecretError::Locked => t!("host.store.secrets_locked"),
                SecretError::Store(error) => error.clone().into(),
            };
            t!("host.store.secret_not_saved", error = error)
        }
    }
}
