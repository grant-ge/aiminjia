//! 跨平台消息去重 helper。每个 connector 在 `start()` 时实例化一个，
//! 入站消息流先经过 `observe(msg_id)`：首次返回 true，重复返回 false。
//! 容量上限简单清空（不做 LRU 因为重连重放窗口短）。

use std::collections::HashSet;

use tokio::sync::RwLock;

/// 默认容量 5000。钉钉/飞书 WebSocket 重连重放最多见过 ~100 条/分钟，
/// 5000 足够覆盖几小时的重连窗口。
const DEFAULT_CAP: usize = 5000;

pub struct MessageDedupSet {
    inner: RwLock<HashSet<String>>,
    cap: usize,
}

impl MessageDedupSet {
    pub fn new(cap: usize) -> Self {
        Self {
            inner: RwLock::new(HashSet::new()),
            cap,
        }
    }

    pub fn with_default_cap() -> Self {
        Self::new(DEFAULT_CAP)
    }

    /// 返回 true 表示**第一次**见过这个 msg_id；false 表示重复。
    /// 空 msg_id 视为"不去重"（永远返回 true）—— 仅用于罕见的协议异常。
    pub async fn observe(&self, msg_id: &str) -> bool {
        if msg_id.is_empty() {
            return true;
        }
        let mut guard = self.inner.write().await;
        if guard.len() >= self.cap {
            guard.clear();
        }
        guard.insert(msg_id.to_string())
    }

    #[cfg(test)]
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn first_observe_returns_true() {
        let s = MessageDedupSet::with_default_cap();
        assert!(s.observe("m1").await);
    }

    #[tokio::test]
    async fn duplicate_observe_returns_false() {
        let s = MessageDedupSet::with_default_cap();
        assert!(s.observe("m1").await);
        assert!(!s.observe("m1").await);
        assert!(!s.observe("m1").await);
    }

    #[tokio::test]
    async fn cap_clears_when_exceeded() {
        let s = MessageDedupSet::new(3);
        assert!(s.observe("a").await);
        assert!(s.observe("b").await);
        assert!(s.observe("c").await);
        assert_eq!(s.len().await, 3);
        assert!(s.observe("d").await);
        assert_eq!(s.len().await, 1);
        assert!(s.observe("a").await);
        assert_eq!(s.len().await, 2);
    }

    #[tokio::test]
    async fn empty_msg_id_is_never_marked_duplicate() {
        let s = MessageDedupSet::with_default_cap();
        assert!(s.observe("").await);
        assert!(s.observe("").await);
        assert_eq!(s.len().await, 0, "empty msg_id must not poison the set");
    }
}
