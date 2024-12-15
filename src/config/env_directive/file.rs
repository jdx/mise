use crate::config::env_directive::EnvResults;
use crate::file::display_path;
use crate::Result;
use eyre::{eyre, WrapErr};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

impl EnvResults {
    pub fn file(
        ctx: &mut tera::Context,
        env: &mut IndexMap<String, (String, Option<PathBuf>)>,
        r: &mut EnvResults,
        normalize_path: fn(&Path, PathBuf) -> PathBuf,
        source: &Path,
        config_root: &Path,
        input: PathBuf,
    ) -> Result<()> {
        let s = r.parse_template(ctx, source, input.to_string_lossy().as_ref())?;
        for p in xx::file::glob(normalize_path(config_root, s.into())).unwrap_or_default() {
            r.env_files.push(p.clone());
            let errfn = || eyre!("failed to parse dotenv file: {}", display_path(&p));
            if let Ok(dotenv) = dotenvy::from_path_iter(&p) {
                for item in dotenv {
                    let (k, v) = item.wrap_err_with(errfn)?;
                    r.env_remove.remove(&k);
                    env.insert(k, (v, Some(p.clone())));
                }
            }
        }
        Ok(())
    }
}
