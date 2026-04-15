use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

#[derive(Clone, Debug)]
pub struct CancellationToken {
    inner: Arc<TokenInner>,
}

#[derive(Debug)]
struct TokenInner {
    cancelled: AtomicBool,
    children: Mutex<Vec<Weak<TokenInner>>>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TokenInner {
                cancelled: AtomicBool::new(false),
                children: Mutex::new(Vec::new()),
            }),
        }
    }

    /// 创建 child token——parent cancel 传播到 child，child cancel 不影响 parent
    pub fn child_token(&self) -> CancellationToken {
        let child = CancellationToken::new();
        if self.is_cancelled() {
            child.cancel();
            return child;
        }
        self.inner.children.lock().unwrap().push(Arc::downgrade(&child.inner));
        child
    }

    pub fn cancel(&self) {
        if self.inner.cancelled.swap(true, Ordering::SeqCst) {
            return; // 已经 cancelled，避免重复传播
        }
        // 递归传播到所有 children
        self.propagate_to_children();
    }

    fn propagate_to_children(&self) {
        let children = self.inner.children.lock().unwrap();
        for weak_child in children.iter() {
            if let Some(child_inner) = weak_child.upgrade() {
                let child_token = CancellationToken { inner: child_inner };
                child_token.cancel(); // 递归——会先 swap 再传播 grandchildren
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
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
    }

    #[test]
    fn child_cancel_does_not_propagate_to_parent() {
        let parent = CancellationToken::new();
        let child = parent.child_token();

        child.cancel();

        assert!(child.is_cancelled());
        assert!(!parent.is_cancelled(), "parent should NOT be cancelled when child is cancelled");
    }

    #[test]
    fn parent_cancel_propagates_to_grandchild() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        let grandchild = child.child_token();

        assert!(!parent.is_cancelled());
        assert!(!child.is_cancelled());
        assert!(!grandchild.is_cancelled());

        parent.cancel();

        assert!(parent.is_cancelled());
        assert!(child.is_cancelled(), "child should be cancelled");
        assert!(grandchild.is_cancelled(), "grandchild should be cancelled via cascade");
    }

    #[test]
    fn child_token_from_already_cancelled_parent_is_immediately_cancelled() {
        let parent = CancellationToken::new();
        parent.cancel();

        let child = parent.child_token();

        assert!(child.is_cancelled(), "child created from cancelled parent should be immediately cancelled");
    }

    #[test]
    fn abandoned_child_does_not_affect_parent_cancel() {
        let parent = CancellationToken::new();

        {
            let _child = parent.child_token();
            // _child is dropped here
        }

        // Parent cancel should succeed even though the child was dropped
        parent.cancel();

        assert!(parent.is_cancelled(), "parent should still be cancellable after child is dropped");
    }
}
