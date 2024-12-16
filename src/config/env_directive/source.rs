use crate::config::env_directive::EnvResults;
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvMap};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

impl EnvResults {
    #[allow(clippy::too_many_arguments)]
    pub fn source(
        ctx: &mut tera::Context,
        tera: &mut tera::Tera,
        env: &mut IndexMap<String, (String, Option<PathBuf>)>,
        r: &mut EnvResults,
        normalize_path: fn(&Path, PathBuf) -> PathBuf,
        source: &Path,
        config_root: &Path,
        env_vars: &EnvMap,
        input: String,
    ) {
        if let Ok(s) = r.parse_template(ctx, tera, source, &input) {
            for p in xx::file::glob(normalize_path(config_root, s.into())).unwrap_or_default() {
                r.env_scripts.push(p.clone());
                let env_diff = EnvDiff::from_bash_script(&p, config_root, env_vars.clone())
                    .unwrap_or_default();
                for p in env_diff.to_patches() {
                    match p {
                        EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                            r.env_remove.remove(&k);
                            env.insert(k.clone(), (v.clone(), Some(source.to_path_buf())));
                        }
                        EnvDiffOperation::Remove(k) => {
                            env.shift_remove(&k);
                            r.env_remove.insert(k);
                        }
                    }
                }
            }
        }
    }
}
