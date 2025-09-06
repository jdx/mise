use crate::env_diff::EnvMap;
use crate::tera::{get_tera, tera_exec};
use std::path::Path;

pub fn build_tera_for_source(source: &Path, current_env: &EnvMap) -> tera::Tera {
    let mut tera = get_tera(source.parent());
    tera.register_function(
        "exec",
        tera_exec(
            source.parent().map(|d| d.to_path_buf()),
            current_env.clone(),
        ),
    );
    tera
}
