//! Conservative shell elision, inspired by aube-scripts' direct-exec planner.
//! Unknown syntax or execution context always retains the configured shell.

use std::process::Command;

/// Keep duct's capture/redirection and child supervision around the new program.
pub(crate) fn optimize_expression(
    fallback: duct::Expression,
    body: &str,
    env: &crate::env_diff::EnvMap,
    cwd: Option<&std::path::Path>,
    enabled: bool,
) -> duct::Expression {
    let mut context = Command::new("sh");
    context.env_clear().envs(env);
    if let Some(cwd) = cwd {
        context.current_dir(cwd);
    }
    let Some(command) = direct_command(&context, false, body, &[], enabled) else {
        return fallback;
    };
    let expression = duct::cmd(command.get_program(), command.get_args())
        .full_env(command.get_envs().filter_map(|(k, v)| v.map(|v| (k, v))))
        .dir(command.get_current_dir().expect("direct command has cwd"));
    #[cfg(unix)]
    let expression = {
        use std::os::unix::process::CommandExt;
        let arg0 = body.split_ascii_whitespace().next().unwrap().to_owned();
        expression.before_spawn(move |command| {
            command.arg0(&arg0);
            Ok(())
        })
    };
    expression
}

/// Build a replacement before configuring stdio or pre-exec hooks. The caller
/// retains its existing process supervision and output handling.
pub(crate) fn direct_command(
    shell: &Command,
    inherit_env: bool,
    body: &str,
    forwarded: &[String],
    enabled: bool,
) -> Option<Command> {
    #[cfg(unix)]
    {
        unix::direct_command(shell, inherit_env, body, forwarded, enabled)
    }
    #[cfg(not(unix))]
    {
        let _ = (shell, inherit_env, body, forwarded, enabled);
        None
    }
}

#[cfg(unix)]
mod unix {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    // Include builtins with external counterparts: /usr/bin/echo, printf, test,
    // etc. need not behave like the shell's implementation.
    const SHELL_WORDS: &str = ". : [ [[ ]] alias bg bind break builtin caller case cd command compgen complete continue coproc declare dirs disown do done echo elif else enable esac eval exec exit export false fc fg fi for function getopts hash history if in jobs kill let local logout mapfile popd printf pushd pwd read readarray readonly return select set shift shopt source suspend test then time times trap true type typeset ulimit umask unalias unset until wait while { }";

    fn words(body: &str) -> Option<Vec<&str>> {
        if !body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" ._-+/@:,=".contains(&b))
        {
            return None;
        }
        let words: Vec<_> = body.split_ascii_whitespace().collect();
        let program = *words.first()?;
        if program.starts_with('-')
            || program.contains(['/', '='])
            || SHELL_WORDS
                .split_ascii_whitespace()
                .any(|word| word == program)
        {
            return None;
        }
        Some(words)
    }

    fn resolve(program: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
        let dirs: Vec<_> = std::env::split_paths(path).collect();
        if dirs.iter().any(|dir| !dir.is_absolute()) {
            return None;
        }
        for dir in dirs {
            let candidate = dir.join(program);
            match std::fs::metadata(&candidate) {
                Ok(meta) if meta.is_file() => {}
                Ok(_) => return None,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return None,
            }
            nix::unistd::access(&candidate, nix::unistd::AccessFlags::X_OK).ok()?;
            // A shell interprets executable text without a shebang after ENOEXEC.
            // Defer rather than changing that behavior or searching later entries.
            let mut head = [0; 256];
            let n = std::fs::File::open(&candidate).ok()?.read(&mut head).ok()?;
            let head = &head[..n];
            if let Some(shebang) = head.strip_prefix(b"#!") {
                use std::os::unix::ffi::OsStrExt;
                let end = shebang.iter().position(|b| *b == b'\n')?;
                let interpreter = shebang[..end]
                    .split(|b| *b == b' ' || *b == b'\t')
                    .find(|word| !word.is_empty())?;
                let interpreter = Path::new(std::ffi::OsStr::from_bytes(interpreter));
                if !interpreter.is_absolute() || !interpreter.is_file() {
                    return None;
                }
                nix::unistd::access(interpreter, nix::unistd::AccessFlags::X_OK).ok()?;
                return Some(candidate);
            }
            let launchable = [
                &b"\x7fELF"[..],
                &[0xfe, 0xed, 0xfa, 0xce],
                &[0xfe, 0xed, 0xfa, 0xcf],
                &[0xce, 0xfa, 0xed, 0xfe],
                &[0xcf, 0xfa, 0xed, 0xfe],
                &[0xca, 0xfe, 0xba, 0xbe],
                &[0xbe, 0xba, 0xfe, 0xca],
            ]
            .iter()
            .any(|magic| head.starts_with(magic));
            return (head.len() >= 32 && launchable).then_some(candidate);
        }
        None
    }

    pub(super) fn direct_command(
        shell: &Command,
        inherit_env: bool,
        body: &str,
        forwarded: &[String],
        enabled: bool,
    ) -> Option<Command> {
        if !enabled {
            return None;
        }
        let words = words(body)?;
        let mut env: BTreeMap<OsString, OsString> = if inherit_env {
            std::env::vars_os().collect()
        } else {
            BTreeMap::new()
        };
        for (key, value) in shell.get_envs() {
            match value {
                Some(value) => {
                    env.insert(key.to_owned(), value.to_owned());
                }
                None => {
                    env.remove(key);
                }
            }
        }
        if ["ENV", "BASH_ENV", "SHELLOPTS", "BASHOPTS"]
            .iter()
            .any(|key| env.contains_key(std::ffi::OsStr::new(key)))
            || env
                .keys()
                .any(|key| key.to_string_lossy().starts_with("BASH_FUNC_"))
        {
            return None;
        }
        let program = resolve(words[0], env.get(std::ffi::OsStr::new("PATH"))?)?;
        let cwd = match shell.get_current_dir() {
            Some(dir) if dir.is_absolute() => dir.to_owned(),
            Some(dir) => std::env::current_dir().ok()?.join(dir),
            None => std::env::current_dir().ok()?,
        };
        let physical_cwd = std::fs::canonicalize(&cwd).ok()?;
        // Preserve a valid logical PWD (including symlinks), as sh does.
        let pwd = env
            .get(std::ffi::OsStr::new("PWD"))
            .filter(|pwd| {
                Path::new(pwd).is_absolute()
                    && same_file::is_same_file(Path::new(pwd), &cwd).unwrap_or(false)
            })
            .cloned()
            .unwrap_or_else(|| physical_cwd.into_os_string());
        let mut command = Command::new(program);
        command.arg0(words[0]).args(&words[1..]).args(forwarded);
        command
            .current_dir(cwd)
            .env_clear()
            .envs(env)
            .env("PWD", pwd);
        Some(command)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn conservative_syntax() {
            assert_eq!(
                words(" node build.js --target=es2020 ").unwrap(),
                ["node", "build.js", "--target=es2020"]
            );
            for body in [
                "",
                "echo hi",
                "printf x",
                "exit 7",
                "X=1 node",
                "./tool",
                "node 'x'",
                "node \"x\"",
                "node $X",
                "node *.js",
                "node ~",
                "node; other",
                "node | other",
                "node && other",
                "node >out",
                "node # comment",
                "node\\ x",
                "node\n",
                "node\tx",
                "node café",
            ] {
                assert!(words(body).is_none(), "{body:?}");
            }
            for word in SHELL_WORDS.split_ascii_whitespace() {
                assert!(words(word).is_none(), "{word}");
            }
        }

        #[test]
        fn resolution_and_effective_environment() {
            use std::os::unix::fs::{PermissionsExt, symlink};
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("bin");
            std::fs::create_dir(&bin).unwrap();
            let tool = bin.join("tool");
            std::fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
            let mut shell = Command::new("not-a-real-shell");
            shell.env_clear().env("PATH", &bin).env("KEEP", "value");
            shell.env("REMOVE", "value").env_remove("REMOVE");
            let planned = direct_command(&shell, false, "tool", &[], true).unwrap();
            let env: BTreeMap<_, _> = planned.get_envs().collect();
            assert_eq!(
                env.get(std::ffi::OsStr::new("KEEP")),
                Some(&Some(std::ffi::OsStr::new("value")))
            );
            assert!(!env.contains_key(std::ffi::OsStr::new("REMOVE")));
            for key in [
                "ENV",
                "BASH_ENV",
                "SHELLOPTS",
                "BASHOPTS",
                "BASH_FUNC_tool%%",
            ] {
                shell.env(key, "");
                assert!(direct_command(&shell, false, "tool", &[], true).is_none());
                shell.env_remove(key);
            }
            assert!(direct_command(&shell, false, "missing", &[], true).is_none());
            for path in [
                format!("{}:.", bin.display()),
                format!("{}:", bin.display()),
                String::new(),
            ] {
                shell.env("PATH", path);
                assert!(direct_command(&shell, false, "tool", &[], true).is_none());
            }
            shell.env(
                "PATH",
                format!("{}:{}", dir.path().join("missing").display(), bin.display()),
            );
            assert!(direct_command(&shell, false, "tool", &[], true).is_some());
            symlink(&tool, bin.join("linked-tool")).unwrap();
            assert!(direct_command(&shell, false, "linked-tool", &[], true).is_some());
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(direct_command(&shell, false, "tool", &[], true).is_none());
            shell.env_remove("PATH");
            assert!(direct_command(&shell, false, "tool", &[], true).is_none());
        }

        #[test]
        fn native_argv0_and_logical_pwd() {
            use std::os::unix::fs::symlink;
            let dir = tempfile::tempdir().unwrap();
            let logical = dir.path().join("logical");
            let physical = dir.path().join("physical");
            std::fs::create_dir(&physical).unwrap();
            symlink(&physical, &logical).unwrap();
            let mut shell = Command::new("sh");
            shell
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("PWD", &logical)
                .current_dir(&physical);
            let mut direct = direct_command(&shell, false, "env", &[], true).unwrap();
            // std's Debug uses argv0, unlike get_program (the resolved executable).
            assert!(format!("{direct:?}").contains("\"env\""));
            let output = String::from_utf8(direct.output().unwrap().stdout).unwrap();
            assert!(output.contains(&format!("PWD={}\n", logical.display())));
        }

        #[test]
        fn preserves_child_context_and_falls_back() {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let tool = dir.path().join("tool");
            std::fs::write(&tool, "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$@\"\n").unwrap();
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
            let mut shell = Command::new("sh");
            shell
                .env_clear()
                .env("PATH", dir.path())
                .current_dir(dir.path());
            let mut direct = direct_command(
                &shell,
                false,
                "tool first",
                &["$literal space".into()],
                true,
            )
            .unwrap();
            assert_eq!(direct.get_program(), tool);
            assert_eq!(
                String::from_utf8(direct.output().unwrap().stdout).unwrap(),
                format!("{}\nfirst\n$literal space\n", dir.path().display())
            );
            assert!(direct_command(&shell, false, "tool", &[], false).is_none());
            shell.env("ENV", "");
            assert!(direct_command(&shell, false, "tool", &[], true).is_none());
            shell.env_remove("ENV");
            std::fs::write(&tool, "echo no-shebang\n").unwrap();
            assert!(direct_command(&shell, false, "tool", &[], true).is_none());
            for body in [
                "#!\necho empty-interpreter\n",
                "#!/missing-inline-interpreter\n",
                "#!/bin/sh",
            ] {
                std::fs::write(&tool, body).unwrap();
                assert!(direct_command(&shell, false, "tool", &[], true).is_none());
            }
            shell.env("PATH", format!(":{}", dir.path().display()));
            assert!(direct_command(&shell, false, "tool", &[], true).is_none());
        }
    }
}
