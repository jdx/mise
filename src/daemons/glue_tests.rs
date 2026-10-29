//! Tests that need mise's config files, toolsets, and trust, which the daemons crate cannot see.

use super::glue::{DaemonSetExt, load};
use super::*;
use crate::config::SettingsExt;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::env_directive::EnvDirective;
use crate::toolset::ToolRequestSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn files(entries: &[(&str, &str)]) -> crate::config::ConfigMap {
    entries
        .iter()
        .map(|(path, body)| {
            let path = PathBuf::from(path);
            let cf: Arc<dyn crate::config::config_file::ConfigFile> =
                Arc::new(MiseToml::from_str(body, &path).unwrap());
            (path, cf)
        })
        .collect()
}

/// A project whose daemons another project can import. The filename has to
/// be one this build actually looks for; unit tests override the list.
fn referenced_project(dir: &Path, body: &str) -> PathBuf {
    let path = untrusted_project(dir, body);
    // Importing requires trust that already exists. These fixtures stand in
    // for projects the developer has reviewed and trusted.
    crate::config::config_file::trust(dir).unwrap();
    path
}

/// Every test that imports takes the settings lock. The paranoid-mode test
/// changes a global setting, and under `paranoid` a fixture's directory-level
/// trust no longer covers its config file, so an overlapping import would fail.
fn import_lock() -> crate::test::SettingsGuard {
    crate::test::SettingsGuard::lock()
}

/// The recorded failure for an import, by the name the project gave it.
fn failure(set: &DaemonSet, name: &str) -> String {
    set.import_errors
        .iter()
        .find(|((_, key), _)| key == name)
        .map(|(_, err)| err.clone())
        .unwrap_or_else(|| panic!("no import failure recorded for {name:?}"))
}

/// A referenced project the developer has never trusted.
fn untrusted_project(dir: &Path, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME);
    std::fs::write(&path, body).unwrap();
    path
}

/// Turns on `paranoid` for the rest of the test. Taking the guard proves the
/// caller holds the settings lock, and the guard restores the settings on drop,
/// even if the test panics.
fn paranoid_on(_held: &crate::test::SettingsGuard) {
    use confique::Layer;
    let mut settings = crate::config::settings::SettingsPartial::empty();
    settings.paranoid = Some(mise_settings::Paranoid::all());
    crate::config::Settings::reset(Some(settings));
}

#[tokio::test]
async fn daemons_merge_before_expanding_tools_and_exports() {
    crate::toolset::install_state::init().await.unwrap();
    let config = files(&[
        (
            "/parent/child/mise.toml",
            "[daemons.postgres]\nrun = 'echo custom'\n",
        ),
        (
            "/parent/mise.toml",
            "[daemons]\npostgres = '18'\nredis = '8'\n",
        ),
    ]);
    let set = load(&config).unwrap();
    assert!(set.daemons["postgres"].tool.is_none());
    // The preset's Postgres variables are gone with the preset, and this
    // custom daemon configures no port, so it gets no endpoint of its own.
    assert!(set.daemons["postgres"].exports.is_empty());
    assert_eq!(
        set.daemons["postgres"].table["run"].as_str(),
        Some("echo custom")
    );
    let mut requests = ToolRequestSet::default();
    set.add_tool_requests(&mut requests).unwrap();
    assert_eq!(requests.tools.len(), 1);
    assert!(
        requests
            .sources
            .values()
            .next()
            .unwrap()
            .is_mise_toml_daemon()
    );
}

#[tokio::test]
async fn preset_names_and_version_conflicts_are_reported() {
    crate::toolset::install_state::init().await.unwrap();
    let config = files(&[("/project/mise.toml", "[daemons]\nunknown = '1'\n")]);
    assert!(
        load(&config)
            .unwrap_err()
            .to_string()
            .contains("available presets")
    );
    let config = files(&[(
        "/project/mise.toml",
        "[daemons]\npostgres = '18'\n[daemons.analytics]\npreset = 'postgres'\nversion = '17'\nport = 5433\n",
    )]);
    // A second instance needs its own port; this is about the tool
    // conflict two versions of one preset produce.
    let set = load(&config).unwrap();
    assert!(
        set.add_tool_requests(&mut ToolRequestSet::default())
            .is_err()
    );
}

#[test]
fn referenced_project_daemons_belong_to_that_project() {
    let _serial = import_lock();
    let tmp = tempfile::tempdir().unwrap();
    let mirror = tmp.path().join("mirror-pipeline");
    referenced_project(
        &mirror,
        "[daemons_settings]\nnamespace = 'mirror'\n[daemons.worker]\nrun = 'exec worker'\n[daemons.idle]\nrun = 'exec idle'\n",
    );
    let app = tmp.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let config = files(&[(
        app.join("mise.toml").to_str().unwrap(),
        "[daemons.worker]\nproject = '../mirror-pipeline'\n[daemons.api]\nrun = 'exec api'\ndepends = ['worker']\n",
    )]);
    let set = load(&config).unwrap();
    let worker = &set.daemons["mirror/worker"];
    assert!(worker.imported);
    assert_eq!(worker.name, "worker");
    assert_eq!(worker.root, mirror.canonicalize().unwrap());
    assert_eq!(
        worker.source,
        mirror
            .canonicalize()
            .unwrap()
            .join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME)
    );
    // Only the named daemon is imported, and it keeps the referenced
    // project's namespace and root rather than this project's.
    assert!(!set.daemons.contains_key("idle"));
    let starting = set.with_dependencies(&["api".into()]);
    assert!(starting.daemons.contains_key("mirror/worker"));
    assert!(starting.daemons.contains_key("api"));
    assert!(set.roots().contains(&mirror.canonicalize().unwrap()));
    assert_eq!(
        set.namespace_for(&mirror.canonicalize().unwrap()),
        Some("mirror")
    );
    // A short depends on an imported daemon is rewritten; pitchfork resolves
    // bare names inside one namespace only.
    assert_eq!(
        set.daemons["api"].table["depends"][0].as_str(),
        Some("mirror/worker")
    );
    // The referenced project installs and exports for its own daemon, and
    // neither daemon here configures a port, so nothing is exported.
    let mut requests = ToolRequestSet::default();
    set.add_tool_requests(&mut requests).unwrap();
    assert!(requests.tools.is_empty());
    assert!(set.env_entries().is_empty());
}

#[test]
fn an_unresolvable_import_does_not_break_other_commands() {
    let _serial = import_lock();
    // Daemons load on every command. A sibling that is not checked out must
    // leave `mise x`, `mise run` and the activation hook working.
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let config = files(&[(
        app.join("mise.toml").to_str().unwrap(),
        "[daemons.pipeline]\nproject = '../mirror-pipeline'\n[daemons.api]\nrun = 'exec api'\n",
    )]);
    let set = load(&config).unwrap();
    // Nothing depends on the unresolved import here, so nothing is blocked.
    assert!(set.blocked.is_empty());
    // The local daemon still loads, so env and tool resolution are unaffected.
    assert!(set.daemons.contains_key("api"));
    assert!(!set.daemons.contains_key("pipeline"));
    // The failure is kept so `mise daemons` can report it with the path.
    let err = &failure(&set, "pipeline");
    assert!(err.contains("mirror-pipeline"), "{err}");
    assert!(err.contains("[daemons.pipeline].project"), "{err}");
    assert!(
        set.add_tool_requests(&mut ToolRequestSet::default())
            .is_ok(),
        "tool resolution must survive an unresolvable import"
    );
}

#[test]
fn nested_referenced_projects_resolve_the_same_namespace_both_ways() {
    let _serial = import_lock();
    // The namespace computed when importing has to be the one registered
    // when that root is prepared, or `depends` and `start` name an ID that
    // pitchfork never saw.
    let tmp = tempfile::tempdir().unwrap();
    let group = tmp.path().join("group");
    referenced_project(&group, "[daemons_settings]\nnamespace = 'shared'\n");
    let mirror = group.join("mirror-pipeline");
    referenced_project(&mirror, "[daemons.worker]\nrun = 'exec worker'\n");

    // What the referenced project computes for itself when it is prepared.
    let own = files(&[
        (
            mirror
                .join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME)
                .to_str()
                .unwrap(),
            "[daemons.worker]\nrun = 'exec worker'\n",
        ),
        (
            group
                .join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME)
                .to_str()
                .unwrap(),
            "[daemons_settings]\nnamespace = 'shared'\n",
        ),
    ]);
    let own = load(&own).unwrap();
    let own_root = own.daemons["worker"].root.clone();
    assert_eq!(own.namespace_for(&own_root), Some("shared"));

    // What another project computes when it imports that daemon.
    let app = tmp.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let config = files(&[(
        app.join("mise.toml").to_str().unwrap(),
        &format!(
            "[daemons.worker]\nproject = {}\n",
            toml::Value::String(mirror.to_string_lossy().into_owned())
        ),
    )]);
    let imported = load(&config).unwrap();
    assert!(imported.daemons.contains_key("shared/worker"));
    assert_eq!(
        imported.namespace_for(&mirror.canonicalize().unwrap()),
        Some("shared")
    );
}

#[test]
fn an_untrusted_referenced_project_is_not_imported() {
    let serial = import_lock();
    // This exercises the real trust gate. `mise x`, `mise run` and
    // `mise daemons start` mark the active config implicitly trusted, and
    // reading a sibling through that branch would grant it durable trust
    // with no prompt.
    let tmp = tempfile::tempdir().unwrap();
    let mirror = tmp.path().join("mirror");
    untrusted_project(&mirror, "[daemons.worker]\nrun = 'exec worker'\n");
    let app = tmp.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let config = files(&[(
        app.join("mise.toml").to_str().unwrap(),
        "[daemons.worker]\nproject = '../mirror'\n[daemons.api]\nrun = 'exec api'\ndepends = ['worker']\n",
    )]);
    let config_path = mirror.join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME);
    // `is_trusted` trusts everything in unit tests, except in paranoid
    // mode, where trust is bound to file contents and checked first. That is
    // the only way to exercise this gate without the bypass.
    paranoid_on(&serial);
    let set = load(&config).unwrap();
    assert!(set.find("worker").is_none());
    let err = &failure(&set, "worker");
    assert!(err.contains("not trusted"), "{err}");
    assert!(err.contains("mise trust"), "{err}");
    // Trying did not trust it as a side effect, which is the whole point:
    // the commands that reach here mark the active config implicitly
    // trusted, and that branch would have granted it durably.
    assert!(!host::is_path_trusted(&config_path));
    // A dependency on an import that never resolved would name a daemon
    // pitchfork cannot find, so it is dropped rather than registered, and
    // the daemon that needed it is recorded so starting it can refuse.
    assert!(!set.daemons["api"].table.contains_key("depends"));
    assert_eq!(
        set.blocked.get("api").map(|(name, _)| name.as_str()),
        Some("worker")
    );

    // Trusting it makes the same configuration import.
    crate::config::config_file::trust(&config_path).unwrap();
    let set = load(&config).unwrap();
    assert_eq!(set.find("worker").map(|d| d.imported), Some(true));
}

#[test]
fn referenced_projects_with_tool_versions_files_still_import() {
    let _serial = import_lock();
    // `.tool-versions` sits in the same config list but is not TOML; parsing
    // it would fail before the project's mise config is ever read.
    let tmp = tempfile::tempdir().unwrap();
    let mirror = tmp.path().join("mirror");
    referenced_project(&mirror, "[daemons.worker]\nrun = 'exec worker'\n");
    std::fs::write(
        mirror.join(&*crate::env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME),
        "node 20.0.0\n",
    )
    .unwrap();
    let app = tmp.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let config = files(&[(
        app.join("mise.toml").to_str().unwrap(),
        "[daemons.worker]\nproject = '../mirror'\n",
    )]);
    let set = load(&config).unwrap();
    assert_eq!(set.find("worker").map(|d| d.imported), Some(true));
}

#[test]
fn custom_daemons_export_their_resolved_port() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    let set = load(&files(&[(
            root.join("mise.toml").to_str().unwrap(),
            "[daemons.api]\nrun = 'server'\nport = 3000\n[daemons.web-ui]\nrun = 'ui'\n[daemons.web-ui.port]\nauto = true\nbase = 4000\n",
        )]))
        .unwrap();
    // Without an export the port would reach pitchfork only.
    assert_eq!(set.daemons["api"].exports["API_PORT"], "3000");
    // Punctuation is not valid in a variable name.
    assert_eq!(set.daemons["web-ui"].exports["WEB_UI_PORT"], "4000");
    // Two names that normalize onto one variable are ambiguous, so
    // neither is exported and both daemons keep working. Failing the load
    // would take `mise env` down for the whole project over a convenience.
    let set = load(&files(&[(
        root.join("mise.toml").to_str().unwrap(),
        "[daemons.web-ui]\nrun = 'a'\nport = 3000\n[daemons.web_ui]\nrun = 'b'\nport = 3001\n",
    )]))
    .unwrap();
    assert!(set.daemons["web-ui"].exports.is_empty());
    assert!(set.daemons["web_ui"].exports.is_empty());
    // Both still have their ports; only the variable is withheld.
    assert_eq!(set.daemons["web-ui"].port.unwrap().port, 3000);
    assert_eq!(set.daemons["web_ui"].port.unwrap().port, 3001);

    // A leading punctuation character becomes an underscore, which a shell
    // accepts, so such a name still gets its variables.
    let set = load(&files(&[(
        root.join("mise.toml").to_str().unwrap(),
        "[daemons.\".api\"]\nrun = 'a'\nport = 3000\n",
    )]))
    .unwrap();
    assert_eq!(set.daemons[".api"].exports["_API_PORT"], "3000");

    // A shell cannot export a name starting with a digit, but that name was
    // legal before this export existed, so it keeps working without one.
    let set = load(&files(&[(
        root.join("mise.toml").to_str().unwrap(),
        "[daemons.9api]\nrun = 'a'\nport = 3000\n",
    )]))
    .unwrap();
    assert!(set.daemons["9api"].exports.is_empty());
    assert_eq!(set.daemons["9api"].port.unwrap().port, 3000);
    assert_eq!(
        set.daemons["9api"].table["port"]["expect"][0].as_integer(),
        Some(3000)
    );

    // Pitchfork routes only a daemon that configures a port, so one
    // without a port gets neither variable.
    let set = load(&files(&[(
        root.join("mise.toml").to_str().unwrap(),
        "[daemons.api]\nrun = 'server'\n",
    )]))
    .unwrap();
    assert!(set.daemons["api"].exports.is_empty());
    assert!(set.daemons["api"].host.is_none());
    // Opting out of the proxy leaves only the port.
    let set = load(&files(&[(
        root.join("mise.toml").to_str().unwrap(),
        "[daemons.api]\nrun = 'server'\nport = 3000\nproxy = false\n",
    )]))
    .unwrap();
    assert_eq!(
        set.daemons["api"].exports.keys().collect::<Vec<_>>(),
        vec!["API_PORT"]
    );
    // The exports reach the environment mise renders.
    let set = load(&files(&[(
        root.join("mise.toml").to_str().unwrap(),
        "[daemons.api]\nrun = 'server'\nport = 3000\n",
    )]))
    .unwrap();
    assert!(
        set.env_entries().iter().any(
            |(d, _)| matches!(d, EnvDirective::Val(k, v, _) if k == "API_PORT" && v == "3000")
        )
    );
}

#[test]
fn the_table_form_rejects_unknown_keys() {
    // Matches `additionalProperties: false` in the schema, and catches a typo
    // that would otherwise leave the group silently empty. Rejection happens
    // when the configuration is parsed, before daemons are loaded.
    let parse = |body: &str| MiseToml::from_str(body, &PathBuf::from("/project/mise.toml"));
    assert!(parse("[daemon_groups.web]\ndaemons = ['api']\n").is_ok());
    let err = parse("[daemon_groups.web]\ndaemons = ['api']\ndeamons = ['api']\n")
        .unwrap_err()
        .to_string();
    assert!(err.contains("TOML"), "{err}");
    // The list form is unaffected.
    assert!(parse("[daemon_groups]\nweb = ['api']\n").is_ok());
}

#[test]
fn a_daemons_url_is_exported_beside_its_port() {
    let tmp = tempfile::tempdir().unwrap();
    let (primary, _) = checkout_pair(tmp.path());
    let set = load(&files(&[(
        primary.join("mise.toml").to_str().unwrap(),
        "[daemons_settings]\nnamespace = 'shop'\n\
             [daemons.api]\nrun = 'server'\nport = 3000\n\
             [daemons.web]\nrun = 'web'\nport = 5173\nproxy = 'front'\n\
             [daemons.cache]\nrun = 'cache'\nproxy = false\nport = 6380\n",
    )]))
    .unwrap();
    assert_eq!(set.daemons["api"].exports["API_PORT"], "3000");
    assert_eq!(
        set.daemons["api"].exports["API_URL"],
        "https://api.shop.localhost"
    );
    // A declared label replaces only the daemon's own component.
    assert_eq!(
        set.daemons["web"].exports["WEB_URL"],
        "https://front.shop.localhost"
    );
    // An opted-out daemon keeps its port and gets no URL at all.
    assert_eq!(set.daemons["cache"].exports["CACHE_PORT"], "6380");
    assert!(!set.daemons["cache"].exports.contains_key("CACHE_URL"));
    assert!(set.daemons["cache"].host.is_none());
    // The URLs reach the environment mise renders.
    assert!(set.env_entries().iter().any(|(d, _)| matches!(
        d,
        EnvDirective::Val(k, v, _) if k == "API_URL" && v == "https://api.shop.localhost"
    )));
}

#[test]
fn an_imported_daemon_does_not_claim_this_projects_variables() {
    let _serial = import_lock();
    let tmp = tempfile::tempdir().unwrap();
    let mirror = tmp.path().join("mirror");
    referenced_project(
        &mirror,
        "[daemons_settings]\nnamespace = 'mirror'\n[daemons.worker]\nrun = 'exec worker'\nport = 4000\n",
    );
    let root = tmp.path().join("app");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    let set = load(&files(&[(
        root.join("mise.toml").to_str().unwrap(),
        &format!(
            "[daemons_settings]\nnamespace = 'app'\n\
                 [daemons.worker]\nrun = 'exec worker'\nport = 4000\n\
                 [daemons.remote]\nproject = '{}'\nname = 'worker'\n",
            mirror.display()
        ),
    )]))
    .unwrap();
    // The import shares the remote daemon's name. Its exports belong to that
    // project, so it must not make this project's `worker` look ambiguous.
    assert_eq!(
        set.daemons["worker"].exports["WORKER_URL"],
        "https://worker.app.localhost"
    );
    assert!(set.env_entries().iter().any(|(d, _)| matches!(
        d,
        EnvDirective::Val(k, _, _) if k == "WORKER_URL"
    )));
}

#[test]
fn an_imported_presets_exports_do_not_take_a_local_ones() {
    let _serial = import_lock();
    let tmp = tempfile::tempdir().unwrap();
    referenced_project(
        &tmp.path().join("owner"),
        "[daemons_settings]\nnamespace = 'remote'\n\
             [daemons.db]\npreset = 'postgres'\nversion = '18'\n",
    );
    let app = tmp.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let set = load(&files(&[(
        app.join("mise.toml").to_str().unwrap(),
        "[daemons_settings]\nnamespace = 'shop'\n\
             [daemons.store]\nproject = '../owner'\nname = 'db'\n\
             [daemons.database]\nrun = 'gateway'\nport = 8080\n",
    )]))
    .unwrap();
    assert!(set.daemons["remote/db"].imported);
    // The imported preset does publish DATABASE_URL, but into its own
    // project's environment: `env_entries` leaves every imported daemon
    // out. There is nothing here for the local daemon to replace, so it
    // keeps the variable it derives.
    let exported: Vec<_> = set
        .env_entries()
        .into_iter()
        .filter_map(|(directive, _)| match directive {
            EnvDirective::Val(key, value, _) if key == "DATABASE_URL" => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(exported, ["https://database.shop.localhost"]);
}

/// Build a primary checkout and a linked worktree of it, as
/// `git worktree add` leaves them.
fn checkout_pair(tmp: &Path) -> (PathBuf, PathBuf) {
    let primary = tmp.join("shop");
    std::fs::create_dir_all(primary.join(".git")).unwrap();
    std::fs::write(primary.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
    let linked = tmp.join("shop-pr-42");
    std::fs::create_dir_all(&linked).unwrap();
    let private = primary.join(".git").join("worktrees").join("pr-42");
    std::fs::create_dir_all(&private).unwrap();
    std::fs::write(private.join("commondir"), "../..\n").unwrap();
    std::fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", private.display()),
    )
    .unwrap();
    (primary, linked)
}
