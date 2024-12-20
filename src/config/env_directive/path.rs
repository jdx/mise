use crate::config::env_directive::EnvResults;
use crate::result;
use std::path::PathBuf;

impl EnvResults {
    pub fn path(
        ctx: &mut tera::Context,
        tera: &mut tera::Tera,
        r: &mut EnvResults,
        paths: &mut Vec<(PathBuf, PathBuf)>,
        source: PathBuf,
        input: String,
    ) -> result::Result<()> {
        // trace!("resolve: input_str: {:#?}", input_str);
        // trace!(
        //     "resolve: normal: input: {:?}, input.to_string(): {:?}",
        //     &input,
        //     input.to_string_lossy().as_ref()
        // );
        let s = r.parse_template(ctx, tera, &source, &input)?;
        // trace!("resolve: s: {:?}", &s);
        paths.push((s.into(), source));
        Ok(())
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::config::env_directive::{EnvDirective, EnvResolveOptions};
    use crate::env_diff::EnvMap;
    use crate::tera::BASE_CONTEXT;
    use crate::test::replace_path;
    use insta::assert_debug_snapshot;
    use test_log::test;

    #[test]
    fn test_env_path() {
        let mut env = EnvMap::new();
        env.insert("A".to_string(), "1".to_string());
        env.insert("B".to_string(), "2".to_string());
        let results = EnvResults::resolve(
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
        .unwrap();
        assert_debug_snapshot!(
            results.env_paths.into_iter().map(|p| replace_path(&p.display().to_string())).collect::<Vec<_>>(),
            @r#"
        [
            "/path/1",
            "/path/2",
            "~/foo/1",
            "~/cwd/rel/1",
            "~/cwd/rel2/2",
        ]
        "#
        );
    }
}
