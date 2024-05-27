use std::env::join_paths;
use std::path::PathBuf;

use color_eyre::{Help, SectionExt};
use indoc::indoc;

use crate::cli::Cli;
use crate::config::{config_file, Config};
use crate::output::tests::{STDERR, STDOUT};
use crate::{cmd, dirs, env, file, forge};

#[macro_export]
macro_rules! assert_cli_snapshot {
    ($($args:expr),+, @$snapshot:literal) => {
        let args = &vec!["mise".into(), $($args.into()),+];
        let (stdout, stderr) = $crate::test::cli_run(args).unwrap();
        let output = [stdout, stderr].join("\n").trim().to_string();
        insta::assert_snapshot!(output, @$snapshot);
    };
    ($($args:expr),+) => {
        let args = &vec!["mise".into(), $($args.into()),+];
        let (stdout, stderr) = $crate::test::cli_run(args).unwrap();
        let output = [stdout, stderr].join("\n").trim().to_string();
        insta::assert_snapshot!(output);
    };
}

#[macro_export]
macro_rules! assert_cli {
    ($($args:expr),+) => {{
        let args = &vec!["mise".into(), $($args.into()),+];
        $crate::test::cli_run(args).unwrap();
        let output = $crate::output::tests::STDOUT.lock().unwrap().join("\n");
        console::strip_ansi_codes(&output).trim().to_string()
    }};
}

#[macro_export]
macro_rules! assert_cli_err {
    ($($args:expr),+) => {{
        let args = &vec!["mise".into(), $($args.into()),+];
        $crate::test::cli_run(args).unwrap_err()
    }};
}

#[ctor::ctor]
fn init() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug")
    }
    console::set_colors_enabled(false);
    console::set_colors_enabled_stderr(false);
    if env::var("__MISE_DIFF").is_ok() {
        // TODO: fix this
        panic!("cannot run tests when mise is activated");
    }
    env::set_var(
        "HOME",
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test"),
    );
    env::remove_var("MISE_TRUSTED_CONFIG_PATHS");
    env::remove_var("MISE_DISABLE_TOOLS");
    env::set_var("NO_COLOR", "1");
    env::set_var("MISE_YES", "1");
    env::set_var("MISE_USE_TOML", "0");
    env::set_var("MISE_DATA_DIR", env::HOME.join("data"));
    env::set_var("MISE_STATE_DIR", env::HOME.join("state"));
    env::set_var("MISE_CONFIG_DIR", env::HOME.join("config"));
    env::set_var("MISE_CACHE_DIR", env::HOME.join("data/cache"));
    env::set_var("MISE_DEFAULT_TOOL_VERSIONS_FILENAME", ".test-tool-versions");
    env::set_var("MISE_DEFAULT_CONFIG_FILENAME", ".test.mise.toml");
    //env::set_var("TERM", "dumb");
}

pub fn reset() {
    Config::reset();
    forge::reset();
    config_file::reset();
    env::set_current_dir(env::HOME.join("cwd")).unwrap();
    env::remove_var("MISE_FAILURE");
    file::remove_all(&*dirs::TRUSTED_CONFIGS).unwrap();
    file::remove_all(&*dirs::TRACKED_CONFIGS).unwrap();
    file::write(
        env::HOME.join(".test-tool-versions"),
        indoc! {r#"
            tiny  2
            dummy ref:master
            "#},
    )
    .unwrap();
    file::write(
        env::current_dir().unwrap().join(".test-tool-versions"),
        indoc! {r#"
            tiny 3
            "#},
    )
    .unwrap();
    file::write(
        env::HOME.join("config/settings.toml"),
        indoc! {r#"
            experimental = true
            verbose = true
            "#},
    )
    .unwrap();
    file::write(
        env::HOME.join("config/config.toml"),
        indoc! {r#"
            [env]
            TEST_ENV_VAR = 'test-123'

            [alias.tiny]
            "my/alias" = '3.0'

            [tasks.configtask]
            run = 'echo "configtask:"'
            [tasks.lint]
            run = 'echo "linting!"'
            [tasks.test]
            run = 'echo "testing!"'
            [settings]
            always_keep_download= true
            always_keep_install= true
            legacy_version_file= true
            plugin_autoupdate_last_check_duration = "20m"
            jobs = 2
            "#},
    )
    .unwrap();
    let _ = file::remove_file(".test.mise.toml");
    assert_cli!("prune");
    assert_cli!("install");
}

pub fn setup_git_repo() {
    cmd!("git", "init", "-b", "trunk").run().unwrap();
    file::write("README.md", "# testing123").unwrap();
    cmd!("git", "add", "README.md").run().unwrap();
    cmd!(
        "git",
        "-c",
        "user.name=ferris",
        "-c",
        "user.email=ferris@example.com",
        "commit",
        "-m",
        "feat: add README"
    )
    .run()
    .unwrap();
}

pub fn cleanup() {
    let _ = file::remove_all(".github");
    let _ = file::remove_all(".git");
    let _ = file::remove_all("README.md");
}

pub fn replace_path(input: &str) -> String {
    let path = join_paths(&*env::PATH)
        .unwrap()
        .to_string_lossy()
        .to_string();
    let home = env::HOME.to_string_lossy().to_string();
    input
        .replace(&path, "$PATH")
        .replace(&home, "~")
        .replace(&*env::MISE_BIN.to_string_lossy(), "mise")
}

pub fn cli_run(args: &Vec<String>) -> eyre::Result<(String, String)> {
    Config::reset();
    forge::reset();
    config_file::reset();
    env::ARGS.write().unwrap().clone_from(args);
    STDOUT.lock().unwrap().clear();
    STDERR.lock().unwrap().clear();
    Cli::run(args).with_section(|| format!("{}", args.join(" ").header("Command:")))?;
    let stdout = clean_output(STDOUT.lock().unwrap().join("\n"));
    let stderr = clean_output(STDERR.lock().unwrap().join("\n"));

    Ok((stdout, stderr))
}

pub fn change_installed_version(tool: &str, cur: &str, new: &str) {
    file::rename(
        dirs::INSTALLS.join(tool).join(cur),
        dirs::INSTALLS.join(tool).join(new),
    )
    .unwrap()
}

fn clean_output(output: String) -> String {
    let output = output.trim().to_string();
    let output = console::strip_ansi_codes(&output).to_string();
    let output = output.replace(dirs::HOME.to_string_lossy().as_ref(), "~");
    let output = replace_path(&output);
    output.trim().to_string()
}

#[macro_export]
macro_rules! with_settings {
    ($body:block) => {{
        let home = $crate::env::HOME.to_string_lossy().to_string();
        insta::with_settings!({sort_maps => true, filters => vec![
            (home.as_str(), "~"),
        ]}, {$body})
    }}
}
