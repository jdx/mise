use crate::config::env_directive::EnvResults;
use crate::result;
use std::path::{Path, PathBuf};

impl EnvResults {
    pub async fn path(
        ctx: &mut tera::Context,
        tera: &mut tera::Tera,
        r: &mut EnvResults,
        source: &Path,
        input: String,
    ) -> result::Result<PathBuf> {
        r.parse_template(ctx, tera, source, &input)
            .map(PathBuf::from)
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::config::{
        Config,
        env_directive::{EnvDirective, EnvResolveOptions},
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
            EnvResolveOptions::default(),
        )
        .await
        .unwrap();
        assert_debug_snapshot!(
            results.env_paths.into_iter().map(|p| replace_path(&p.display().to_string())).collect::<Vec<_>>(),
            @r#"
        [
            "~/foo/1",
            "~/cwd/rel2/2",
            "~/cwd/rel/1",
            "/path/1",
            "/path/2",
        ]
        "#
        );
    }
}
