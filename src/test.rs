use color_eyre::{Help, SectionExt};
use std::env::{join_paths, set_current_dir};
use std::path::PathBuf;

use crate::cli::Cli;
use crate::config::Config;
use crate::output::tests::{STDERR, STDOUT};
use crate::{dirs, env, file};

#[ctor::ctor]
fn init() {
    console::set_colors_enabled(false);
    console::set_colors_enabled_stderr(false);
    if env::var("__RTX_DIFF").is_ok() {
        // TODO: fix this
        panic!("cannot run tests when rtx is activated");
    }
    env::set_var(
        "HOME",
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test"),
    );
    set_current_dir(env::HOME.join("cwd")).unwrap();
    env::remove_var("RTX_TRUSTED_CONFIG_PATHS");
    env::set_var("NO_COLOR", "1");
    env::set_var("RTX_YES", "1");
    env::set_var("RTX_USE_TOML", "0");
    env::set_var("RTX_DATA_DIR", env::HOME.join("data"));
    env::set_var("RTX_STATE_DIR", env::HOME.join("state"));
    env::set_var("RTX_CONFIG_DIR", env::HOME.join("config"));
    env::set_var("RTX_CACHE_DIR", env::HOME.join("data/cache"));
    env::set_var("RTX_DEFAULT_TOOL_VERSIONS_FILENAME", ".test-tool-versions");
    env::set_var("RTX_DEFAULT_CONFIG_FILENAME", ".test.rtx.toml");
    //env::set_var("TERM", "dumb");
    reset_config();
}

pub fn reset_config() {
    file::write(
        env::HOME.join(".test-tool-versions"),
        indoc! {r#"
            tiny  2
            dummy ref:master
            "#},
    )
    .unwrap();
    file::write(
        env::PWD.join(".test-tool-versions"),
        indoc! {r#"
            tiny 3
            "#},
    )
    .unwrap();
    file::write(
        env::HOME.join("config/config.toml"),
        indoc! {r#"
            [env]
            TEST_ENV_VAR = 'test-123'
            [settings]
            experimental = true
            verbose = true
            always_keep_download= true
            always_keep_install= true
            legacy_version_file= true
            plugin_autoupdate_last_check_duration = 20
            jobs = 2

            [alias.tiny]
            "my/alias" = '3.0'

            [tasks.configtask]
            run = 'echo "configtask:"'
            [tasks.lint]
            run = 'echo "linting!"'
            [tasks.test]
            run = 'echo "testing!"'
            "#},
    )
    .unwrap();
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
        .replace(&*env::RTX_BIN.to_string_lossy(), "rtx")
}

pub fn cli_run(args: &Vec<String>) -> eyre::Result<(String, String)> {
    Config::reset();
    *env::ARGS.write().unwrap() = args.clone();
    STDOUT.lock().unwrap().clear();
    STDERR.lock().unwrap().clear();
    Cli::run(args).with_section(|| format!("{}", args.join(" ").header("Command:")))?;
    let stdout = clean_output(STDOUT.lock().unwrap().join("\n"));
    let stderr = clean_output(STDERR.lock().unwrap().join("\n"));

    Ok((stdout, stderr))
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

#[macro_export]
macro_rules! assert_cli_snapshot {
    ($($args:expr),+, @$snapshot:literal) => {
        let args = &vec!["rtx".into(), $($args.into()),+];
        let (stdout, stderr) = $crate::test::cli_run(args).unwrap();
        let output = [stdout, stderr].join("\n").trim().to_string();
        insta::assert_snapshot!(output, @$snapshot);
    };
    ($($args:expr),+) => {
        let args = &vec!["rtx".into(), $($args.into()),+];
        let (stdout, stderr) = $crate::test::cli_run(args).unwrap();
        let output = [stdout, stderr].join("\n").trim().to_string();
        insta::assert_snapshot!(output);
    };
}

#[macro_export]
macro_rules! assert_cli {
    ($($args:expr),+) => {{
        let args = &vec!["rtx".into(), $($args.into()),+];
        $crate::test::cli_run(args).unwrap();
        let output = $crate::output::tests::STDOUT.lock().unwrap().join("\n");
        console::strip_ansi_codes(&output).trim().to_string()
    }};
}

#[macro_export]
macro_rules! assert_cli_err {
    ($($args:expr),+) => {{
        let args = &vec!["rtx".into(), $($args.into()),+];
        $crate::test::cli_run(args).unwrap_err()
    }};
}
