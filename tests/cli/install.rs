use crate::cli::prelude::*;
use eyre::Result;
use predicates::prelude::*;
use test_case::test_case;

// From e2e/test_bun, e2e/tes_deno, e2e/test_java
#[test_case("bun", ".bun-version", "1.0.17", &["bun", "-v"], "1.0.17", false)]
#[test_case("deno", ".deno-version", "1.35.3", &["deno", "-V"], "1.35.3", false)]
#[test_case("java", ".sdkmanrc", "java=17.0.2", &["java", "-version"], "openjdk version \"17.0.2\"", true)]
#[test_case("java", ".java-version", "17.0.2", &["java", "-version"], "openjdk version \"17.0.2\"", true)]
fn test_tool_specific_version_files(
    tool: &str,
    file_name: &str,
    contents: &str,
    tool_cmd: &[&str],
    expected: &str,
    use_err: bool,
) -> Result<()> {
    // Given a tool specific version file exists in $ROOT
    let env = EnvironmentBuilder::new()
        .with_root_files([File {
            path: file_name.into(),
            content: contents.into(),
        }])
        .build()?;

    // And the tool is installed
    env.mise().args(["install", tool]).run()?.success();

    // When executing the tools version command
    // Mise should output the correct version information
    let res = env
        .mise()
        .args([&["x", tool, "--"], tool_cmd].concat())
        .run()?
        .success();
    if use_err {
        res.stderr(predicate::str::contains(expected))
    } else {
        res.stdout(predicate::str::contains(expected))
    };

    env.teardown()
}

// From e2e/test_install
#[test]
fn test_tiny_install_from_config() -> Result<()> {
    mise! {
        given_environment!(has_root_files CONFIGS.get(".mise.toml"));
        when!(
            given!(args "i", "tiny-ref@latest", "-f");
            should!(succeed),
        ),
        when!(
            given!(args "plugins", "uninstall", "tiny-ref");
            should!(succeed),
        ),
    }
}

// From e2e/test_neovim
#[test]
#[ignore]
fn test_neovim() -> Result<()> {
    mise! {
        when!(
            given!(args "install", "neovim@ref:master");
            should!(succeed),
        ),
        when!(
            given!(args "exec", "neovim@ref:master", "--", "nvim", "--version");
            should!(output "NVIM v0."),
            should!(succeed),
        ),
    }
}

// From e2e/test_nodejs
#[test]
fn test_nodejs() -> Result<()> {
    // Given a node version file and default npm packages file exists in $ROOT
    let env = EnvironmentBuilder::new()
        .with_root_files([
            CONFIGS.get(".node-version"),
            CONFIGS.get(".default-npm-packages"),
        ])
        .with_exported_var("MISE_EXPERIMENTAL", "1")
        .with_exported_var("MISE_NODE_COREPACK", "1")
        .build()?;

    // Given the node plugin is uninstalled
    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["plugin", "uninstall", "node"])
        .run()?
        .success();

    // When node@lts/hydrogen is insalled
    // Mise should succeed
    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["install", "node@lts/hydrogen"])
        .run()?
        .success();

    // When node is force installed
    // Mise should succeed
    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["install", "-f", "node"])
        .run()?
        .success();

    // When executing the node version command using node@lts/hydrogen
    // Mise should output the correct node version
    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["exec", "node@lts/hydrogen", "--", "node", "--version"])
        .run()?
        .stdout(predicate::str::contains("v18."))
        .success();

    // When executing the node version command
    // Mise should output the correct node version
    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["exec", "--", "node", "--version"])
        .run()?
        .stdout(predicate::str::contains("v20.0.0"))
        .success();

    // When executing `which yarn`
    // Mise should output `yarn`
    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["exec", "--", "which", "yarn"])
        .run()?
        .stdout(predicate::str::contains("yarn"))
        .success();

    env.teardown()
}

// From e2e/test_python
#[test]
fn test_python() -> Result<()> {
    mise! {
        given_environment!(has_exported_var "MISE_EXPERIMENTAL", "1"),
        given_environment!(has_home_files CONFIGS.get(".default-python-packages")),
        given_environment!(has_root_files python_config_fixture());
        when!(
            given!(args "install");
            should!(succeed),
        ),
        when!(
            given!(args "exec", "--", "python", "-m", "venv", "$MISE_DATA_DIR/venv");
            should!(succeed),
        ),
        when!(
            given!(args "exec", "python@3.12.0", "--", "python", "--version");
            should!(output "Python 3.12.0"),
            should!(succeed),
        ),
        when!(
            given!(args "env", "i-s", "--", "bash");
            should!(output "$DATA/venv"),
            should!(succeed),
        ),
        when!(
            given!(args "exec", "--", "which", "python");
            should!(output "$DATA/venv/bin/python"),
            should!(succeed),
        ),
    }
}

fn python_config_fixture() -> File {
    File {
        path: ".mise.toml".into(),
        content: toml::toml! {
            [env]
            "_".python.venv = {path="{{env.MISE_DATA_DIR}}/venv", create=true}

            [tools]
            python = "{{exec(command='echo 3.12.0')}}"
        }.to_string(),
    }
}

// From e2e/test_python
#[test]
#[ignore]
fn test_python_complie() -> Result<()> {
    mise! {
        given_environment!(has_exported_var "MISE_ALL_COMPILE", "1");
        when!(
            given!(args "install", "python@3.12", "-f");
            should!(succeed),
        ),
        when!(
            given!(args "exec", "python@3.12", "--", "python", "--version");
            should!(output "Python 3.12"),
            should!(succeed),
        ),
    }
}

// From e2e/test_raw
#[test]
fn test_raw() -> Result<()> {
    mise! {
        when!(
            given!(args "install", "--raw", "-f", "tiny@1", "tiny@2", "tiny@3");
            should!(succeed),
        ),
        when!(
            given!(env_var "MISE_RAW", "1"),
            given!(args "install", "-f", "tiny@1", "tiny@2", "tiny@3");
            should!(succeed),
        )
    }
}
