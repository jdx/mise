use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn hash_to_str<T: Hash>(t: &T) -> String {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    let bytes = s.finish();
    format!("{bytes:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_to_str() {
        assert_eq!(hash_to_str(&"foo"), "3e8b8c44c3ca73b7");
    }
}
