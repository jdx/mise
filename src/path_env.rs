pub use mise_util::path_env::*;

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::dirs;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    use super::*;

    #[tokio::test]
    async fn test_path_env() {
        let _config = Config::get().await.unwrap();
        let shims_dir = dirs::shims();
        let shims = shims_dir.to_str().unwrap();
        let mut path_env = PathEnv::from_iter(
            [
                "/before-1",
                "/before-2",
                "/before-3",
                shims,
                "/after-1",
                "/after-2",
                "/after-3",
            ]
            .map(PathBuf::from),
        );
        path_env.add("/1".into());
        path_env.add("/2".into());
        path_env.add("/3".into());
        assert_eq!(
            path_env.to_string(),
            format!("/before-1:/before-2:/before-3:/1:/2:/3:{shims}:/after-1:/after-2:/after-3")
        );
    }

    #[tokio::test]
    async fn test_path_env_no_mise() {
        let _config = Config::get().await.unwrap();
        let mut path_env = PathEnv::from_iter(
            [
                "/before-1",
                "/before-2",
                "/before-3",
                "/after-1",
                "/after-2",
                "/after-3",
            ]
            .map(PathBuf::from),
        );
        path_env.add("/1".into());
        path_env.add("/2".into());
        path_env.add("/3".into());
        assert_eq!(
            path_env.to_string(),
            "/1:/2:/3:/before-1:/before-2:/before-3:/after-1:/after-2:/after-3"
        );
    }

    #[tokio::test]
    async fn test_path_env_with_colon() {
        let _config = Config::get().await.unwrap();
        let mut path_env = PathEnv::from_iter(["/item1", "/item2"].map(PathBuf::from));
        path_env.add("/1:/2".into());
        assert_eq!(path_env.to_string(), "/1:/2:/item1:/item2");
    }

    #[tokio::test]
    async fn test_managed_entries_include_a_promoted_entry() {
        let _config = Config::get().await.unwrap();
        let shims_dir = dirs::shims();
        let shims = shims_dir.to_str().unwrap();
        let mut path_env =
            PathEnv::from_iter([shims, "/home/me/.cargo/bin", "/usr/bin"].map(PathBuf::from));
        path_env.add("/tools/java/bin".into());
        path_env.add("/home/me/.cargo/bin".into());
        path_env.add("/tools/flutter/bin".into());
        assert_eq!(
            path_env.managed_entries(),
            [
                "/tools/java/bin",
                "/home/me/.cargo/bin",
                "/tools/flutter/bin"
            ]
            .map(PathBuf::from)
        );
        assert_eq!(
            path_env.to_string(),
            format!("/tools/java/bin:/home/me/.cargo/bin:/tools/flutter/bin:{shims}:/usr/bin")
        );
    }

    #[tokio::test]
    async fn test_managed_entries_include_a_marked_entry_in_place() {
        let _config = Config::get().await.unwrap();
        let mut path_env =
            PathEnv::from_iter(["/user/shims", "/system/shims", "/usr/bin"].map(PathBuf::from));
        // mise inserted a system shim directory next to the inherited one.
        path_env.mark_managed("/system/shims".into());
        path_env.add("/tools/node/bin".into());
        assert_eq!(
            path_env.managed_entries(),
            ["/tools/node/bin", "/system/shims"].map(PathBuf::from)
        );
    }
}
