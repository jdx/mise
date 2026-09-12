//! mise's embedder profile for the embedded aube package manager.
//!
//! mise links aube in as a library and drives it in-process
//! ([`aube::embed`]) rather than going through `aube::cli_main`, so nothing
//! registers a host identity for us. Without one, aube falls back to its
//! standalone [`aube::embed::AUBE`] profile and behaves like the `aube` CLI:
//! it prints an `aube <aube-version> by jdx.dev` banner into mise's own
//! install progress display, sends an `aube/<aube-version>` User-Agent to the
//! npm registry, resolves and provisions its own node, and validates
//! `engines.aube` against the aube crate version.
//!
//! Only the branding and the four embedder-fixed behavior toggles differ from
//! [`aube::embed::AUBE`]. Config and manifest names stay compatible with aube;
//! embedded npm installs separately pass invocation-scoped mise-owned cache
//! and store paths and disable aube's global virtual store.

use std::path::PathBuf;
use std::sync::Once;

use aube::embed::{AUBE, Host};

static MISE_HOST: Host = Host {
    // Branding. `display_name`/`version` are what the install progress banner
    // renders, and `user_agent` is what the npm registry and lifecycle
    // scripts' `npm_config_user_agent` see — all of which should say mise,
    // since the user asked mise for a tool and never chose aube. `vendor:
    // None` drops the `by jdx.dev` attribution (aube suppresses it for any
    // registered embedder anyway; being explicit keeps it from depending on
    // that).
    name: "mise",
    display_name: "mise",
    vendor: None,
    version: env!("CARGO_PKG_VERSION"),
    user_agent: concat!("mise/", env!("CARGO_PKG_VERSION")),

    // Config surface: inherited so `aube.allowBuilds` and the generated
    // lockfile keep their canonical names. Embedded npm installs override
    // their cache/store paths per invocation instead of relying on these
    // process-wide namespace defaults.
    self_names: AUBE.self_names,
    compatible_names: AUBE.compatible_names,
    lockfile_basename: AUBE.lockfile_basename,
    workspace_yaml: AUBE.workspace_yaml,
    manifest_namespace: AUBE.manifest_namespace,
    env_prefix: AUBE.env_prefix,
    config_env_prefix: AUBE.config_env_prefix,
    cache_namespace: AUBE.cache_namespace,
    data_namespace: AUBE.data_namespace,

    // The install dir is a mise-generated throwaway project holding exactly
    // one lockfile, so aube's canonical-lockfile precedence is the right (and
    // only reachable) answer.
    canonical_lockfile_always_wins: true,
    // mise owns node provisioning: `install_via_aube_embed` hands aube the
    // node it resolved as a tool dependency. Leaving aube's own resolver live
    // would let it probe version files and download a second node behind
    // mise's back. A per-call `EmbedderRuntime` is honored regardless of this
    // toggle, and with no node dependency resolved aube still falls back to an
    // ambient `node` on PATH.
    runtime_switching: false,
    // mise's version is not in aube's version namespace, so an `engines.aube`
    // constraint must not be checked against it. `engines.node` is unaffected.
    self_engines_check: false,
    // mise owns its own upgrade path; aube's update notifier and its
    // aube.jdx.dev endpoints must never run from inside mise.
    self_update_enabled: false,
};

static INIT: Once = Once::new();

/// Hidden aube CLI entry that lifecycle shims invoke on the host executable.
///
/// Embedded aube sets `AUBE_NODE_GYP_EXE` to `current_exe()` (mise) and writes
/// lazy `node-gyp` shims that call `$AUBE_NODE_GYP_EXE __node-gyp-bootstrap
/// <project-dir>`. Standalone aube handles that itself; as an embedder mise
/// must intercept it before its own argv parser — otherwise naked-run
/// preprocessing turns it into `mise run __node-gyp-bootstrap` and native
/// `allow_builds` installs (e.g. `gemini-cli` → `node-pty`) fail with "no
/// tasks defined".
const NODE_GYP_BOOTSTRAP_CMD: &str = "__node-gyp-bootstrap";

/// Register mise as aube's host. Idempotent and cheap; call it before any
/// aube work rather than relying on a single startup hook, so the library
/// entry points (`npm:` installs and registry metadata queries) are each
/// self-sufficient.
///
/// aube's registration is itself first-write-wins, so the [`Once`] is only to
/// keep repeat calls off the hot path.
pub(crate) fn init() {
    INIT.call_once(|| {
        // No setting defaults: mise passes every install-scoped knob it cares
        // about (release age, trust-policy excludes, build allowlist) through
        // the synthetic project's `.npmrc` and `package.json`, which outrank
        // embedder defaults anyway.
        aube::embed::initialize(&MISE_HOST, vec![]);
    });
}

/// Handle aube's private trampoline argv before mise's own parser, tokio
/// runtime, or naked-run rewrite touch the args. Returns `Some(exit_code)`.
///
/// Uses [`aube::embed::bootstrap_node_gyp`] (aube ≥ 2.2) rather than routing
/// through [`aube::cli_main`], matching standalone aube's `__node-gyp-bootstrap`
/// behavior: bootstrap into the cache and print the executable path.
pub(crate) fn try_run_embedded_cli(args: &[String]) -> Option<i32> {
    if !is_embedded_cli_command(args) {
        return None;
    }
    let project_dir = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    init();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("mise: failed to start runtime for node-gyp bootstrap: {err}");
            return Some(1);
        }
    };
    match runtime.block_on(aube::embed::bootstrap_node_gyp(&project_dir)) {
        Ok(path) => {
            println!("{}", path.display());
            Some(0)
        }
        Err(err) => {
            eprintln!("{err:?}");
            Some(1)
        }
    }
}

fn is_embedded_cli_command(args: &[String]) -> bool {
    args.get(1).map(String::as_str) == Some(NODE_GYP_BOOTSTRAP_CMD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mise_host_keeps_aube_config_names() {
        assert_eq!(MISE_HOST.cache_namespace, AUBE.cache_namespace);
        assert_eq!(MISE_HOST.data_namespace, AUBE.data_namespace);
        assert_eq!(MISE_HOST.manifest_namespace, AUBE.manifest_namespace);
        assert_eq!(MISE_HOST.lockfile_basename, AUBE.lockfile_basename);
    }

    #[test]
    fn mise_host_brands_as_mise() {
        assert_eq!(MISE_HOST.display_name, "mise");
        assert_eq!(MISE_HOST.vendor, None);
        assert_eq!(MISE_HOST.version, env!("CARGO_PKG_VERSION"));
        assert!(MISE_HOST.user_agent.starts_with("mise/"));
    }

    #[test]
    fn mise_host_disables_aube_owned_behavior() {
        assert!(!MISE_HOST.runtime_switching);
        assert!(!MISE_HOST.self_engines_check);
        assert!(!MISE_HOST.self_update_enabled);
    }

    #[test]
    fn detects_aube_node_gyp_bootstrap_trampoline() {
        assert!(is_embedded_cli_command(&[
            "mise".into(),
            NODE_GYP_BOOTSTRAP_CMD.into(),
            "/tmp/project".into(),
        ]));
        assert!(!is_embedded_cli_command(&[
            "mise".into(),
            "install".into(),
            "node".into(),
        ]));
        assert!(!is_embedded_cli_command(&["mise".into()]));
    }
}
