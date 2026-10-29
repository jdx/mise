use crate::config::env_directive::{EnvDirectiveContext, EnvResults};
use crate::result;
use std::path::PathBuf;

impl EnvResults {
    pub(super) async fn path(
        ctx: &mut EnvDirectiveContext<'_>,
        input: String,
    ) -> result::Result<PathBuf> {
        ctx.parse_template("_.path", &input).map(PathBuf::from)
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::config::{
        Config,
        env_directive::{EnvDirective, EnvResolveOptions, ToolsFilter},
    };
    use crate::env_diff::EnvMap;
    use crate::tera::BASE_CONTEXT;
    use crate::test::replace_path;
    use insta::assert_debug_snapshot;

    #[tokio::test]
    async fn test_env_path() {
        let mut env = EnvMap::new();
        env.insert("A".to_string(), "1".to_string());
        env.insert("B".to_string(), "2".to_string());
        let config = Config::get().await.unwrap();
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &env,
            vec![
                (
                    EnvDirective::Path("/path/1".into(), Default::default()),
                    PathBuf::from("/config"),
                ),
                (
                    EnvDirective::Path("/path/2".into(), Default::default()),
                    PathBuf::from("/config"),
                ),
                (
                    EnvDirective::Path("~/foo/{{ env.A }}".into(), Default::default()),
                    Default::default(),
                ),
                (
                    EnvDirective::Path(
                        "./rel/{{ env.A }}:./rel2/{{env.B}}".into(),
                        Default::default(),
                    ),
                    Default::default(),
                ),
            ],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::NonToolsOnly,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap();
        assert_debug_snapshot!(
            results.env_paths.into_iter().map(|p| replace_path(&p.display().to_string())).collect::<Vec<_>>(),
            @r#"
        [
            "~/foo/1",
            "~/rel2/2",
            "~/rel/1",
            "/path/1",
            "/path/2",
        ]
        "#
        );
    }

    #[tokio::test]
    async fn test_env_path_append() {
        let env = EnvMap::new();
        let config = Config::get().await.unwrap();
        let append = || crate::config::env_directive::EnvDirectiveOptions {
            position: crate::config::env_directive::PathPosition::Append,
            ..Default::default()
        };
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &env,
            vec![
                (
                    EnvDirective::Path("/first".into(), Default::default()),
                    PathBuf::from("/config"),
                ),
                (
                    EnvDirective::Path("/tail/1".into(), append()),
                    PathBuf::from("/config"),
                ),
                (
                    EnvDirective::Path("/tail/2".into(), append()),
                    PathBuf::from("/config"),
                ),
            ],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::NonToolsOnly,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(results.env_paths, vec![PathBuf::from("/first")]);
        assert_eq!(
            results.env_paths_append,
            vec![PathBuf::from("/tail/1"), PathBuf::from("/tail/2")]
        );
    }
}
