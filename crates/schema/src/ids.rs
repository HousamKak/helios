//! Entity identifiers.
//!
//! Two ID flavours co-exist:
//!   * `EntityId` (alias for `String`) for everything the H schema uses —
//!     base36 timestamp + random suffix, matches `H/types/common.ts:generateId`
//!     so artifactflow project_ids and any imported data stay valid.
//!   * Native `i32` PIDs and other kernel-issued IDs are kept as their own
//!     types in the relevant entity structs.

use std::time::{SystemTime, UNIX_EPOCH};

pub type EntityId = String;

/// Base36 timestamp + 8-char random suffix.
/// Matches the H system's id format so existing project_ids round-trip.
pub fn generate_id() -> EntityId {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let ts_36 = to_base36(ts as u64);
    let rand_suffix = random_suffix(8);
    format!("{ts_36}-{rand_suffix}")
}

fn to_base36(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    const ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while n > 0 {
        out.push(ALPHABET[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("base36 alphabet is ASCII")
}

fn random_suffix(len: usize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    SystemTime::now().hash(&mut h);
    std::process::id().hash(&mut h);
    let mut state = h.finish();
    let mut out = String::with_capacity(len);
    const ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    for _ in 0..len {
        // xorshift64 step
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push(ALPHABET[(state % 36) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_have_expected_shape() {
        let id = generate_id();
        let dash_at = id.find('-').expect("id has a dash separator");
        let (ts, suffix) = id.split_at(dash_at);
        assert!(!ts.is_empty(), "timestamp segment is non-empty");
        assert_eq!(suffix.len(), 9, "suffix is `-` plus 8 chars");
        assert!(ts.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn ids_are_unique_within_burst() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(generate_id()), "duplicate id in 1000-burst");
        }
    }
}
