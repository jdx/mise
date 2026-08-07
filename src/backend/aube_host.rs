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

/// Register mise as aube's host. Idempotent and cheap; call it before any
/// aube work rather than relying on a single startup hook, so the library
/// entry points (`npm:` installs and registry metadata queries) are each
/// self-sufficient.
///
/// aube's registration is itself first-write-wins, so the [`Once`] is only to
/// keep repeat calls off the hot path.
pub fn init() {
    INIT.call_once(|| {
        // No setting defaults: mise passes every install-scoped knob it cares
        // about (release age, trust-policy excludes, build allowlist) through
        // the synthetic project's `.npmrc` and `package.json`, which outrank
        // embedder defaults anyway.
        aube::embed::initialize(&MISE_HOST, vec![]);
    });
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
}
