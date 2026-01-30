use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use color_eyre::eyre::ErrReport;
use indoc::formatdoc;
use once_cell::sync::OnceCell;

use crate::env::PATH_KEY;
use crate::path_env::PathEnv;
use crate::{env, file};

pub fn setup() -> color_eyre::Result<PathBuf> {
    static SETUP: OnceCell<PathBuf> = OnceCell::new();
    let path = SETUP.get_or_try_init(|| {
        let path = env::MISE_DATA_DIR.join(".fake-asdf");
        let asdf_bin = path.join("asdf");
        if !asdf_bin.exists() {
            file::create_dir_all(&path)?;
            file::write(
                &asdf_bin,
                formatdoc! {r#"
                #!/bin/sh
                mise asdf "$@"
            "#},
            )?;
            let mut perms = asdf_bin.metadata()?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&asdf_bin, perms)?;
        }
        Ok::<PathBuf, ErrReport>(path)
    })?;

    Ok(path.clone())
}

pub fn get_path_with_fake_asdf() -> String {
    let mut path_env = PathEnv::from_iter(env::split_paths(
        &env::var_os(&*PATH_KEY).unwrap_or_default(),
    ));
    match setup() {
        Ok(fake_asdf_path) => path_env.add(fake_asdf_path),
        Err(e) => warn!("Failed to setup fake asdf: {:#}", e),
    };
    path_env.to_string()
}
