use crate::cli::prelude::*;
use eyre::Result;
use predicates::prelude::*;
use std::include_str;

// From e2e/test_link
#[test]
fn test_linking() -> Result<()> {
    mise! {
        given_environment!(
            has_root_files
            exec_env_fixture(),
            install_fixture(),
            list_aliases_fixture(),
            list_all_fixture(),
            list_legacy_file_names_fixture(),
        );
        when!(
            given!(args "plugins", "link", "$ROOT/plugins/tiny");
            should!(succeed),
        ),
        when!(
            given!(args "plugins");
            should!(output "tiny"),
            should!(succeed),
        ),
        when!(
            given!(args "plugins", "link", "$ROOT/plugins/tiny");
            should!(fail),
        ),
        when!(
            given!(args "plugins", "link", "-f", "$ROOT/plugins/tiny");
            should!(succeed),
        ),
    }
}

fn exec_env_fixture() -> File {
    File {
        path: "plugins/tiny/bin/exec-env".into(),
        content: include_str!("data/plugins/tiny/bin/exec-env").into(),
    }
}

fn install_fixture() -> File {
    File {
        path: "plugins/tiny/bin/install".into(),
        content: include_str!("data/plugins/tiny/bin/exec-env").into(),
    }
}

fn list_aliases_fixture() -> File {
    File {
        path: "plugins/tiny/bin/list-aliases".into(),
        content: include_str!("data/plugins/tiny/bin/list-aliases").into(),
    }
}

fn list_all_fixture() -> File {
    File {
        path: "plugins/tiny/bin/list-all".into(),
        content: include_str!("data/plugins/tiny/bin/list-all").into(),
    }
}

fn list_legacy_file_names_fixture() -> File {
    File {
        path: "plugins/tiny/bin/list-legacy_filenames".into(),
        content: include_str!("data/plugins/tiny/bin/list-legacy-filenames").into(),
    }
}

// From e2e/test_ls_remote
#[test]
fn test_list_remote() -> Result<()> {
    mise! {
        when!(
            given!(args "plugins", "list-remote");
            should!(output "elixir"),
            should!(succeed),
        ),
        when!(
            given!(args "ls-remote", "tiny");
            should!(output "1.1.0"),
            should!(succeed),
        )
    }
}

// From e2e/test_nodejs
#[test]
#[ignore]
fn test_asdf_nodejs() -> Result<()> {
    // Given a default npm packages file exists in $ROOT
    let env = EnvironmentBuilder::new()
        .with_root_files([
            CONFIGS.get(".node-version"),
            CONFIGS.get(".default-npm-packages"),
        ])
        .with_exported_var("MISE_EXPERIMENTAL", "1")
        .with_exported_var("MISE_NODE_COREPACK", "1")
        .build()?;

    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["plugin", "uninstall", "node"])
        .run()?
        .success();

    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args([
            "plugin",
            "install",
            "nodejs",
            "https://github.com/asdf-vm/asdf-nodejs.git",
        ])
        .run()?
        .success();

    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["use", "nodejs@20.1.0"])
        .run()?
        .success();

    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["ls"])
        .run()?
        .success();

    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["exec", "--", "node", "--version"])
        .run()?
        .stdout(predicate::eq("v20.1.0\n"))
        .success();

    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["ls-remote", "nodejs"])
        .run()?
        .stdout(predicate::str::contains("20.1.0"))
        .success();

    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["node", "nodebuild", "--version"])
        .run()?
        .stdout(predicate::str::contains("node-build "))
        .success();

    env.mise()
        .env("MISE_LEGACY_VERSION_FILE", "1")
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["current", "node"])
        .run()?
        .stdout(predicate::str::contains("20.0.0"))
        .success();

    env.mise()
        .env("MISE_LEGACY_VERSION_FILE", "0")
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["current", "node"])
        .run()?
        .stdout(predicate::str::contains("20.0.0").not())
        .success();

    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["plugin", "uninstall", "nodejs"])
        .run()?
        .success();

    env.mise()
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["plugins", "--user", "node"])
        .run()?
        .stdout(predicate::str::contains("node").not())
        .success();

    env.mise()
        .env("MISE_DISABLE_TOOLS", "node")
        .env(
            "MISE_NODE_DEFAULT_PACKAGES_FILE",
            env.root_path().join(".default-npm-packages"),
        )
        .args(["plugins", "--core"])
        .run()?
        .stdout(predicate::str::contains("node").not())
        .success();

    env.teardown()
}

// From e2e/test_plugins_install
#[test]
fn test_install_multiple() -> Result<()> {
    mise! {
        when!(
            given!(args "plugin", "uninstall", "tiny", "shfmt", "shellcheck");
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "install", "tiny", "shfmt", "shellcheck");
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "ls");
            should!(output "tiny"),
            should!(output "shfmt"),
            should!(output "shellcheck"),
            should!(succeed),
        ),
    }
}

// From e2e/test_plugins_install
#[test]
fn test_tiny_remote_install() -> Result<()> {
    mise! {
        when!(
            given!(args "plugin", "install", "https://github.com/mise-plugins/rtx-tiny.git");
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "ls");
            should!(output "tiny"),
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "install", "-f", "tiny");
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "ls");
            should!(output "tiny"),
            should!(succeed),
        ),
    }
}

// From e2e/test_plugins_install
#[test]
fn test_tiny_local_and_remote_install() -> Result<()> {
    mise! {
        when!(
            given!(args "plugin", "install", "tiny", "https://github.com/mise-plugins/rtx-tiny");
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "ls");
            should!(output "tiny"),
            should!(succeed),
        ),
    }
}

// From e2e/test_plugins_install
#[test]
fn test_install_all() -> Result<()> {
    mise! {
        given_environment!(has_root_files CONFIGS.get(".tool-versions"));
        when!(
            given!(args "plugin", "install", "--all");
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "ls");
            should!(output "shellcheck"),
            should!(succeed),
        ),
    }
}

// From e2e/test_plugins_install
#[test]
fn test_uninstall() -> Result<()> {
    mise! {
        when!(
            given!(args "plugin", "install", "tiny");
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "uninstall", "tiny");
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "ls");
            should!(not_output "tiny"),
            should!(succeed),
        ),
    }
}

// From e2e/test_purge
#[test]
fn test_purge() -> Result<()> {
    mise! {
        given_environment!(has_root_files CONFIGS.get(".mise.toml"));
        when!(
            given!(args "install", "tiny");
            should!(succeed),
        ),
        when!(
            given!(args "ls", "--installed");
            should!(output "tiny"),
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "uninstall", "tiny");
            should!(succeed),
        ),
        when!(
            given!(args "ls", "--installed");
            should!(output "tiny"),
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "install", "tiny");
            should!(succeed),
        ),
        when!(
            given!(args "ls", "--installed");
            should!(output "tiny"),
            should!(succeed),
        ),
        when!(
            given!(args "plugin", "uninstall", "tiny", "--purge");
            should!(succeed),
        ),
        when!(
            given!(args "ls", "--installed");
            should!(not_output "tiny"),
            should!(succeed),
        ),
    }
}

// From e2e/test_plugins_install
#[test]
fn test_update() -> Result<()> {
    mise! {
        given_environment!(has_root_files CONFIGS.get(".tool-versions"));
        when!(
            given!(args "plugin", "update");
            should!(succeed)
        ),
        when!(
            given!(args "plugin", "update", "shfmt");
            should!(succeed)
        ),
    }
}
