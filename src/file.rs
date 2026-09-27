pub use mise_util::file::*;

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::path::Path;
    use crate::{dirs, env};
    use itertools::Itertools;

    use super::*;

    #[tokio::test]
    async fn test_find_up() {
        let _config = Config::get().await.unwrap();
        let path = &env::current_dir().unwrap();
        let filenames = vec![".miserc", ".mise.toml", ".test-tool-versions"]
            .into_iter()
            .map(|s| s.to_string())
            .collect_vec();
        let mut find_up = FindUp::new(path, &filenames);
        assert_eq!(
            find_up.next(),
            Some(dirs::HOME.join("cwd/.test-tool-versions"))
        );
        assert_eq!(find_up.next(), Some(dirs::HOME.join(".test-tool-versions")));
    }

    #[tokio::test]
    async fn test_find_up_2() {
        let _config = Config::get().await.unwrap();
        let path = &dirs::HOME.join("fixtures");
        let filenames = vec![".test-tool-versions"];
        let result = find_up(path, &filenames);
        assert_eq!(result, Some(dirs::HOME.join(".test-tool-versions")));
    }

    #[tokio::test]
    async fn test_dir_subdirs() {
        let _config = Config::get().await.unwrap();
        let subdirs = dir_subdirs(&dirs::HOME).unwrap();
        assert!(subdirs.contains("cwd"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_display_path() {
        let _config = Config::get().await.unwrap();
        use std::ops::Deref;
        let path = dirs::HOME.join("cwd");
        assert_eq!(display_path(path), "~/cwd");

        let path = Path::new("/tmp")
            .join(dirs::HOME.deref().strip_prefix("/").unwrap())
            .join("cwd");
        assert_eq!(display_path(&path), path.display().to_string());
    }

    #[tokio::test]
    async fn test_replace_path() {
        let _config = Config::get().await.unwrap();
        assert_eq!(replace_path(Path::new("~/cwd")), dirs::HOME.join("cwd"));
        assert_eq!(replace_path(Path::new("/cwd")), Path::new("/cwd"));
    }
}
