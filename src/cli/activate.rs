use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::env::PATH_KEY;
use crate::file::touch_dir;
use crate::path_env::PathEnv;
use crate::shell::{ActivateOptions, ActivatePrelude, Shell, ShellType, get_shell};
use crate::toolset::env_cache::CachedEnv;
use crate::{dirs, env};
use eyre::Result;
use itertools::Itertools;

/// Initializes mise in the current shell session
///
/// This should go into your shell's rc file or login shell.
/// Otherwise, it will only take effect in the current session.
/// (e.g. ~/.zshrc, ~/.zprofile, ~/.zshenv, ~/.bashrc, ~/.bash_profile, ~/.profile, ~/.config/fish/config.fish, or $PROFILE for powershell)
///
/// Typically, this can be added with something like the following:
///
///     echo 'eval "$(mise activate zsh)"' >> ~/.zshrc
///
/// However, this requires that "mise" is in your PATH. If it is not, you need to
/// specify the full path like this:
///
///     echo 'eval "$(/path/to/mise activate zsh)"' >> ~/.zshrc
///
/// Customize status output with `status` settings.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Activate {
    /// Shell type to generate the script for
    #[clap()]
    shell_type: Option<ShellType>,

    /// Suppress non-error messages
    #[clap(long, short)]
    quiet: bool,

    /// Shell type to generate the script for
    #[clap(long, short, hide = true)]
    shell: Option<ShellType>,

    /// Do not automatically call hook-env
    ///
    /// This can be helpful for debugging mise. If you run `eval "$(mise activate --no-hook-env)"`, then
    /// you can call `mise hook-env` manually which will output the env vars to stdout without actually
    /// modifying the environment. That way you can do things like `mise hook-env --trace` to get more
    /// information or just see the values that hook-env is outputting.
    #[clap(long)]
    no_hook_env: bool,

    /// Use shims instead of modifying PATH
    /// Effectively the same as:
    ///
    ///     PATH="$HOME/.local/share/mise/shims:$PATH"
    ///
    /// `mise activate --shims` does not support all the features of `mise activate`.
    /// See https://mise.jdx.dev/dev-tools/shims.html#shims-vs-path for more information
    #[clap(long, verbatim_doc_comment)]
    shims: bool,

    /// Show "mise: <PLUGIN>@<VERSION>" message when changing directories
    #[clap(long, hide = true)]
    status: bool,
}

impl Activate {
    pub fn run(self) -> Result<()> {
        let shell = get_shell(self.shell_type.or(self.shell))
            .expect("no shell provided. Run `mise activate zsh` or similar");

        // touch ROOT to allow hook-env to run
        let _ = touch_dir(&dirs::DATA);

        let mise_bin = if cfg!(target_os = "linux") {
            // linux dereferences symlinks, so use argv0 instead
            PathBuf::from(&*env::ARGV0)
        } else {
            env::MISE_BIN.clone()
        };
        match self.shims {
            true => self.activate_shims(shell.as_ref(), &mise_bin)?,
            false => self.activate(shell.as_ref(), &mise_bin)?,
        }

        Ok(())
    }

    fn activate_shims(&self, shell: &dyn Shell, mise_bin: &Path) -> std::io::Result<()> {
        let exe_dir = mise_bin.parent().unwrap();
        let mut prelude = vec![];
        if let Some(p) = self.prepend_path(exe_dir) {
            prelude.push(p);
        }
        if let Some(p) = self.prepend_path(&dirs::SHIMS) {
            prelude.push(p);
        }
        miseprint!("{}", shell.format_activate_prelude(&prelude))?;
        Ok(())
    }

    fn activate(&self, shell: &dyn Shell, mise_bin: &Path) -> std::io::Result<()> {
        let mut prelude = vec![];
        if let Some(set_path) = remove_shims()? {
            prelude.push(set_path);
        }
        let exe_dir = mise_bin.parent().unwrap();
        let mut flags = vec![];
        if self.quiet {
            flags.push(" --quiet");
        }
        if self.status {
            flags.push(" --status");
        }
        if let Some(prepend_path) = self.prepend_path(exe_dir) {
            prelude.push(prepend_path);
        }

        // Generate encryption key for env cache if caching is enabled
        // This key is session-scoped and lost when the shell closes
        if Settings::get().env_cache {
            let key = CachedEnv::ensure_encryption_key();
            prelude.push(ActivatePrelude::SetEnv(
                "__MISE_ENV_CACHE_KEY".to_string(),
                key,
            ));
        }

        miseprint!(
            "{}",
            shell.activate(ActivateOptions {
                exe: mise_bin.to_path_buf(),
                flags: flags.join(""),
                no_hook_env: self.no_hook_env,
                prelude,
            })
        )?;
        Ok(())
    }

    fn prepend_path(&self, p: &Path) -> Option<ActivatePrelude> {
        if is_dir_not_in_nix(p) && !is_dir_in_path(p) && !p.is_relative() {
            Some(ActivatePrelude::PrependEnv(
                PATH_KEY.to_string(),
                p.to_string_lossy().to_string(),
            ))
        } else {
            None
        }
    }
}

fn remove_shims() -> std::io::Result<Option<ActivatePrelude>> {
    // When not_found_auto_install is enabled, preserve shims in PATH so they can
    // trigger auto-install for tools that aren't installed yet
    if Settings::get().not_found_auto_install {
        return Ok(None);
    }

    let shims = dirs::SHIMS
        .canonicalize()
        .unwrap_or(dirs::SHIMS.to_path_buf());
    if env::PATH
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .contains(&shims)
    {
        let path_env = PathEnv::from_iter(env::PATH.clone());
        // PathEnv automatically removes the shims directory
        let path = path_env.join().to_string_lossy().to_string();
        Ok(Some(ActivatePrelude::SetEnv(PATH_KEY.to_string(), path)))
    } else {
        Ok(None)
    }
}

fn is_dir_in_path(dir: &Path) -> bool {
    let dir = dir.canonicalize().unwrap_or(dir.to_path_buf());
    env::PATH
        .clone()
        .into_iter()
        .any(|p| p.canonicalize().unwrap_or(p) == dir)
}

fn is_dir_not_in_nix(dir: &Path) -> bool {
    !dir.canonicalize()
        .unwrap_or(dir.to_path_buf())
        .starts_with("/nix/")
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>eval "$(mise activate bash)"</bold>
    $ <bold>eval "$(mise activate zsh)"</bold>
    $ <bold>mise activate fish | source</bold>
    $ <bold>execx($(mise activate xonsh))</bold>
    $ <bold>(&mise activate pwsh) | Out-String | Invoke-Expression</bold>
"#
);
