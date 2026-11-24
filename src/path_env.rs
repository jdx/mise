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
    async fn test_command_wrapper_dir_leads_entries_ahead_of_the_shims() {
        let _config = Config::get().await.unwrap();
        let shims_dir = dirs::shims();
        let shims = shims_dir.to_str().unwrap();
        let wrappers = dirs::COMMAND_WRAPPERS.to_str().unwrap();
        // `~/.cargo/bin` listed ahead of the shims keeps its lead over the tools mise adds, but
        // not over the wrapper directory, or `[wrappers.cargo]` would never be reached.
        let mut path_env =
            PathEnv::from_iter(["/home/me/.cargo/bin", shims, "/usr/bin"].map(PathBuf::from));
        path_env.add_command_wrappers();
        path_env.add("/tools/node/bin".into());
        assert_eq!(
            path_env.to_string(),
            format!("{wrappers}:/home/me/.cargo/bin:/tools/node/bin:{shims}:/usr/bin")
        );
        assert_eq!(
            path_env.managed_entries(),
            [wrappers, "/tools/node/bin"].map(PathBuf::from)
        );
    }

    #[tokio::test]
    async fn test_add_keeps_the_command_wrapper_dir_in_its_callers_order() {
        let _config = Config::get().await.unwrap();
        let wrappers = dirs::COMMAND_WRAPPERS.to_str().unwrap();
        // An install hook lists the tool's own directories, then the inherited PATH. A wrapper
        // directory inherited from the shell must not get ahead of the tool being installed.
        let mut path_env = PathEnv::new();
        for path in ["/installs/tool/bin", "/home/me/bin", wrappers, "/usr/bin"] {
            path_env.add(path.into());
        }
        assert_eq!(
            path_env.to_string(),
            format!("/installs/tool/bin:/home/me/bin:{wrappers}:/usr/bin")
        );
    }

    #[tokio::test]
    async fn test_from_child_path_keeps_the_command_wrapper_dir_first() {
        let _config = Config::get().await.unwrap();
        let shims_dir = dirs::shims();
        let shims = shims_dir.to_str().unwrap();
        let wrappers = dirs::COMMAND_WRAPPERS.to_str().unwrap();
        // A task adds its `_.path` to the PATH mise built. Without a shims directory there,
        // an added directory goes ahead of every entry but the wrappers.
        let mut path_env =
            PathEnv::from_child_path([wrappers, "/tools/node/bin", "/usr/bin"].map(PathBuf::from));
        path_env.add("/project/task-bin".into());
        assert_eq!(
            path_env.to_string(),
            format!("{wrappers}:/project/task-bin:/tools/node/bin:/usr/bin")
        );

        let mut path_env = PathEnv::from_child_path(
            [wrappers, "/home/me/bin", shims, "/usr/bin"].map(PathBuf::from),
        );
        path_env.add("/project/task-bin".into());
        assert_eq!(
            path_env.to_string(),
            format!("{wrappers}:/home/me/bin:/project/task-bin:{shims}:/usr/bin")
        );

        // A wrapper directory that does not lead the PATH is an inherited entry like any other.
        let mut path_env =
            PathEnv::from_child_path(["/usr/bin", wrappers, "/bin"].map(PathBuf::from));
        path_env.add("/project/task-bin".into());
        assert_eq!(
            path_env.to_string(),
            format!("/project/task-bin:/usr/bin:{wrappers}:/bin")
        );
    }

    #[tokio::test]
    async fn test_command_wrapper_dir_is_not_moved_unless_added() {
        let _config = Config::get().await.unwrap();
        let shims_dir = dirs::shims();
        let shims = shims_dir.to_str().unwrap();
        let wrappers = dirs::COMMAND_WRAPPERS.to_str().unwrap();
        // No wrapper is configured here, so the inherited copy stays where the user has it.
        let mut path_env =
            PathEnv::from_iter(["/home/me/bin", shims, wrappers, "/usr/bin"].map(PathBuf::from));
        path_env.add("/tools/node/bin".into());
        assert_eq!(
            path_env.to_string(),
            format!("/home/me/bin:/tools/node/bin:{shims}:{wrappers}:/usr/bin")
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
