use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::env::PATH_KEY;
use crate::file;
use crate::file::{canonicalize_or_self, touch_dir};
use crate::shell::{
    ActivateOptions, ActivatePrelude, EXAMPLE_SHELL, PortablePath, Shell, ShellType, home_suffix,
    require_shell,
};
use crate::toolset::env_cache::CachedEnv;
use crate::{dirs, env};
use eyre::Result;

/// Print the script to activate mise in an interactive shell
///
/// Evaluate this command's output with the syntax for your shell; running it alone
/// only prints the script. Activation updates tools and environment variables as
/// this shell changes directories.
///
/// Add the appropriate example below once to your interactive startup file:
/// ~/.bashrc for Bash, ~/.zshrc for Zsh, ~/.config/fish/config.fish for Fish,
/// or $PROFILE for PowerShell. See the getting-started guide for other shells.
///
/// The mise executable must be on PATH before that line runs. Otherwise use its
/// absolute path, for example `eval "$(~/.local/bin/mise activate zsh)"`.
///
/// Use `mise exec -- command` for scripts and CI that do not need interactive hooks.
/// Customize status output with the `status` settings.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(r#"eval "$(mise activate bash)""#, help = "Activate mise in Bash."),
    example(r#"eval "$(mise activate zsh)""#, help = "Activate mise in Zsh."),
    example("mise activate fish | source", help = "Activate mise in Fish."),
    example("execx($(mise activate xonsh))", help = "Activate mise in Xonsh."),
    example(
        "(&mise activate pwsh) | Out-String | Invoke-Expression",
        help = "Activate mise in PowerShell."
    )
)]
pub(crate) struct Activate {
    /// Shell type to generate the script for
    #[usage(value_enum)]
    shell_type: Option<ShellType>,

    /// Suppress non-error messages
    #[usage(long, short)]
    quiet: bool,

    /// Shell type to generate the script for
    #[usage(long, short, hide = true, value_enum)]
    shell: Option<ShellType>,

    /// Do not automatically call hook-env
    ///
    /// This can be helpful for debugging mise. If you run `eval "$(mise activate --no-hook-env)"`, then
    /// you can call `mise hook-env` manually which will output the env vars to stdout without actually
    /// modifying the environment. That way you can do things like `mise hook-env --trace` to get more
    /// information or just see the values that hook-env is outputting.
    #[usage(long)]
    no_hook_env: bool,

    /// Use shims instead of modifying PATH
    ///
    /// Effectively the same as:
    ///
    ///     PATH="$HOME/.local/share/mise/shims:$PATH"
    ///
    /// `mise activate --shims` does not support all the features of `mise activate`.
    /// See https://mise.jdx.dev/dev-tools/shims.html#shims-vs-path for more information
    #[usage(long, verbatim_doc_comment)]
    shims: bool,

    /// Show "mise: <TOOL>@<VERSION>" message when changing directories
    #[usage(long, hide = true)]
    status: bool,

    /// Bake absolute paths into the script (1), or emit a portable script
    /// (0, default) resolving mise via PATH and home-relative dirs through a
    /// __MISE_HOME header ($HOME, then $USERPROFILE, then ~)
    #[usage(long, default = "0", choices("0", "1"))]
    hardcoded_binary_paths: String,
}

impl Activate {
    pub(crate) fn run(self) -> Result<()> {
        let shell = require_shell(
            self.shell_type.or(self.shell),
            &format!("Name the shell: `mise activate {EXAMPLE_SHELL}`."),
        )?;

        // touch ROOT to allow hook-env to run
        let _ = touch_dir(&dirs::DATA);

        let mise_bin = self.mise_bin(shell.as_ref());
        match self.shims {
            true => self.activate_shims(shell.as_ref(), &mise_bin)?,
            false => self.activate(shell.as_ref(), &mise_bin)?,
        }

        Ok(())
    }

    fn mise_bin(&self, shell: &dyn Shell) -> PathBuf {
        if self.is_portable() && !shell.prefer_absolute_exe() {
            // Portable scripts invoke bare `mise` so no host-specific path
            // is baked in; the real executable's directory is still
            // positioned portably below for sessions where it is not on PATH.
            PathBuf::from("mise")
        } else {
            Self::real_mise_bin()
        }
    }

    /// Absolute path to the running executable, used for PATH setup and —
    /// on shells that cannot bypass wrapper functions — for invocation.
    fn real_mise_bin() -> PathBuf {
        if cfg!(target_os = "linux") {
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
        }
    }

    /// The real executable's directory, when it is an absolute non-nix path.
    fn real_exe_dir() -> Option<PathBuf> {
        let dir = Self::real_mise_bin();
        let dir = dir.parent()?;
        if dir.is_relative() || !is_dir_not_in_nix(dir) {
            return None;
        }
        Some(dir.to_path_buf())
    }

    /// Home-relative suffix of the real executable's directory, if it lives
    /// under `$HOME`. No generation-time PATH gate: a portable snapshot must
    /// account for the runtime PATH.
    fn portable_exe_dir_suffix() -> Option<String> {
        home_suffix(&Self::real_exe_dir()?, &env::HOME)
    }

    fn is_portable(&self) -> bool {
        self.hardcoded_binary_paths != "1"
    }

    /// Home header plus one idempotent runtime PATH block for `front_order`
    /// (highest precedence first). `None` when no dir is home-relative so
    /// callers fall back to absolute preludes. Dirs outside `$HOME` render
    /// as absolute paths inside the block; emission itself is unconditional
    /// so a saved snapshot works wherever it is sourced.
    fn portable_path_preludes(
        shell: &dyn Shell,
        front_order: &[&Path],
    ) -> Option<Vec<ActivatePrelude>> {
        let home = env::HOME.clone();
        let entries: Vec<PortablePath> = front_order
            .iter()
            .filter(|d| is_dir_not_in_nix(d) && !d.is_relative())
            .map(|d| PortablePath {
                home_suffix: home_suffix(d, &home),
                absolute: d.to_string_lossy().to_string(),
            })
            .collect();
        if !entries.iter().any(|e| e.home_suffix.is_some()) {
            return None;
        }
        Some(vec![
            ActivatePrelude::Raw(shell.render_portable_home_init()),
            ActivatePrelude::Raw(shell.render_portable_path_block(&PATH_KEY, &entries)),
        ])
    }

    fn activate_shims(&self, shell: &dyn Shell, mise_bin: &Path) -> std::io::Result<()> {
        let user_shims = dirs::shims();
        let system_shims = dirs::system_shims();
        let mut shim_dirs = vec![user_shims];
        if system_shims.is_dir() && !file::storage_paths_eq(&shim_dirs[0], &system_shims) {
            shim_dirs.push(system_shims);
        }
        // In portable mode the generated command may be bare `mise`, so PATH
        // setup uses the real executable's directory; hardcoded mode already
        // carries the absolute path in `mise_bin`.
        let real_exe_dir = self.is_portable().then(Self::real_exe_dir).flatten();
        let exe_dir: &Path = real_exe_dir
            .as_deref()
            .unwrap_or_else(|| mise_bin.parent().unwrap());
        let mut prelude = vec![];
        // Portable (default): one unconditional runtime block so a saved
        // snapshot works where the runtime PATH differs; the block itself
        // skips when already positioned, so re-sourcing does not grow PATH.
        // The exe dir rides last (lowest precedence) so shims keep
        // precedence — and so a cargo-installed mise never shadows its own
        // shims with sibling binaries sharing its directory.
        if self.is_portable() {
            let mut front_order: Vec<&Path> = vec![];
            if dirs::COMMAND_WRAPPERS.is_dir() {
                front_order.push(dirs::COMMAND_WRAPPERS.as_path());
            }
            front_order.extend(shim_dirs.iter().map(PathBuf::as_path));
            let exe_home = Self::portable_exe_dir_suffix().is_some();
            if exe_home && let Some(exe) = real_exe_dir.as_deref() {
                front_order.push(exe);
            }
            if let Some(portable) = Self::portable_path_preludes(shell, &front_order) {
                prelude.extend(portable);
                miseprint!("{}", shell.format_activate_prelude(&prelude))?;
                return Ok(());
            }
            // Nothing home-relative at all: fall through to the absolute
            // preludes below as the explicit fallback. An exe dir outside
            // $HOME is covered there by the guarded absolute prepend; the
            // portable default never bakes absolute exe paths.
        }
        // The shims dir is always (move-)prepended so it stays at the front of PATH
        // even when activation is re-sourced (e.g. VS Code terminals) — see #8757.
        // The mise executable's own dir only needs to be present so `mise` is
        // callable, so it uses the guarded prepend: this avoids re-prepending (and
        // thereby reordering) a system dir such as /usr/bin that is already in PATH
        // for deb/rpm installs, which would otherwise move it ahead of
        // /usr/local/bin (#10264).
        let prepended_exe_dir = if let Some(p) = self.prepend_path(exe_dir) {
            prelude.push(p);
            true
        } else {
            false
        };
        let has_command_wrappers = dirs::COMMAND_WRAPPERS.is_dir();
        let mut dispatch_dirs = shim_dirs.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        if has_command_wrappers {
            dispatch_dirs.insert(0, dirs::COMMAND_WRAPPERS.as_path());
        }
        let dispatch_dirs_already_first = are_dirs_first_in_paths(&env::PATH, &dispatch_dirs);
        if shell.supports_move_path() || prepended_exe_dir || !dispatch_dirs_already_first {
            // Prepend in reverse order so user shims retain precedence over system
            // shims in shells where each operation inserts at the front.
            let mut path_changed = prepended_exe_dir;
            for shims_dir in shim_dirs.iter().rev() {
                if let Some(p) = self.shims_prepend_path(shell, shims_dir, path_changed) {
                    prelude.push(p);
                    path_changed = true;
                }
            }
            if has_command_wrappers
                && let Some(p) = self.shims_prepend_path(shell, &dirs::COMMAND_WRAPPERS, true)
            {
                prelude.push(p);
            }
        }
        miseprint!("{}", shell.format_activate_prelude(&prelude))?;
        Ok(())
    }

    fn activate(&self, shell: &dyn Shell, mise_bin: &Path) -> std::io::Result<()> {
        let mut prelude = vec![];
        // Preserve the user's PATH before adding the shim boundary.
        // Portable snapshots snapshot at runtime before prepends run; the
        // templates snapshot after preludes, which would otherwise make
        // `mise deactivate` restore mise's own shim farms.
        if self.is_portable() {
            prelude.push(ActivatePrelude::Raw(shell.render_orig_path_init()));
        } else if env::__MISE_ORIG_PATH.is_none() {
            let path = std::env::join_paths(&*env::PATH)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
            prelude.push(ActivatePrelude::Set(
                "__MISE_ORIG_PATH".to_string(),
                path.to_string_lossy().to_string(),
            ));
        }
        let wants_shims_positioned =
            Settings::get().activate_shims && Settings::get().not_found_auto_install;
        // Portable (default): one unconditional runtime block covering the
        // shims and — when home-relative — the real exe dir last, so shims
        // keep precedence. The block skips itself at runtime when already
        // positioned, so re-sourcing does not grow PATH.
        let real_exe_dir = self.is_portable().then(Self::real_exe_dir).flatten();
        let exe_home = Self::portable_exe_dir_suffix().is_some();
        let mut positioned_portably = false;
        if self.is_portable() && wants_shims_positioned {
            let user_shims = dirs::shims();
            let system_shims = dirs::system_shims();
            let mut front_order: Vec<&Path> = vec![user_shims.as_path()];
            if system_shims.is_dir() && !file::storage_paths_eq(front_order[0], &system_shims) {
                front_order.push(system_shims.as_path());
            }
            if exe_home && let Some(exe) = real_exe_dir.as_deref() {
                front_order.push(exe);
            }
            // user_shims/system_shims/real_exe_dir outlive the call below.
            if let Some(portable) = Self::portable_path_preludes(shell, &front_order) {
                prelude.extend(portable);
                positioned_portably = true;
            }
        }
        if !positioned_portably {
            let shim_path_update = if wants_shims_positioned {
                position_shims_before_path()?
            } else {
                remove_shims_from_path()?
            };
            if let Some(set_path) = shim_path_update {
                prelude.push(set_path);
            }
        }
        let exe_dir: &Path = real_exe_dir
            .as_deref()
            .unwrap_or_else(|| mise_bin.parent().unwrap());
        let mut flags = vec![];
        if self.quiet {
            flags.push(" --quiet".to_string());
        }
        if self.status {
            flags.push(" --status".to_string());
        }
        flags.extend(forwarded_logging_flags(&env::ARGS.read().unwrap()));
        if self.is_portable() {
            // Home-relative exe dirs ride inside the block above when shims
            // are positioned; with shims off they get their own block here —
            // after the baked remove above, whose Set would clobber prepends
            // before it. Outside $HOME the portable default relies on PATH
            // instead of baking an absolute path (system installs live on
            // PATH already); `--hardcoded-binary-paths=1` restores the
            // guarded absolute prepend.
            if exe_home
                && !wants_shims_positioned
                && let Some(exe) = real_exe_dir.as_deref()
                && let Some(portable) =
                    Self::portable_path_preludes(shell, std::slice::from_ref(&exe))
            {
                prelude.extend(portable);
            }
        } else if let Some(prepend_path) = self.prepend_path(exe_dir) {
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

    /// Used by activate_shims for the shims directory. Shells with native path
    /// deduplication move the existing entry to the front. Other shells prepend
    /// only when the shims are not already first, which preserves precedence
    /// without growing PATH on every re-source. If an earlier prelude changes
    /// PATH, prepend again so the shims remain first after that change.
    fn shims_prepend_path(
        &self,
        shell: &dyn Shell,
        p: &Path,
        path_changed_before: bool,
    ) -> Option<ActivatePrelude> {
        if !is_dir_not_in_nix(p) || p.is_relative() {
            return None;
        }
        if shell.supports_move_path() {
            Some(ActivatePrelude::MovePrepend(
                PATH_KEY.to_string(),
                p.to_string_lossy().to_string(),
            ))
        } else if should_prepend_shims(&env::PATH, p, path_changed_before) {
            Some(ActivatePrelude::Prepend(
                PATH_KEY.to_string(),
                p.to_string_lossy().to_string(),
            ))
        } else {
            None
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

fn position_shims_before_path() -> std::io::Result<Option<ActivatePrelude>> {
    let user_shims = dirs::shims();
    let system_shims = dirs::system_shims();
    let mut path = vec![user_shims];
    if system_shims.is_dir() && !file::storage_paths_eq(&path[0], &system_shims) {
        path.push(system_shims);
    }
    path.extend(
        env::PATH
            .iter()
            .filter(|path| !file::is_mise_shims_dir(path))
            .cloned(),
    );
    let path = std::env::join_paths(path)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
    Ok(Some(ActivatePrelude::Set(
        PATH_KEY.to_string(),
        path.to_string_lossy().to_string(),
    )))
}

fn remove_shims_from_path() -> std::io::Result<Option<ActivatePrelude>> {
    if !env::PATH.iter().any(|path| file::is_mise_shims_dir(path)) {
        return Ok(None);
    }
    let path = std::env::join_paths(
        env::PATH
            .iter()
            .filter(|path| !file::is_mise_shims_dir(path)),
    )
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
    Ok(Some(ActivatePrelude::Set(
        PATH_KEY.to_string(),
        path.to_string_lossy().to_string(),
    )))
}

fn is_dir_in_path(dir: &Path) -> bool {
    let dir = canonicalize_or_self(dir);
    env::PATH
        .clone()
        .into_iter()
        .any(|p| canonicalize_or_self(&p) == dir)
}

fn should_prepend_shims(paths: &[PathBuf], dir: &Path, path_changed_before: bool) -> bool {
    path_changed_before || !is_dir_first_in_paths(paths, dir)
}

fn is_dir_first_in_paths(paths: &[PathBuf], dir: &Path) -> bool {
    let dir = canonicalize_or_self(dir);
    paths
        .first()
        .is_some_and(|p| canonicalize_or_self(p) == dir)
}

fn are_dirs_first_in_paths(paths: &[PathBuf], dirs: &[&Path]) -> bool {
    paths.len() >= dirs.len()
        && paths
            .iter()
            .zip(dirs)
            .all(|(path, dir)| canonicalize_or_self(path) == canonicalize_or_self(dir))
}

fn is_dir_not_in_nix(dir: &Path) -> bool {
    !canonicalize_or_self(dir).starts_with("/nix/")
}

#[cfg(test)]
mod tests {
    use super::{
        Activate, are_dirs_first_in_paths, forwarded_logging_flags, is_dir_first_in_paths,
        should_prepend_shims,
    };
    use std::path::PathBuf;

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

    fn activate_with(hardcoded_binary_paths: &str) -> Activate {
        Activate {
            shell_type: None,
            quiet: false,
            shell: None,
            no_hook_env: false,
            shims: false,
            status: false,
            hardcoded_binary_paths: hardcoded_binary_paths.to_string(),
        }
    }

    #[test]
    fn portable_mise_bin_by_default() {
        use crate::shell::ShellType;
        let shell = ShellType::Bash.as_shell();
        assert_eq!(
            activate_with("0").mise_bin(shell.as_ref()),
            PathBuf::from("mise")
        );
    }

    #[test]
    fn hardcoded_mise_bin_is_absolute() {
        use crate::shell::ShellType;
        let shell = ShellType::Bash.as_shell();
        assert!(activate_with("1").mise_bin(shell.as_ref()).is_absolute());
    }

    /// Pwsh keeps the absolute executable: `& 'mise'` inside the `mise`
    /// wrapper function would resolve to the function itself and recurse
    /// until call depth overflow (functions outrank PATH entries, unlike
    /// bash's `command mise` or nu's `^"mise"`).
    #[test]
    fn pwsh_keeps_the_absolute_exe_in_portable_mode() {
        use crate::shell::ShellType;
        let shell = ShellType::Pwsh.as_shell();
        assert!(activate_with("0").mise_bin(shell.as_ref()).is_absolute());
    }

    /// One header plus one idempotent runtime block; zero churn: the
    /// generation-time home appears nowhere no matter how many dirs are
    /// positioned.
    #[test]
    fn portable_preludes_reference_one_home_header() {
        use crate::shell::ShellType;
        use std::path::Path;
        let home = crate::env::HOME.clone();
        let home_str = home.to_string_lossy().to_string();
        let shims = home.join(".local/share/mise/shims");
        let wrappers = home.join(".local/share/mise/command-wrappers/bin");
        let shell = ShellType::Bash.as_shell();
        let preludes = Activate::portable_path_preludes(
            shell.as_ref(),
            &[wrappers.as_path(), shims.as_path()],
        )
        .expect("home-relative dirs render portably");
        assert_eq!(preludes.len(), 2);
        let rendered = shell.format_activate_prelude(&preludes);
        assert_eq!(rendered.matches(&home_str).count(), 0, "{rendered}");
        assert!(
            rendered.contains("$__MISE_HOME/.local/share/mise/shims"),
            "{rendered}"
        );
        // The block guards itself at runtime so re-sourcing is a no-op.
        assert!(rendered.contains("if [["), "{rendered}");
        assert!(rendered.contains("fi\n"), "{rendered}");
        // Wrappers keep precedence over shims in the final PATH.
        let wrappers_pos = rendered
            .find(".local/share/mise/command-wrappers/bin")
            .unwrap();
        let shims_pos = rendered.find(".local/share/mise/shims").unwrap();
        assert!(wrappers_pos < shims_pos, "{rendered}");
        // The header resolves at runtime: HOME, then USERPROFILE, then ~.
        // (Spelled ${HOME} in bash, $HOME in fish, $env.HOME in nu, etc.)
        assert!(rendered.contains("HOME"), "{rendered}");
        assert!(rendered.contains("USERPROFILE"), "{rendered}");
        // Outside $HOME there is nothing to centralize: no header at all.
        let outside_only = if cfg!(windows) {
            Path::new("C:/ProgramData/mise/shims")
        } else {
            Path::new("/usr/local/share/mise/shims")
        };
        assert!(Activate::portable_path_preludes(shell.as_ref(), &[outside_only]).is_none());
    }

    /// A system dir outside $HOME still rides along as an absolute path
    /// inside the block — otherwise its commands would vanish from PATH.
    #[test]
    fn portable_preludes_keep_outside_home_dirs_absolute() {
        use crate::shell::ShellType;
        use std::path::Path;
        let home = crate::env::HOME.clone();
        let shims = home.join(".local/share/mise/shims");
        let shell = ShellType::Bash.as_shell();
        // `/usr/local/...` is not absolute on Windows (no drive prefix), so
        // it would be filtered as relative there; use a drive-absolute path.
        let outside = if cfg!(windows) {
            "C:/ProgramData/mise/shims"
        } else {
            "/usr/local/share/mise/shims"
        };
        let preludes = Activate::portable_path_preludes(
            shell.as_ref(),
            &[Path::new(outside), shims.as_path()],
        )
        .expect("mixed dirs render portably");
        assert_eq!(preludes.len(), 2);
        let rendered = shell.format_activate_prelude(&preludes);
        assert!(rendered.contains(&format!("\"{outside}\"")), "{rendered}");
        assert!(
            rendered.contains("$__MISE_HOME/.local/share/mise/shims"),
            "{rendered}"
        );
    }

    /// Every shell resolves home at runtime (HOME, then USERPROFILE, then ~)
    /// with zero generation-time churn baked in.
    #[test]
    fn portable_home_init_is_zero_churn_in_every_shell() {
        use crate::shell::ShellType;
        let home_str = crate::env::HOME.to_string_lossy().to_string();
        for shell_type in [
            ShellType::Bash,
            ShellType::Zsh,
            ShellType::Fish,
            ShellType::Nu,
            ShellType::Pwsh,
            ShellType::Xonsh,
            ShellType::Elvish,
        ] {
            let shell = shell_type.as_shell();
            let init = shell.render_portable_home_init();
            assert!(init.contains("HOME"), "{shell_type}: {init}");
            assert!(init.contains("USERPROFILE"), "{shell_type}: {init}");
            assert!(
                !init.contains(&home_str),
                "{shell_type} bakes in generation-time home: {init}"
            );
        }
    }

    #[test]
    fn detects_only_a_matching_first_path_entry() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().to_path_buf();
        let equivalent = target.join(".");
        let other = PathBuf::from("/other");

        assert!(!is_dir_first_in_paths(&[], &target));
        assert!(is_dir_first_in_paths(&[equivalent, other.clone()], &target));
        assert!(!is_dir_first_in_paths(&[other, target.clone()], &target));

        assert!(!should_prepend_shims(
            std::slice::from_ref(&target),
            &target,
            false
        ));
        assert!(should_prepend_shims(
            std::slice::from_ref(&target),
            &target,
            true
        ));
    }

    #[test]
    fn detects_an_existing_dispatch_prefix() {
        let root = tempfile::tempdir().unwrap();
        let wrappers = root.path().join("wrappers");
        let shims = root.path().join("shims");
        let other = root.path().join("other");

        assert!(are_dirs_first_in_paths(
            &[wrappers.clone(), shims.clone(), other.clone()],
            &[&wrappers, &shims]
        ));
        assert!(!are_dirs_first_in_paths(
            &[shims.clone(), wrappers.clone(), other],
            &[&wrappers, &shims]
        ));
        assert!(!are_dirs_first_in_paths(
            std::slice::from_ref(&wrappers),
            &[&wrappers, &shims]
        ));
    }
}
