use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancellationReason {
    UserCancel,
    Interrupt,
    SiblingError,
    BackgroundStop,
}

#[derive(Clone, Debug)]
pub struct CancellationToken {
    inner: Arc<TokenInner>,
}

#[derive(Debug)]
struct TokenInner {
    cancelled: AtomicBool,
    reason: Mutex<Option<CancellationReason>>,
    parent: Mutex<Option<Weak<TokenInner>>>,
    children: Mutex<Vec<Weak<TokenInner>>>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TokenInner {
                cancelled: AtomicBool::new(false),
                reason: Mutex::new(None),
                parent: Mutex::new(None),
                children: Mutex::new(Vec::new()),
            }),
        }
    }

    /// 创建 child token——parent cancel 传播到 child，child cancel 不影响 parent。
    /// 线程安全：注册和取消检查在同一个 Mutex 临界区内完成，消除竞态。
    pub fn child_token(&self) -> CancellationToken {
        let child = CancellationToken::new();
        *child.inner.parent.lock().unwrap() = Some(Arc::downgrade(&self.inner));
        let mut children = self.inner.children.lock().unwrap();
        children.push(Arc::downgrade(&child.inner));
        self.compact_children_locked(&mut children);
        let parent_was_cancelled = self.inner.cancelled.load(Ordering::SeqCst);
        drop(children);

        if parent_was_cancelled {
            child.cancel_with_optional_reason(self.reason());
        }
        child
    }

    pub fn cancel(&self) {
        self.cancel_with_reason(CancellationReason::UserCancel);
    }

    pub fn cancel_with_reason(&self, reason: CancellationReason) {
        self.cancel_with_optional_reason(Some(reason));
    }

    pub fn reason(&self) -> Option<CancellationReason> {
        *self
            .inner
            .reason
            .lock()
            .expect("cancellation reason mutex poisoned")
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    fn cancel_with_optional_reason(&self, reason: Option<CancellationReason>) {
        if self.inner.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }

        *self
            .inner
            .reason
            .lock()
            .expect("cancellation reason mutex poisoned") = reason;

        self.propagate_to_children(reason);
        self.detach_from_parent();
    }

    fn propagate_to_children(&self, reason: Option<CancellationReason>) {
        let child_inners: Vec<Arc<TokenInner>> = {
            let mut children = self.inner.children.lock().unwrap();
            let live_children = children.iter().filter_map(|weak| weak.upgrade()).collect();
            self.compact_children_locked(&mut children);
            live_children
        };

        for child_inner in child_inners {
            let child_token = CancellationToken { inner: child_inner };
            child_token.cancel_with_optional_reason(reason);
        }
    }

    fn compact_children_locked(&self, children: &mut Vec<Weak<TokenInner>>) {
        children.retain(|weak| weak.strong_count() > 0);
    }

    fn detach_from_parent(&self) {
        let parent = self
            .inner
            .parent
            .lock()
            .expect("cancellation parent mutex poisoned")
            .clone();
        let Some(parent) = parent else {
            return;
        };
        let Some(parent_inner) = parent.upgrade() else {
            return;
        };

        let mut children = parent_inner.children.lock().unwrap();
        children.retain(|weak| {
            if weak.strong_count() == 0 {
                return false;
            }
            weak.upgrade()
                .map(|child_inner| !Arc::ptr_eq(&child_inner, &self.inner))
                .unwrap_or(false)
        });
    }

    #[doc(hidden)]
    pub fn compact_children_for_test(&self) {
        let mut children = self.inner.children.lock().unwrap();
        self.compact_children_locked(&mut children);
    }

    #[doc(hidden)]
    pub fn debug_child_count(&self) -> usize {
        self.inner.children.lock().unwrap().len()
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_cancel_propagates_to_child() {
        let parent = CancellationToken::new();
        let child = parent.child_token();

        assert!(!parent.is_cancelled());
        assert!(!child.is_cancelled());

        parent.cancel();

        assert!(parent.is_cancelled());
        assert!(child.is_cancelled(), "child should be cancelled when parent is cancelled");
        assert_eq!(parent.reason(), Some(CancellationReason::UserCancel));
        assert_eq!(child.reason(), Some(CancellationReason::UserCancel));
    }

    #[test]
    fn child_cancel_does_not_propagate_to_parent() {
        let parent = CancellationToken::new();
        let child = parent.child_token();

        child.cancel_with_reason(CancellationReason::SiblingError);

        assert!(child.is_cancelled());
        assert!(!parent.is_cancelled(), "parent should NOT be cancelled when child is cancelled");
        assert_eq!(child.reason(), Some(CancellationReason::SiblingError));
        assert_eq!(parent.reason(), None);
        assert_eq!(parent.debug_child_count(), 0);
    }

    #[test]
    fn parent_cancel_propagates_to_grandchild() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        let grandchild = child.child_token();

        parent.cancel_with_reason(CancellationReason::Interrupt);

        assert!(parent.is_cancelled());
        assert!(child.is_cancelled(), "child should be cancelled");
        assert!(grandchild.is_cancelled(), "grandchild should be cancelled via cascade");
        assert_eq!(child.reason(), Some(CancellationReason::Interrupt));
        assert_eq!(grandchild.reason(), Some(CancellationReason::Interrupt));
    }

    #[test]
    fn child_token_from_already_cancelled_parent_is_immediately_cancelled() {
        let parent = CancellationToken::new();
        parent.cancel_with_reason(CancellationReason::Interrupt);

        let child = parent.child_token();

        assert!(child.is_cancelled(), "child created from cancelled parent should be immediately cancelled");
        assert_eq!(child.reason(), Some(CancellationReason::Interrupt));
    }

    #[test]
    fn abandoned_child_does_not_affect_parent_cancel() {
        let parent = CancellationToken::new();

        {
            let _child = parent.child_token();
        }

        parent.compact_children_for_test();
        parent.cancel();

        assert!(parent.is_cancelled(), "parent should still be cancellable after child is dropped");
    }

    #[test]
    fn child_token_race_with_cancel_is_safe() {
        use std::sync::Barrier;

        let iterations = 1000;
        for _ in 0..iterations {
            let parent = Arc::new(CancellationToken::new());
            let barrier = Arc::new(Barrier::new(2));
            let child_holder: Arc<Mutex<Option<CancellationToken>>> = Arc::new(Mutex::new(None));

            let p = parent.clone();
            let b = barrier.clone();
            let h = child_holder.clone();

            let t1 = std::thread::spawn(move || {
                b.wait();
                *h.lock().unwrap() = Some(p.child_token());
            });

            let p2 = parent.clone();
            let b2 = barrier.clone();
            let t2 = std::thread::spawn(move || {
                b2.wait();
                p2.cancel_with_reason(CancellationReason::Interrupt);
            });

            t1.join().unwrap();
            t2.join().unwrap();

            let child = child_holder.lock().unwrap().take().unwrap();
            assert!(
                child.is_cancelled(),
                "child created concurrently with parent.cancel() must end up cancelled"
            );
            assert_eq!(child.reason(), Some(CancellationReason::Interrupt));
        }
    }
}
