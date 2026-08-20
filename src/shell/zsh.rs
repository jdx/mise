#![allow(unknown_lints)]
use std::fmt::Display;

use indoc::formatdoc;
use shell_escape::unix::escape;

use crate::config::Settings;
use crate::shell::bash::Bash;
use crate::shell::{self, ActivateOptions, Shell};

#[derive(Default)]
pub struct Zsh {}

impl Zsh {}

impl Shell for Zsh {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let flags = opts.flags;

        let exe = escape(exe.to_string_lossy());
        let mut out = String::new();

        out.push_str(&shell::build_deactivation_script(self));

        out.push_str(&self.format_activate_prelude(&opts.prelude));

        // much of this is from direnv
        // https://github.com/direnv/direnv/blob/cb5222442cb9804b1574954999f6073cc636eff0/internal/cmd/shell_zsh.go#L10-L22
        out.push_str(&formatdoc! {r#"
            export MISE_SHELL=zsh
            if [ -z "${{__MISE_ORIG_PATH:-}}" ]; then
              export __MISE_ORIG_PATH="$PATH"
            fi
            export __MISE_ZSH_PRECMD_RUN=0
            export __MISE_ZSH_CHPWD_RAN=0

            mise() {{
              local command
              command="${{1:-}}"
              if [ "$#" = 0 ]; then
                command {exe}
                return
              fi
              shift

              case "$command" in
              deactivate|shell|sh)
                # if argv doesn't contains -h,--help
                if [[ ! " $@ " =~ " --help " ]] && [[ ! " $@ " =~ " -h " ]]; then
                  eval "$(command {exe} "$command" "$@")"
                  return $?
                fi
                ;;
              esac
              command {exe} "$command" "$@"
            }}
        "#});

        if !opts.no_hook_env {
            out.push_str(&formatdoc! {r#"

            autoload -Uz add-zsh-hook
            _mise_hook() {{
              eval "$({exe} hook-env{flags} -s zsh "$@")";
            }}
            _mise_hook_env_state() {{
              # enumerate MISE_* vars with the typeset builtin rather than
              # ${{parameters}}: referencing that special array autoloads the
              # zsh/parameter module via dlopen, which can deadlock under
              # Rosetta in login shells (https://github.com/jdx/mise/discussions/11187)
              local -a keys=(${{(o)${{(f)"$(typeset +m 'MISE_*' 2>/dev/null)"}}##* }})
              if (( ${{#keys}} > 0 )); then
                typeset -p "${{keys[@]}}" 2>/dev/null
              fi
            }}
            _mise_hook_precmd() {{
              if [[ "${{__MISE_ZSH_CHPWD_RAN:-0}}" == "1" ]]; then
                export __MISE_ZSH_CHPWD_RAN=0
                unset __MISE_ZSH_ACTIVATE_PATH
                unset __MISE_ZSH_ACTIVATE_ENV
                return
              fi
              if [[ "${{__MISE_ZSH_PRECMD_RUN:-0}}" == "0" &&
                    "$PATH" == "${{__MISE_ZSH_ACTIVATE_PATH:-}}" &&
                    "$(_mise_hook_env_state)" == "${{__MISE_ZSH_ACTIVATE_ENV:-}}" ]]; then
                export __MISE_ZSH_PRECMD_RUN=1
                unset __MISE_ZSH_ACTIVATE_PATH
                unset __MISE_ZSH_ACTIVATE_ENV
                return
              fi
              unset __MISE_ZSH_ACTIVATE_PATH
              unset __MISE_ZSH_ACTIVATE_ENV
              _mise_hook --reason precmd
            }}
            _mise_hook_chpwd() {{
              export __MISE_ZSH_CHPWD_RAN=1
              _mise_hook --reason chpwd
            }}
            add-zsh-hook precmd _mise_hook_precmd
            add-zsh-hook chpwd _mise_hook_chpwd

            _mise_hook
            export __MISE_ZSH_ACTIVATE_PATH="$PATH"
            export __MISE_ZSH_ACTIVATE_ENV="$(_mise_hook_env_state)"
            "#});
        }
        if Settings::get().not_found_auto_install {
            out.push_str(&formatdoc! {r#"
            if [ -z "${{_mise_cmd_not_found:-}}" ]; then
                _mise_cmd_not_found=1
                [ -n "$(declare -f command_not_found_handler)" ] && eval "${{$(declare -f command_not_found_handler)/command_not_found_handler/_command_not_found_handler}}"

                function command_not_found_handler() {{
                    if [[ "$1" != "mise" && "$1" != "mise-"* ]] && {exe} hook-not-found -s zsh -- "$1"; then
                      # Refresh inline rather than through `_mise_hook`: `--no-hook-env` omits
                      # that definition while still emitting this handler, and without a refresh
                      # "$@" runs before the tool just installed is on PATH. Called with no
                      # arguments `_mise_hook` is this same eval, so one inline call -- fish's
                      # shape -- covers both modes and leaves nothing to keep in sync.
                      #
                      # MISE_SHELL is the gate `deactivate` turns off: it unsets the variable but
                      # leaves this handler registered, and a shell the user deactivated must not
                      # have mise's environment applied back to it.
                      #
                      # `--force` because an install just happened: with `hook_env.cache_ttl` set
                      # and an inherited `__MISE_SESSION`, the TTL fast path returns before the
                      # check that would notice it, and "$@" would fail exactly as before.
                      if [ -n "${{MISE_SHELL:-}}" ]; then
                        eval "$({exe} hook-env{flags} --force -s zsh)"
                      fi
                      "$@"
                    elif [ -n "$(declare -f _command_not_found_handler)" ]; then
                        _command_not_found_handler "$@"
                    else
                        echo "zsh: command not found: $1" >&2
                        return 127
                    fi
                }}
            fi
            "#});
        }

        out
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
        autoload -Uz add-zsh-hook
        add-zsh-hook -d precmd _mise_hook_precmd 2>/dev/null
        add-zsh-hook -d chpwd _mise_hook_chpwd 2>/dev/null
        (( $+functions[_mise_hook_precmd] )) && unset -f _mise_hook_precmd
        (( $+functions[_mise_hook_chpwd] )) && unset -f _mise_hook_chpwd
        (( $+functions[_mise_hook] )) && unset -f _mise_hook
        (( $+functions[_mise_hook_env_state] )) && unset -f _mise_hook_env_state
        (( $+functions[mise] )) && unset -f mise
        unset MISE_SHELL
        unset __MISE_DIFF
        unset __MISE_SESSION
        unset __MISE_ZSH_PRECMD_RUN
        unset __MISE_ZSH_CHPWD_RAN
        unset __MISE_ZSH_ACTIVATE_PATH
        unset __MISE_ZSH_ACTIVATE_ENV
        "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        Bash::default().set_env(k, v)
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        format!("export {k}=\"{v}:${k}\"\n")
    }

    fn unset_env(&self, k: &str) -> String {
        Bash::default().unset_env(k)
    }

    fn set_alias(&self, name: &str, cmd: &str) -> String {
        Bash::default().set_alias(name, cmd)
    }

    fn unset_alias(&self, name: &str) -> String {
        Bash::default().unset_alias(name)
    }
}

impl Display for Zsh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "zsh")
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use std::path::Path;
    use test_log::test;

    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_activate() {
        // Unset __MISE_ORIG_PATH to avoid PATH restoration logic in output
        unsafe {
            std::env::remove_var("__MISE_ORIG_PATH");
            std::env::remove_var("__MISE_DIFF");
        }

        let zsh = Zsh::default();
        let exe = Path::new("/some/dir/mise");
        let opts = ActivateOptions {
            exe: exe.to_path_buf(),
            flags: " --status".into(),
            no_hook_env: false,
            prelude: vec![],
        };
        assert_snapshot!(zsh.activate(opts));
    }

    /// The text of `command_not_found_handler`, which `activate` emits last.
    fn command_not_found_handler(script: &str) -> &str {
        let start = script
            .find("function command_not_found_handler()")
            .expect("activate should emit a command-not-found handler");
        &script[start..]
    }

    /// `--no-hook-env` drops the `_mise_hook` definition but still emits
    /// `command_not_found_handler`, so the handler has to refresh the environment on its
    /// own — otherwise `"$@"` runs before the tool it just installed is on PATH.
    #[test]
    fn test_activate_no_hook_env_refreshes_after_auto_install() {
        let opts = ActivateOptions {
            exe: Path::new("/some/dir/mise").to_path_buf(),
            flags: " --status".into(),
            no_hook_env: true,
            prelude: vec![],
        };
        let script = Zsh::default().activate(opts);

        assert!(!script.contains("_mise_hook() {"));
        assert!(script.contains("hook-not-found -s zsh"));
        // With the definition gone, the only `hook-env` left in the script is this refresh.
        // `--force` so an inherited `__MISE_SESSION` plus `hook_env.cache_ttl` cannot make it
        // exit early on the one call that follows a fresh install.
        assert!(script.contains("hook-env --status --force -s zsh"));
    }

    /// `deactivate` unsets `MISE_SHELL` but leaves this handler registered, so an
    /// ungated refresh would re-apply mise's environment in a shell the user had
    /// deactivated. It is the same gate `_mise_hook` carries in the other shells.
    #[test]
    fn test_activate_command_not_found_refresh_is_gated_on_mise_shell() {
        for no_hook_env in [false, true] {
            let opts = ActivateOptions {
                exe: Path::new("/some/dir/mise").to_path_buf(),
                flags: " --status".into(),
                no_hook_env,
                prelude: vec![],
            };
            let script = Zsh::default().activate(opts);
            let handler = command_not_found_handler(&script);

            let gate = handler
                .find(r#"if [ -n "${MISE_SHELL:-}" ]; then"#)
                .expect("the refresh should be gated on MISE_SHELL");
            let refresh = handler
                .find("hook-env --status --force -s zsh")
                .expect("the handler should refresh the environment after an install");
            assert!(gate < refresh, "the gate has to precede what it guards");
        }
    }

    /// Referencing a `zsh/parameter` special autoloads the module via dlopen, which can
    /// deadlock under Rosetta in login shells (jdx/mise#11187). `_mise_hook_env_state` was
    /// rewritten to avoid exactly that; the handler must not reintroduce it, and under
    /// `--no-hook-env` nothing else in the script would have loaded the module first.
    #[test]
    fn test_activate_command_not_found_avoids_zsh_parameter_module() {
        let opts = ActivateOptions {
            exe: Path::new("/some/dir/mise").to_path_buf(),
            flags: " --status".into(),
            no_hook_env: true,
            prelude: vec![],
        };
        let script = Zsh::default().activate(opts);

        assert!(!command_not_found_handler(&script).contains("$+functions"));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Zsh::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        let sh = Bash::default();
        assert_snapshot!(replace_path(&sh.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    #[test]
    fn test_unset_env() {
        assert_snapshot!(Zsh::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        let deactivate = Zsh::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
