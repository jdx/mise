use std::collections::HashMap;

pub fn get<'a, V>(map: &'a HashMap<String, V>, key: &str) -> Option<&'a V> {
    if let Some(value) = map.get(key) {
        return Some(value);
    }

    #[cfg(windows)]
    {
        map.iter()
            .find_map(|(candidate, value)| equivalent(candidate, key).then_some(value))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub fn contains<V>(map: &HashMap<String, V>, key: &str) -> bool {
    get(map, key).is_some()
}

pub fn insert<V>(map: &mut HashMap<String, V>, key: String, value: V) -> Option<V> {
    #[cfg(windows)]
    let previous = if map.contains_key(&key) {
        None
    } else {
        let existing = map
            .keys()
            .find(|candidate| equivalent(candidate, &key))
            .cloned();
        existing.and_then(|existing| map.remove(&existing))
    };

    #[cfg(not(windows))]
    let previous = None;

    map.insert(key, value).or(previous)
}

pub fn remove<V>(map: &mut HashMap<String, V>, key: &str) -> Option<V> {
    if map.contains_key(key) {
        return map.remove(key);
    }

    #[cfg(windows)]
    {
        let existing = map
            .keys()
            .find(|candidate| equivalent(candidate, key))
            .cloned();
        existing.and_then(|existing| map.remove(&existing))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn equivalent(left: &str, right: &str) -> bool {
    const CSTR_EQUAL: i32 = 2;
    const TRUE: i32 = 1;

    #[link(name = "kernel32")]
    extern "system" {
        fn CompareStringOrdinal(
            left: *const u16,
            left_len: i32,
            right: *const u16,
            right_len: i32,
            ignore_case: i32,
        ) -> i32;
    }

    if left == right {
        return true;
    }

    let left: Vec<_> = left.encode_utf16().collect();
    let right: Vec<_> = right.encode_utf16().collect();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return false;
    };

    // SAFETY: both pointers refer to initialized UTF-16 buffers for the explicit lengths passed
    // to the Windows API, and the function does not retain them.
    unsafe {
        CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, TRUE) == CSTR_EQUAL
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::equivalent;

    #[test]
    fn windows_key_comparison_uses_the_os_case_table() {
        assert!(equivalent("Path", "PATH"));
        assert!(equivalent("\u{00e5}", "\u{00c5}"));
        assert!(!equivalent("Path", "Path2"));
    }
}
