//! ID generation — sortable, collision-resistant unique identifiers.
//!
//! Format: `{base36_ms}_{random4}` (e.g., "lqk5a8m0_x7f2")
//! - base36 millisecond timestamp: sortable by creation time
//! - 4-char random suffix: prevents collision within the same millisecond

use std::time::{SystemTime, UNIX_EPOCH};

const BASE36_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// Generate a unique, sortable ID.
///
/// Format: `{base36_ms}_{random4}` — approximately 14 characters.
pub fn gen_id() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let ts_part = base36_encode(ms);
    let rand_part = random_base36(4);
    format!("{}_{}", ts_part, rand_part)
}

fn base36_encode(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut chars = Vec::with_capacity(12);
    while n > 0 {
        chars.push(BASE36_CHARS[(n % 36) as usize]);
        n /= 36;
    }
    chars.reverse();
    String::from_utf8(chars).unwrap_or_default()
}

fn random_base36(len: usize) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let state = RandomState::new();
    let mut hasher = state.build_hasher();
    hasher.write_u64(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );
    let mut hash = hasher.finish();

    let mut result = Vec::with_capacity(len);
    for _ in 0..len {
        result.push(BASE36_CHARS[(hash % 36) as usize]);
        hash /= 36;
        if hash == 0 {
            // Re-hash if we run out of bits
            let state2 = RandomState::new();
            let mut h2 = state2.build_hasher();
            h2.write_u64(hash.wrapping_add(result.len() as u64));
            hash = h2.finish();
        }
    }
    String::from_utf8(result).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gen_id_format() {
        let id = gen_id();
        assert!(id.contains('_'), "ID should contain underscore: {}", id);
        let parts: Vec<&str> = id.split('_').collect();
        assert_eq!(parts.len(), 2);
        assert!(!parts[0].is_empty());
        assert_eq!(parts[1].len(), 4);
    }

    #[test]
    fn test_gen_id_uniqueness() {
        let ids: Vec<String> = (0..100).map(|_| gen_id()).collect();
        let mut deduped = ids.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(ids.len(), deduped.len(), "Generated IDs should be unique");
    }

    #[test]
    fn test_base36_encode() {
        assert_eq!(base36_encode(0), "0");
        assert_eq!(base36_encode(35), "z");
        assert_eq!(base36_encode(36), "10");
    }
}
