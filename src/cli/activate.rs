use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::env::PATH_KEY;
use crate::file::{canonicalize_cached, canonicalize_or_self, touch_dir};
use crate::path_env::PathEnv;
use crate::shell::{
    ActivateOptions, ActivatePrelude, EXAMPLE_SHELL, Shell, ShellType, require_shell,
};
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

    /// Show "mise: <TOOL>@<VERSION>" message when changing directories
    #[clap(long, hide = true)]
    status: bool,
}

impl Activate {
    pub fn run(self) -> Result<()> {
        let shell = require_shell(
            self.shell_type.or(self.shell),
            &format!("Name the shell: `mise activate {EXAMPLE_SHELL}`."),
        )?;

        // touch ROOT to allow hook-env to run
        let _ = touch_dir(&dirs::DATA);

        let mise_bin = if cfg!(target_os = "linux") {
            // linux dereferences symlinks, so use argv0 instead
            let argv0 = PathBuf::from(&*env::ARGV0);
            let path = if argv0.is_absolute() {
                argv0
            } else {
                which::which(&*env::ARGV0).unwrap_or_else(|_| env::MISE_BIN.clone())
            };
            if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(path))
                    .unwrap_or_else(|_| env::MISE_BIN.clone())
            }
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
        // The shims dir is always (move-)prepended so it stays at the front of PATH
        // even when activation is re-sourced (e.g. VS Code terminals) — see #8757.
        // The mise executable's own dir only needs to be present so `mise` is
        // callable, so it uses the guarded prepend: this avoids re-prepending (and
        // thereby reordering) a system dir such as /usr/bin that is already in PATH
        // for deb/rpm installs, which would otherwise move it ahead of
        // /usr/local/bin (#10264).
        if let Some(p) = self.prepend_path(exe_dir) {
            prelude.push(p);
        }
        if let Some(p) = self.shims_prepend_path(shell, &dirs::SHIMS) {
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
            flags.push(" --quiet".to_string());
        }
        if self.status {
            flags.push(" --status".to_string());
        }
        flags.extend(forwarded_logging_flags(&env::ARGS.read().unwrap()));
        if let Some(prepend_path) = self.prepend_path(exe_dir) {
            prelude.push(prepend_path);
        }

        // Generate encryption key for env cache if caching is enabled
        // This key is session-scoped and lost when the shell closes
        if Settings::get().env_cache {
            let key = CachedEnv::ensure_encryption_key();
            prelude.push(ActivatePrelude::Set(
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
            Some(ActivatePrelude::Prepend(
                PATH_KEY.to_string(),
                p.to_string_lossy().to_string(),
            ))
        } else {
            None
        }
    }

    /// Used by activate_shims for the shims directory. Always prepends the path to
    /// the front, even if already present (accepting a duplicate entry), so the
    /// shims dir wins on re-source. For shells with native path dedup (fish), uses
    /// MovePrepend to reorder without duplicating.
    fn shims_prepend_path(&self, shell: &dyn Shell, p: &Path) -> Option<ActivatePrelude> {
        if !is_dir_not_in_nix(p) || p.is_relative() {
            return None;
        }
        if shell.supports_move_path() {
            Some(ActivatePrelude::MovePrepend(
                PATH_KEY.to_string(),
                p.to_string_lossy().to_string(),
            ))
        } else {
            Some(ActivatePrelude::Prepend(
                PATH_KEY.to_string(),
                p.to_string_lossy().to_string(),
            ))
        }
    }
}

/// Logging flags given to `mise activate` that have to keep applying to every later
/// `hook-env`, since that — not `activate` — is what prints the per-directory output.
///
/// Only `--quiet` reaches [`Activate`] as a field; `--silent` and `--log-level` are global
/// flags on [`crate::cli::Cli`], so they are read back from the argv `Cli::run` recorded.
/// `--quiet` is left to `Activate::quiet` to avoid emitting it twice, and the verbosity
/// flags (`-v`, `--debug`, `--trace`) are deliberately not forwarded — the same split
/// `hook_env::has_preclap_logging_flag` draws between flags that suppress warnings and
/// flags that do not.
///
/// Flags are forwarded in the order they were given, so `hook-env`'s own
/// `overrides_with_all` resolves them exactly as this invocation did.
/// `--log-level <LEVEL>` is normalized to `--log-level=<LEVEL>` so the flag survives as a
/// single word when the shell templates split the flag string.
fn forwarded_logging_flags(args: &[String]) -> Vec<String> {
    let mut flags = vec![];
    let mut remaining = args.iter();
    while let Some(arg) = remaining.next() {
        if arg == "--silent" {
            flags.push(" --silent".to_string());
        } else if let Some(level) = arg.strip_prefix("--log-level=") {
            flags.push(format!(" --log-level={level}"));
        } else if arg == "--log-level"
            && let Some(level) = remaining.next()
        {
            flags.push(format!(" --log-level={level}"));
        }
    }
    flags
}

fn remove_shims() -> std::io::Result<Option<ActivatePrelude>> {
    // When not_found_auto_install is enabled, preserve shims in PATH so they can
    // trigger auto-install for tools that aren't installed yet
    if Settings::get().not_found_auto_install {
        return Ok(None);
    }

    let shims = canonicalize_or_self(&dirs::SHIMS);
    if env::PATH
        .iter()
        .filter_map(|p| canonicalize_cached(p))
        .contains(&shims)
    {
        let path_env = PathEnv::from_iter(env::PATH.clone());
        // PathEnv automatically removes the shims directory. Verbatim, because this PATH
        // goes back into the user's live shell: a duplicate entry the user put there is
        // theirs to keep, and only the shims dir may be dropped here.
        let path = path_env.join_verbatim().to_string_lossy().to_string();
        Ok(Some(ActivatePrelude::Set(PATH_KEY.to_string(), path)))
    } else {
        Ok(None)
    }
}

fn is_dir_in_path(dir: &Path) -> bool {
    let dir = canonicalize_or_self(dir);
    env::PATH
        .clone()
        .into_iter()
        .any(|p| canonicalize_or_self(&p) == dir)
}

fn is_dir_not_in_nix(dir: &Path) -> bool {
    !canonicalize_or_self(dir).starts_with("/nix/")
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

#[cfg(test)]
mod tests {
    use super::forwarded_logging_flags;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn forwards_silent_so_it_reaches_hook_env() {
        assert_eq!(
            forwarded_logging_flags(&args(&["mise", "activate", "bash", "--silent"])),
            vec![" --silent".to_string()]
        );
    }

    #[test]
    fn forwards_a_flag_given_before_the_subcommand_too() {
        assert_eq!(
            forwarded_logging_flags(&args(&["mise", "--silent", "activate", "bash"])),
            vec![" --silent".to_string()]
        );
    }

    #[test]
    fn normalizes_both_log_level_spellings_to_one_word() {
        let separate =
            forwarded_logging_flags(&args(&["mise", "activate", "bash", "--log-level", "error"]));
        let joined =
            forwarded_logging_flags(&args(&["mise", "activate", "bash", "--log-level=error"]));
        assert_eq!(separate, vec![" --log-level=error".to_string()]);
        assert_eq!(separate, joined);
    }

    #[test]
    fn keeps_the_order_so_hook_env_resolves_overrides_the_same_way() {
        assert_eq!(
            forwarded_logging_flags(&args(&[
                "mise",
                "activate",
                "bash",
                "--silent",
                "--log-level=error"
            ])),
            vec![" --silent".to_string(), " --log-level=error".to_string()]
        );
    }

    #[test]
    fn leaves_quiet_to_the_activate_flag_and_skips_verbosity() {
        // `--quiet` is carried by `Activate::quiet`; forwarding it here would duplicate it.
        // `-v`/`--debug`/`--trace` raise the level rather than suppressing warnings.
        for arg in ["-q", "--quiet", "-v", "--debug", "--trace"] {
            assert!(
                forwarded_logging_flags(&args(&["mise", "activate", "bash", arg])).is_empty(),
                "{arg} should not be forwarded"
            );
        }
    }

    #[test]
    fn a_trailing_log_level_without_a_value_is_dropped() {
        assert!(
            forwarded_logging_flags(&args(&["mise", "activate", "bash", "--log-level"])).is_empty()
        );
    }

    #[test]
    fn nothing_is_forwarded_without_a_logging_flag() {
        assert!(forwarded_logging_flags(&args(&["mise", "activate", "bash"])).is_empty());
    }
}
