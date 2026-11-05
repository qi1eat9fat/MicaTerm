//! 存在保险库里的一条秘密的输入框（主机密码、代理密码、凭据的密码和口令共用，M2 code review T1）：
//!
//! - 编辑时在后台读回原来存的，用户还没输就填进去；
//! - 保险库锁着、这条又存过时「锁着」：界面换成「已保存，保险库锁定中」（`ui::form::locked_secret`），
//!   不读也不改；解开后读回来；
//! - 保存时和原来存的比，得出要怎么改（[`SecretField::change`]）。秘密全程是 `Zeroizing<String>`。

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use zeroize::Zeroizing;

use qishell_hosts::SecretRef;

use super::VaultStore;
use crate::ui::form::typed_secret;

/// 在后台读一条秘密（`HostStore::read_secret`）；读不出和没存过一样是 `None`。
pub type ReadSecret<'a> = &'a dyn Fn(SecretRef, &mut App) -> Task<Option<Zeroizing<String>>>;

/// 保存时一条秘密要怎么改：`None` 不动，`Some(None)` 删掉，`Some(Some(..))` 写进去。
pub type SecretChange = Option<Option<Zeroizing<String>>>;

/// 一条秘密的输入框和它原来存的值。
pub struct SecretField {
    input: Entity<InputState>,
    /// 原来存在哪个条目（编辑时）；新建时是 `None`。
    secret: Option<SecretRef>,
    /// 原来存的：`None` 是还没读回来（这时空框不表示「删掉」），`Some(None)` 是没存过。
    saved: Option<Option<Zeroizing<String>>>,
    locked: bool,
    _load: Option<Task<()>>,
}

impl SecretField {
    /// 一个打码的输入框。`secret` 是原来存的条目（新建时 `None`）；锁着又存过时一开始就是锁着的。
    pub fn new(
        secret: Option<SecretRef>,
        placeholder: SharedString,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(placeholder)
        });
        let locked = locked_away(secret.as_ref(), cx);
        Self {
            input,
            secret,
            saved: Some(None),
            locked,
            _load: None,
        }
    }

    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// 原来存的条目。
    pub fn secret(&self) -> Option<&SecretRef> {
        self.secret.as_ref()
    }

    /// 存过、保险库却锁着：显示「已保存，保险库锁定中」，不读也不改。
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// 框里现在的字。
    pub fn typed(&self, cx: &App) -> Zeroizing<String> {
        typed_secret(&self.input, cx)
    }

    /// 保存时要怎么改：和原来存的一样就不动，清空了就删掉。锁着时不动。
    pub fn change(&self, cx: &App) -> SecretChange {
        if self.locked {
            return None;
        }
        let typed = self.typed(cx);
        match &self.saved {
            // 还没读回来：只有真输了东西才写。
            None => self.change_ignoring_saved(cx),
            Some(saved) => match (saved, typed.is_empty()) {
                (None, true) => None,
                (Some(_), true) => Some(None),
                (Some(saved), false) if saved.as_str() == typed.as_str() => None,
                (_, false) => Some(Some(typed)),
            },
        }
    }

    /// 不看原来存的（条目换了，比如代理的地址改了）：输了就写，空着不动。
    pub fn change_ignoring_saved(&self, cx: &App) -> SecretChange {
        if self.locked {
            return None;
        }
        let typed = self.typed(cx);
        (!typed.is_empty()).then_some(Some(typed))
    }

    /// 在后台读回原来存的（锁着、没有条目就不读）。`field` 从视图里找回这个框。
    pub fn load_saved<V: 'static>(
        &mut self,
        read: ReadSecret<'_>,
        window: &mut Window,
        cx: &mut Context<V>,
        field: fn(&mut V) -> &mut SecretField,
    ) {
        let Some(secret) = self.secret.clone().filter(|_| !self.locked) else {
            return;
        };
        self.saved = None;
        let read = read(secret, cx);
        self._load = Some(cx.spawn_in(window, async move |this, cx| {
            let saved = read.await;
            this.update_in(cx, |this, window, cx| {
                field(this).set_saved(saved, window, cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// 保险库变了：锁着的解开了就读回来；又锁上了、框里也没输东西，就换成锁着的那一行。
    pub fn vault_changed<V: 'static>(
        &mut self,
        read: ReadSecret<'_>,
        window: &mut Window,
        cx: &mut Context<V>,
        field: fn(&mut V) -> &mut SecretField,
    ) {
        let locked = locked_away(self.secret.as_ref(), cx);
        if self.locked && !locked {
            self.locked = false;
            self.load_saved(read, window, cx, field);
        } else if locked && !self.locked && self.input.read(cx).value().is_empty() {
            self.locked = true;
        }
    }

    /// 原来存的不算数了（换了一把私钥，原来的口令属于旧的）：当作没存过。
    pub fn forget_saved(&mut self) {
        self.saved = Some(None);
        self._load = None;
    }

    /// 清空框里的字：对话框要关了，明文现在就放掉，不等实体回收。
    pub fn clear(&self, window: &mut Window, cx: &mut App) {
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    /// 读回来了：用户还没输就填进去。
    fn set_saved(&mut self, saved: Option<Zeroizing<String>>, window: &mut Window, cx: &mut App) {
        if self.input.read(cx).value().is_empty()
            && let Some(saved) = &saved
        {
            let value = SharedString::from(saved.as_str().to_owned());
            self.input
                .update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.saved = Some(saved);
    }
}

/// 这条秘密存过、保险库却锁着：读不出来，也不能改。
pub fn locked_away(secret: Option<&SecretRef>, cx: &App) -> bool {
    match (VaultStore::global(cx), secret) {
        (Some(vault), Some(secret)) => {
            let vault = vault.read(cx);
            vault.is_locked() && vault.contains(secret)
        }
        _ => false,
    }
}
