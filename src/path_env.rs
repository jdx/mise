pub(crate) use mise_util::path_env::*;

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
}
