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

    /// 创建 child token——parent cancel 传播到 child，child cancel 不影响 parent。
    /// 线程安全：注册和取消检查在同一个 Mutex 临界区内完成，消除竞态。
    pub fn child_token(&self) -> CancellationToken {
        let child = CancellationToken::new();
        // 在 parent 的 children Mutex 内完成注册 + 取消检查，
        // 确保 cancel() 不会在 "检查 is_cancelled" 和 "push child" 之间传播并遗漏这个 child。
        let mut children = self.inner.children.lock().unwrap();
        children.push(Arc::downgrade(&child.inner));
        // 如果 parent 在我们拿到锁之前（或之中）已经被 cancel，
        // cancel() 的 propagate_to_children() 可能已经跑完了旧的 children 快照，
        // 所以这里需要补发一次 cancel。
        if self.inner.cancelled.load(Ordering::SeqCst) {
            drop(children); // 先释放锁再 cancel，避免死锁（child.cancel 不需要 parent 的锁）
            child.cancel();
        }
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

    #[test]
    fn child_token_race_with_cancel_is_safe() {
        // 验证：即使 parent.cancel() 和 parent.child_token() 并发，
        // child 最终一定能观察到 cancelled 状态。
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
                p2.cancel();
            });

            t1.join().unwrap();
            t2.join().unwrap();

            let child = child_holder.lock().unwrap().take().unwrap();
            assert!(
                child.is_cancelled(),
                "child created concurrently with parent.cancel() must end up cancelled"
            );
        }
    }
}
