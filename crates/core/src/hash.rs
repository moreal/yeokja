use xxhash_rust::xxh64::xxh64;

const SEED: u64 = 0;
const SENTINEL: u64 = 0;

pub fn content_hash(content: &str) -> u64 {
    xxh64(content.as_bytes(), SEED)
}

pub fn context_hash(prev_source_hash: Option<u64>, next_source_hash: Option<u64>) -> u64 {
    let prev = prev_source_hash.unwrap_or(SENTINEL);
    let next = next_source_hash.unwrap_or(SENTINEL);
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&prev.to_le_bytes());
    buf[8..].copy_from_slice(&next.to_le_bytes());
    xxh64(&buf, SEED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_deterministic() {
        let h1 = content_hash("hello world");
        let h2 = content_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn content_hash_differs_for_different_input() {
        let h1 = content_hash("hello");
        let h2 = content_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn context_hash_uses_sentinel_at_boundaries() {
        let h_start = context_hash(None, Some(42));
        let h_end = context_hash(Some(42), None);
        let h_mid = context_hash(Some(1), Some(42));
        assert_ne!(h_start, h_end);
        assert_ne!(h_start, h_mid);
    }

    #[test]
    fn context_hash_deterministic() {
        let h1 = context_hash(Some(1), Some(2));
        let h2 = context_hash(Some(1), Some(2));
        assert_eq!(h1, h2);
    }
}
