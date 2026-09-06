use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, eyre};
use console::style;
use path_absolutize::Absolutize;

use crate::backend::unalias_backend;
use crate::file::{make_symlink, remove_all};
use crate::{dirs, file};

/// Symlink a plugin into mise
///
/// This is used for developing a plugin.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "ln",
    verbatim_doc_comment,
    example(r###"mise plugins link my-tool ./mise-my-tool"###),
    example(
        r###"mise plugins link ./mise-my-tool"###,
        help = r###"Alternative: infer the name "my-tool""###
    ),
    example(
        r###"mise ls-remote my-tool"###,
        help = r###"List versions through the linked plugin"###
    )
)]
pub(super) struct PluginsLink {
    /// The name of the plugin
    /// e.g.: cmake, poetry
    #[usage(verbatim_doc_comment)]
    name: String,

    /// The local path to the plugin
    /// e.g.: ./vfox-cmake
    #[usage(value_hint = ValueHint::DirPath, verbatim_doc_comment)]
    dir: Option<PathBuf>,

    /// Overwrite existing plugin
    #[usage(long, short = 'f')]
    force: bool,
}

impl PluginsLink {
    pub(super) async fn run(self) -> Result<()> {
        let (name, path) = match self.dir {
            Some(path) => (self.name, path),
            None => {
                let path = PathBuf::from(PathBuf::from(&self.name).absolutize()?);
                let name = get_name_from_path(&path);
                (name, path)
            }
        };
        let name = unalias_backend(&name);
        let path = path.absolutize()?;
        let symlink = dirs::PLUGINS.join(name);
        if symlink.exists() {
            if self.force {
                remove_all(&symlink)?;
            } else {
                return Err(eyre!(
                    "plugin {} already exists, use --force to overwrite",
                    style(&name).blue().for_stderr()
                ));
            }
        }
        file::create_dir_all(*dirs::PLUGINS)?;
        make_symlink(&path, &symlink)?;

        Ok(())
    }
}

fn get_name_from_path(path: &Path) -> String {
    let name = path.file_name().unwrap().to_str().unwrap();
    let name = name.strip_prefix("asdf-").unwrap_or(name);
    let name = name.strip_prefix("rtx-").unwrap_or(name);
    let name = name.strip_prefix("mise-").unwrap_or(name);
    let name = name.strip_prefix("vfox-").unwrap_or(name);
    unalias_backend(name).to_string()
}
