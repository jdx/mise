// Based on https://github.com/iki/mise-shim by Jan Killian (MIT License)

use std::env;
use std::process::{Command, exit};

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

    let args = env::args_os().skip(1);

    let status = Command::new("mise")
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
            eprintln!("See https://mise.jdx.dev for installation instructions.");
            exit(1);
        }
    }
}
