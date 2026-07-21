// Based on https://github.com/iki/mise-shim by Jan Killian (MIT License)

use std::env;
use std::path::Path;
use std::process::{Command, exit};

const MISE_SHIM_PATH_ENV: &str = "__MISE_SHIM_PATH";

fn paths_eq(a: &Path, b: &Path) -> bool {
    let lexical_eq = |a: &Path, b: &Path| {
        if cfg!(windows) {
            a.to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy())
        } else {
            a == b
        }
    };
    if lexical_eq(a, b) {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => lexical_eq(&a, &b),
        _ => false,
    }
}

fn main() {
    let exe = env::current_exe().unwrap_or_else(|e| {
        eprintln!("mise-shim: failed to determine executable path: {e}");
        exit(1);
    });
    let tool = exe
        .file_stem()
        .unwrap_or_else(|| {
            eprintln!("mise-shim: failed to determine tool name from executable path");
            exit(1);
        })
        .to_os_string();

    if env::var_os(MISE_SHIM_PATH_ENV)
        .as_deref()
        .is_some_and(|previous| paths_eq(Path::new(previous), &exe))
    {
        eprintln!(
            "mise-shim: recursive shim invocation detected for {}: {}",
            tool.to_string_lossy(),
            exe.display()
        );
        exit(1);
    }

    let args = env::args_os().skip(1);

    let status = Command::new("mise")
        .env(MISE_SHIM_PATH_ENV, &exe)
        .arg("x")
        .arg("--")
        .arg(&tool)
        .args(args)
        .status();

    match status {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("mise-shim: failed to execute mise: {e}");
            eprintln!("Ensure `mise` is installed and available on your PATH.");
            eprintln!("See https://mise.en.dev for installation instructions.");
            exit(1);
        }
    }
}
