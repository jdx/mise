// Based on https://github.com/iki/mise-shim by Jan Killian (MIT License)

use std::env;
use std::path::Path;
use std::process::{Command, ExitCode};

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

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<i32, String> {
    let exe = env::current_exe()
        .map_err(|err| format!("mise-shim: failed to determine executable path: {err}"))?;
    let tool = exe
        .file_stem()
        .ok_or_else(|| "mise-shim: failed to determine tool name from executable path".to_string())?
        .to_os_string();
    if env::var_os(MISE_SHIM_PATH_ENV)
        .as_deref()
        .is_some_and(|previous| paths_eq(Path::new(previous), &exe))
    {
        return Err(format!(
            "mise-shim: recursive shim invocation detected for {}: {}",
            tool.to_string_lossy(),
            exe.display()
        ));
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
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(err) => Err(format!(
            "mise-shim: failed to execute mise: {err}\n\
             Ensure `mise` is installed and available on your PATH.\n\
             See https://mise.jdx.dev for installation instructions."
        )),
    }
}
