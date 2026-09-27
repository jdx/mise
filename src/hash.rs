pub use mise_util::hash::*;

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::*;

    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use sha2::Sha256;
    use std::path::Path;

    #[tokio::test]
    async fn test_hash_to_str() {
        let _config = Config::get().await.unwrap();
        assert_eq!(hash_to_str(&"foo"), "e1b19adfb2e348a2");
    }

    #[tokio::test]
    async fn test_hash_sha256() {
        let _config = Config::get().await.unwrap();
        let path = Path::new(".test-tool-versions");
        let hash = file_hash_prog::<Sha256>(path, None).unwrap();
        assert_snapshot!(hash);
    }
}
