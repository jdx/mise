use crate::config::env_directive::EnvResults;
use crate::env;
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffOptions, EnvMap};
use crate::Result;
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

impl EnvResults {
    #[allow(clippy::too_many_arguments)]
    pub fn source(
        ctx: &mut tera::Context,
        tera: &mut tera::Tera,
        paths: &mut Vec<(PathBuf, PathBuf)>,
        r: &mut EnvResults,
        normalize_path: fn(&Path, PathBuf) -> PathBuf,
        source: &Path,
        config_root: &Path,
        env_vars: &EnvMap,
        input: String,
    ) -> Result<IndexMap<PathBuf, IndexMap<String, String>>> {
        let mut out = IndexMap::new();
        let s = r.parse_template(ctx, tera, source, &input)?;
        let orig_path = env_vars.get(&*env::PATH_KEY).cloned().unwrap_or_default();
        let mut env_diff_opts = EnvDiffOptions::default();
        env_diff_opts.ignore_keys.shift_remove(&*env::PATH_KEY); // allow modifying PATH
        for p in xx::file::glob(normalize_path(config_root, s.into())).unwrap_or_default() {
            if !p.exists() {
                continue;
            }
            let env = out.entry(p.clone()).or_insert_with(IndexMap::new);
            let env_diff =
                EnvDiff::from_bash_script(&p, config_root, env_vars.clone(), &env_diff_opts)?;
            for p in env_diff.to_patches() {
                match p {
                    EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                        if k == *env::PATH_KEY {
                            // TODO: perhaps deal with path removals as well
                            if let Some(new_path) = v.strip_suffix(&orig_path) {
                                for p in env::split_paths(new_path) {
                                    paths.push((p, source.to_path_buf()));
                                }
                            }
                        } else {
                            r.env_remove.remove(&k);
                            env.insert(k.clone(), v.clone());
                        }
                    }
                    EnvDiffOperation::Remove(k) => {
                        env.shift_remove(&k);
                        r.env_remove.insert(k);
                    }
                }
            }
        }
        Ok(out)
    }
}
