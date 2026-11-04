//! Builds the macOS notification helper that `mise-system` embeds on macOS.

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

use eyre::{Result, eyre};

fn main() -> Result<()> {
    build_notification_helper()
}

fn build_notification_helper() -> Result<()> {
    let source = "src/history/notify/macos.m";
    let info = "src/history/notify/macos.plist";
    let icon = "assets/android-chrome-512x512.png";
    println!("cargo:rerun-if-changed={source}");
    println!("cargo:rerun-if-changed={info}");
    println!("cargo:rerun-if-changed={icon}");
    println!("cargo:rerun-if-env-changed=MISE_NOTIFICATION_SIGN_IDENTITY");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return Ok(());
    }
    let app = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("mise-notify.app");
    let contents = app.join("Contents");
    let output = contents.join("MacOS/mise-notify");
    fs::create_dir_all(contents.join("MacOS"))?;
    fs::create_dir_all(contents.join("Resources"))?;
    let status = cc::Build::new()
        .get_compiler()
        .to_command()
        .args(["-fobjc-arc", "-O2", "-mmacosx-version-min=10.14"])
        .args([
            "-framework",
            "AppKit",
            "-framework",
            "UserNotifications",
            "-framework",
            "CoreServices",
        ])
        .arg(source)
        .arg("-o")
        .arg(&output)
        .status()?;
    if !status.success() {
        return Err(eyre!("failed to build the macOS notification helper"));
    }
    // The compiler may place a debug-symbol bundle beside the executable.
    // It is not needed at runtime and must not become part of the app's seal,
    // because only the runtime bundle is embedded in mise.
    let debug_symbols = output.with_extension("dSYM");
    if debug_symbols.exists() {
        fs::remove_dir_all(debug_symbols)?;
    }
    fs::copy(info, contents.join("Info.plist"))?;
    let png = fs::read(icon)?;
    let mut icns = Vec::with_capacity(png.len() + 16);
    icns.extend_from_slice(b"icns");
    icns.extend_from_slice(&u32::try_from(png.len() + 16)?.to_be_bytes());
    icns.extend_from_slice(b"ic09");
    icns.extend_from_slice(&u32::try_from(png.len() + 8)?.to_be_bytes());
    icns.extend_from_slice(&png);
    fs::write(contents.join("Resources/mise.icns"), icns)?;

    let identity = env::var("MISE_NOTIFICATION_SIGN_IDENTITY").unwrap_or_else(|_| "-".into());
    println!(
        "cargo:rustc-env=MISE_NOTIFICATION_RELEASE_SIGNED={}",
        if identity == "-" { "0" } else { "1" }
    );
    let mut codesign = Command::new("/usr/bin/codesign");
    codesign
        .args(["--force", "--sign"])
        .arg(&identity)
        .args(["--identifier", "dev.jdx.mise.notifications"]);
    if identity != "-" {
        codesign.args(["--options", "runtime", "--timestamp"]);
    }
    let signed = codesign.arg(&app).output()?;
    if !signed.status.success() {
        return Err(eyre!(
            "failed to sign the macOS notification helper: {}",
            String::from_utf8_lossy(&signed.stderr).trim()
        ));
    }
    Ok(())
}
