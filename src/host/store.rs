// 来自 since2006/shell-rs @ b2a307a: src/host/store.rs
// 裁剪：去掉分组、临时连接、外部连接、端口转发、书签、项目终端、片段分类、同步、`keys/` 目录、
// 给外部 CLI 用的 `capture_failures`（Agent CLI 工单 04 加回）；钥匙串换成保险库（`SecretStore`）。

//! 主机数据的 GPUI 外壳：窗口壳建一个，主机栏和各个对话框都观察它。[`HostData`] 持有数据、
//! 写穿数据库；这里把每次改动的结果说出来（通知观察者，写不进去的发 [`HostStoreEvent`]），
//! 并把保险库的读写放到后台执行器上跑。

use std::ops::{Deref, DerefMut};

use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, SharedString, Task};
use zeroize::Zeroizing;

use micaterm_hosts::{
    CredentialDraft, CredentialId, HostData, HostDraft, HostEvent, HostId, HostOs, SecretJob,
    SecretRef, SnippetDraft, SnippetId,
};

use super::describe::ErrorMessage as _;
use crate::i18n::t;

/// 主机、凭据、命令片段（见 [`HostData`]）唯一的真实来源，各面板和对话框共用、观察它。
///
/// 写方法都带 `cx`：改数据（数据写穿数据库），再通知观察者，写不进去的发 `PersistFailed`。
/// 只读的方法和数据的 `*_unnotified` 经 `Deref` 取得。外壳上同名带 `cx` 的写方法优先被找到，
/// 所以绕不过 `settle`；新加写方法要加进下面的 `settled!` 列表。
pub struct HostStore {
    data: HostData,
    /// [`Self::capture_failures`] 期间，写不进去的话也收在这里。
    captured: Option<Vec<SharedString>>,
}

/// 除了普通的变更通知以外，主机数据要告诉窗口壳的事。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostStoreEvent {
    /// 改动在内存里生效了，但数据库或保险库没收下。已经写成给用户看的话。
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

/// 数据的写方法，各带一个 `cx`，调完 `settle`。
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
        Self {
            data,
            captured: None,
        }
    }

    /// 跑 `change`，顺带收下这期间写不进数据库的话（照样发 `PersistFailed`）：外部 CLI 要把它们
    /// 告诉 Agent。
    pub fn capture_failures<R>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Self, &mut Context<Self>) -> R,
    ) -> (R, Vec<SharedString>) {
        self.captured = Some(Vec::new());
        let result = change(self, cx);
        (result, self.captured.take().unwrap_or_default())
    }

    /// 把数据做了什么说出来：写不进去的发 `PersistFailed`，有改动就通知一次；
    /// 排着的保险库写入放到后台去跑。
    pub fn settle(&mut self, cx: &mut Context<Self>) {
        for job in self.data.take_secret_jobs() {
            self.run_secret_job(job, cx).detach();
        }
        let mut changed = false;
        for event in self.data.take_events() {
            match event {
                HostEvent::Failed(failure) => {
                    let message = failure.message();
                    if let Some(captured) = &mut self.captured {
                        captured.push(message.clone());
                    }
                    cx.emit(HostStoreEvent::PersistFailed(message));
                }
                HostEvent::Changed => changed = true,
            }
        }
        if changed {
            cx.notify();
        }
    }

    settled! {
        fn insert(draft: HostDraft) -> HostId;
        fn update(id: HostId, draft: HostDraft) -> bool;
        fn remove(id: HostId) -> bool;
        fn move_host(id: HostId, drop: micaterm_hosts::HostDrop) -> bool;
        fn insert_credential(draft: CredentialDraft) -> CredentialId;
        fn update_credential(id: CredentialId, draft: CredentialDraft) -> bool;
        fn remove_credential(id: CredentialId) -> Vec<HostId>;
        fn set_host_os(id: HostId, os: Option<HostOs>);
        fn save_secret(secret: SecretRef, value: Option<Zeroizing<String>>);
        fn insert_snippet(draft: SnippetDraft) -> SnippetId;
        fn update_snippet(id: SnippetId, draft: SnippetDraft) -> bool;
        fn remove_snippet(id: SnippetId) -> bool;
        fn set_host_probe(id: HostId, probe_os: bool) -> bool;
        fn set_host_hostname(id: HostId, hostname: Option<SharedString>) -> bool;
    }

    /// 复制一台主机，名字叫「<原名> 副本」，放在原主机后面。保险库里的密码按主机 id 存，
    /// 副本是新主机，不带密码（用户 2026-11-04 决定）。
    pub fn duplicate(&mut self, id: HostId, cx: &mut Context<Self>) -> Option<HostId> {
        let name = t!("host.store.copy_name", name = self.host(id)?.name);
        let copy = self.data.duplicate(id, name);
        self.settle(cx);
        copy
    }

    /// 主机自己的登录密码在保险库里的条目，没有这台主机时是 `None`。
    pub fn host_password(&self, id: HostId) -> Option<SecretRef> {
        self.host(id).map(|host| host.password_secret())
    }

    /// 在后台读一条秘密。读不出（锁着、坏了）和没存过一样是 `None`：调用方只拿它预填表单。
    pub fn read_secret(
        &self,
        secret: SecretRef,
        cx: &mut Context<Self>,
    ) -> Task<Option<Zeroizing<String>>> {
        let secrets = self.data.secrets();
        cx.background_spawn(async move { secrets.get(&secret).ok().flatten() })
    }

    /// `store` 的 [`Self::read_secret`]，交给秘密输入框（`vault::SecretField`）在后台读回原来存的。
    pub fn reader(
        store: &Entity<Self>,
    ) -> impl Fn(SecretRef, &mut App) -> Task<Option<Zeroizing<String>>> + 'static {
        let store = store.clone();
        move |secret, cx| store.update(cx, |store, cx| store.read_secret(secret, cx))
    }

    /// [`HostData::save_secret`]，给要知道结果的调用方：失败也已经作为 `PersistFailed` 报告过。
    pub fn write_secret(
        &mut self,
        secret: SecretRef,
        value: Option<Zeroizing<String>>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), SharedString>> {
        let job = self.data.secret_job(secret, value);
        self.run_secret_job(job, cx)
    }

    /// 在后台执行器上跑一次保险库写入，跑完把结果交回数据。
    fn run_secret_job(
        &self,
        job: SecretJob,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), SharedString>> {
        cx.spawn(async move |this, cx| {
            let outcome = cx.background_spawn(async move { job.run() }).await;
            let failed = outcome.failure().map(|failure| failure.message());
            this.update(cx, |store, cx| {
                store.data.secret_done(outcome);
                store.settle(cx);
            })
            .ok();
            failed.map_or(Ok(()), Err)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use gpui_kit::TestAppContext;
    use micaterm_hosts::secrets::InMemorySecretStore;
    use micaterm_hosts::{AuthKind, CredentialKind, HostDatabase, SecretStore as _};

    use super::*;

    fn draft(name: &str) -> HostDraft {
        HostDraft::new(name, "10.0.0.1", 22, "root", AuthKind::Password)
    }

    #[gpui_kit::test]
    fn a_change_the_database_refuses_stands_in_memory_and_is_reported(cx: &mut TestAppContext) {
        let mut data = HostData::load(HostDatabase::in_memory().unwrap()).unwrap();
        // 只在内存里的凭据：数据库不认识它，用它的主机写不进去。
        let credential = data.insert_credential_unnotified(CredentialDraft::new(
            "部署",
            CredentialKind::Password,
            "deploy",
        ));
        let store = cx.new(|_| HostStore::new(data));
        let events = Rc::new(RefCell::new(Vec::new()));
        let _subscription = cx.update(|cx| {
            let events = events.clone();
            cx.subscribe(&store, move |_, event: &HostStoreEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });

        let (kept, failed) = store.update(cx, |store, cx| {
            (
                store.insert(draft("web"), cx),
                store.insert(draft("db").with_credential(credential), cx),
            )
        });
        cx.run_until_parked();

        let events = events.borrow();
        assert_eq!(events.len(), 1, "{events:?}");
        let HostStoreEvent::PersistFailed(message) = &events[0];
        assert!(
            message.starts_with("The new host could not be saved to the local database"),
            "{message}"
        );
        store.read_with(cx, |store, _| {
            assert!(store.host(kept).is_some() && store.host(failed).is_some());
        });
    }

    #[gpui_kit::test]
    fn a_copy_is_named_after_the_original_and_leaves_its_password_behind(cx: &mut TestAppContext) {
        let secrets = Arc::new(InMemorySecretStore::default());
        let store = cx.new(|_| HostStore::new(HostData::seed().with_secrets(secrets.clone())));
        let web01 = store.read_with(cx, |store, _| store.hosts()[0].id);
        let password = store.read_with(cx, |store, _| store.host_password(web01).unwrap());
        secrets.set(&password, "hunter2").unwrap();

        let copy = store.update(cx, |store, cx| store.duplicate(web01, cx).unwrap());

        store.read_with(cx, |store, _| {
            assert_eq!(store.host(copy).unwrap().name.as_ref(), "web-01 copy");
            assert_eq!(store.hosts()[1].id, copy, "right after the original");
            let copied = store.host_password(copy).unwrap();
            assert!(secrets.get(&copied).unwrap().is_none());
        });
    }

    #[gpui_kit::test]
    fn a_vault_that_keeps_nothing_says_so_when_the_write_is_done(cx: &mut TestAppContext) {
        let store = cx.new(|_| HostStore::new(HostData::seed()));
        let secret = store.read_with(cx, |store, _| {
            store.host_password(store.hosts()[0].id).unwrap()
        });
        let write = store.update(cx, |store, cx| {
            store.write_secret(secret, Some(Zeroizing::new("hunter2".into())), cx)
        });
        let failure = cx.foreground_executor.block_test(write).unwrap_err();
        assert!(
            failure.starts_with("The password, passphrase or key could not be written to the vault"),
            "{failure}"
        );
    }
}
