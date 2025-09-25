use versions::{Mess, Versioning};

/// splits a version number into an optional prefix and the remaining version string
pub fn split_version_prefix(version: &str) -> (String, String) {
    version
        .char_indices()
        .find_map(|(i, c)| {
            if c.is_ascii_digit() {
                if i == 0 {
                    return Some(i);
                }
                // If the previous char is a delimiter or 'v', we found a split point.
                let prev_char = version.chars().nth(i - 1).unwrap();
                if ['-', '_', '/', '.', 'v', 'V'].contains(&prev_char) {
                    return Some(i);
                }
            }
            None
        })
        .map_or_else(
            || ("".into(), version.into()),
            |i| {
                let (prefix, version) = version.split_at(i);
                (prefix.into(), version.into())
            },
        )
}

/// split a version number into chunks
/// given v: "1.2-3a4" return ["1", ".2", "-3a4"]
pub fn chunkify_version(v: &str) -> Vec<String> {
    fn chunkify(m: &Mess, sep0: &str, chunks: &mut Vec<String>) {
        for (i, chunk) in m.chunks.iter().enumerate() {
            let sep = if i == 0 { sep0 } else { "." };
            chunks.push(format!("{sep}{chunk}"));
        }
        if let Some((next_sep, next_mess)) = &m.next {
            chunkify(next_mess, next_sep.to_string().as_ref(), chunks)
        }
    }

    let mut chunks = vec![];
    // don't parse "latest", otherwise bump from latest to any version would have one chunk only
    if v != "latest" {
        if let Some(v) = Versioning::new(v) {
            let m = match v {
                Versioning::Ideal(sem_ver) => sem_ver.to_mess(),
                Versioning::General(version) => version.to_mess(),
                Versioning::Complex(mess) => mess,
            };
            chunkify(&m, "", &mut chunks);
        }
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::{chunkify_version, split_version_prefix};

    #[test]
    fn test_split_version_prefix() {
        assert_eq!(split_version_prefix("latest"), ("".into(), "latest".into()));
        assert_eq!(split_version_prefix("v1.2.3"), ("v".into(), "1.2.3".into()));
        assert_eq!(
            split_version_prefix("mountpoint-s3-v1.2.3-5_beta.5"),
            ("mountpoint-s3-v".into(), "1.2.3-5_beta.5".into())
        );
        assert_eq!(
            split_version_prefix("cli/1.2.3"),
            ("cli/".into(), "1.2.3".into())
        );
        assert_eq!(
            split_version_prefix("temurin-17.0.7+7"),
            ("temurin-".into(), "17.0.7+7".into())
        );
        assert_eq!(split_version_prefix("1.2"), ("".into(), "1.2".into()));
        assert_eq!(
            split_version_prefix("2:1.2.1"),
            ("".into(), "2:1.2.1".into())
        );
        assert_eq!(
            split_version_prefix("2025-05-17"),
            ("".into(), "2025-05-17".into())
        );
    }

    #[test]
    fn test_chunkify_version() {
        assert_eq!(chunkify_version("1.2-3a4"), vec!["1", ".2", "-3a4"]);
        assert_eq!(chunkify_version("latest"), Vec::<String>::new());
        assert_eq!(chunkify_version("1.0.0"), vec!["1", ".0", ".0"]);
        assert_eq!(
            chunkify_version("2.3.4-beta"),
            vec!["2", ".3", ".4", "-beta"]
        );
    }
}
