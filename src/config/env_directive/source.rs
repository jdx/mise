use crate::Result;
use crate::config::env_directive::{EnvDirectiveContext, EnvResults};
use crate::env;
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffOptions};
use indexmap::IndexMap;
use std::path::PathBuf;

impl EnvResults {
    pub(super) fn source(
        ctx: &mut EnvDirectiveContext<'_>,
        paths: &mut Vec<(PathBuf, PathBuf)>,
        input: String,
    ) -> Result<IndexMap<PathBuf, IndexMap<String, String>>> {
        // Note: in safe mode `_.source` directives are dropped during env
        // resolution (see EnvResults::resolve), so this is never reached.
        let mut out = IndexMap::new();
        let s = ctx.parse_template(&input)?;
        let orig_path = ctx
            .exec_env
            .get(&*env::PATH_KEY)
            .cloned()
            .unwrap_or_default();
        let mut env_diff_opts = EnvDiffOptions::default();
        env_diff_opts.ignore_keys.shift_remove(&*env::PATH_KEY); // allow modifying PATH
        for p in xx::file::glob(ctx.normalize_path(s.into())).unwrap_or_default() {
            if !p.exists() {
                continue;
            }
            let env = out.entry(p.clone()).or_insert_with(IndexMap::new);
            let env_diff = EnvDiff::from_bash_script(
                &p,
                ctx.config_root,
                ctx.exec_env.clone(),
                &env_diff_opts,
            )?;
            for p in env_diff.to_patches() {
                match p {
                    EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                        if k == *env::PATH_KEY {
                            // `_.source` intentionally supports PATH prepends only. Preserving the
                            // original PATH as an exact suffix lets mise represent the additions in
                            // env_paths, where activation ordering and cleanup are managed. General
                            // PATH edits cannot be reproduced safely by that model.
                            if let Some(new_path) = v.strip_suffix(&orig_path) {
                                for p in env::split_paths(new_path) {
                                    if p.as_os_str().is_empty() {
                                        continue;
                                    }
                                    paths.push((p, ctx.source.to_path_buf()));
                                }
                            }
                        } else {
                            ctx.results.env_remove.remove(&k);
                            env.insert(k.clone(), v.clone());
                        }
                    }
                    EnvDiffOperation::Remove(k) => {
                        env.shift_remove(&k);
                        ctx.results.env_remove.insert(k);
                    }
                }
            }
        }
        Ok(out)
    }
}
