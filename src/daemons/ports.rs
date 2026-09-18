//! Deterministic, config-time port allocation for project daemons.
//!
//! Ports are rendered into `run`, `ready_cmd`, and `[env]` exports while the
//! configuration is loaded, long before pitchfork could resolve anything, so
//! pitchfork's own `bump` cannot separate two checkouts of the same project.
//! `port = "auto"` instead derives an offset from the project root: the primary
//! checkout keeps the well-known base port and each linked git worktree gets its
//! own deterministic slot.
use eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Distance between consecutive slots. One port per slot suits every current
/// preset; a daemon that binds a contiguous range raises it so neighbouring
/// slots cannot overlap.
pub(crate) const DEFAULT_STRIDE: u16 = 1;

/// Slot 0 is the primary checkout, so linked worktrees draw from 1..SLOTS.
/// The span is kept narrow enough that an allocated port stays recognisably
/// near its base (Postgres occupies 5432..=5943, Redis 6379..=6890) and the two
/// preset ranges cannot reach each other.
pub(crate) const SLOTS: u16 = 512;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PortRequest {
    /// An explicit port, unchanged across checkouts.
    Fixed(u16),
    /// A base port offset by the slot derived from the project root.
    Auto { base: Option<u16>, stride: u16 },
    /// Pitchfork's own structured `port` table, forwarded verbatim.
    Passthrough(toml::Value),
}

/// A resolved allocation as persisted in `state.json`. The inputs travel with
/// the port so a later change to `base` or `stride` re-derives, while a change
/// to the hash function leaves existing allocations alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PortClaim {
    pub port: u16,
    pub base: u16,
    /// Zero marks an explicit fixed port, which is recorded for collision
    /// reporting but never re-derived. `auto` strides are always positive.
    pub stride: u16,
}

impl PortClaim {
    pub(crate) fn fixed(port: u16) -> Self {
        Self {
            port,
            base: port,
            stride: 0,
        }
    }

    pub(crate) fn is_auto(&self) -> bool {
        self.stride > 0
    }
}

/// Parse the `port` value of a daemon declaration.
pub(crate) fn parse(name: &str, value: toml::Value) -> Result<PortRequest> {
    match value {
        toml::Value::Integer(port) => u16::try_from(port)
            .ok()
            .filter(|p| *p > 0)
            .map(PortRequest::Fixed)
            .ok_or_else(|| eyre::eyre!("[daemons.{name}].port must be an integer from 1 to 65535")),
        toml::Value::String(text) if text == "auto" => Ok(PortRequest::Auto {
            base: None,
            stride: DEFAULT_STRIDE,
        }),
        toml::Value::String(text) => bail!(
            "[daemons.{name}].port string must be \"auto\"; got {text:?}. Use an integer for a fixed port."
        ),
        toml::Value::Table(table) if table.contains_key("auto") => {
            if table.get("auto").and_then(toml::Value::as_bool) != Some(true) {
                bail!("[daemons.{name}].port.auto must be true; omit it for a fixed port");
            }
            let base = port_field(name, &table, "base")?;
            let stride = port_field(name, &table, "stride")?.unwrap_or(DEFAULT_STRIDE);
            for key in table.keys() {
                if !matches!(key.as_str(), "auto" | "base" | "stride") {
                    bail!("unknown [daemons.{name}].port key {key:?}");
                }
            }
            Ok(PortRequest::Auto { base, stride })
        }
        other => Ok(PortRequest::Passthrough(other)),
    }
}

fn port_field(name: &str, table: &toml::Table, key: &str) -> Result<Option<u16>> {
    table
        .get(key)
        .map(|value| {
            value
                .as_integer()
                .and_then(|n| u16::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| {
                    eyre::eyre!("[daemons.{name}].port.{key} must be an integer from 1 to 65535")
                })
        })
        .transpose()
}

/// Whether this project root sits inside a linked git worktree.
///
/// A project root is the directory holding `mise.toml`, which is often nested
/// well below the checkout (`/repo/packages/api`), so the enclosing checkout is
/// found by walking ancestors rather than looking beside the config. The first
/// `.git` encountered decides: a directory is the primary checkout, and a file
/// names the real git directory.
///
/// Only a `gitdir:` under a `worktrees/` directory is a linked worktree. A
/// submodule points into `.git/modules/`, and `git clone --separate-git-dir`
/// points somewhere else entirely; both are the single copy of their project and
/// must keep the base port.
fn in_linked_worktree(root: &Path) -> bool {
    for dir in root.ancestors() {
        let git = dir.join(".git");
        // A directory here is the primary checkout; stop before any outer one.
        if git.is_dir() {
            return false;
        }
        if git.is_file() {
            let Ok(content) = std::fs::read_to_string(&git) else {
                // Unreadable marker: fall back to the well-known port rather
                // than silently moving a single checkout off it.
                return false;
            };
            let Some(target) = content
                .lines()
                .find_map(|line| line.trim().strip_prefix("gitdir:"))
            else {
                return false;
            };
            return Path::new(target.trim())
                .components()
                .any(|c| c.as_os_str() == "worktrees");
        }
    }
    false
}

/// Whether this root keeps the base port. The primary checkout, a project
/// outside git, a submodule, and a separate-git-dir clone are each the single
/// copy of their project, so they take slot 0 and the default single-checkout
/// experience is unchanged.
pub(crate) fn is_primary(root: &Path) -> bool {
    !in_linked_worktree(root)
}

/// The slot for a project root: 0 for a primary checkout, otherwise a stable
/// value in 1..SLOTS derived from the canonical root path.
pub(crate) fn slot(root: &Path) -> u16 {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if is_primary(&root) {
        return 0;
    }
    // hash_to_str renders a u64 siphash as hex; reuse it so the slot and the
    // state directory agree on how a root is identified. The root, not the
    // checkout, is hashed, so sibling projects in one worktree stay distinct.
    let hash = u64::from_str_radix(&crate::hash::hash_to_str(&root), 16).unwrap_or_default();
    1 + u16::try_from(hash % u64::from(SLOTS - 1)).unwrap_or_default()
}

/// Resolve an `auto` request for a project root, preferring a previously
/// persisted allocation for the same base and stride.
pub(crate) fn resolve(
    name: &str,
    root: &Path,
    base: Option<u16>,
    stride: u16,
    preset_default: Option<u16>,
    persisted: Option<PortClaim>,
) -> Result<PortClaim> {
    let base = base.or(preset_default).ok_or_else(|| {
        eyre::eyre!(
            "[daemons.{name}].port = \"auto\" needs a base port; \
             set port = {{ auto = true, base = <port> }} or use a preset"
        )
    })?;
    if let Some(claim) = persisted
        && claim.base == base
        && claim.stride == stride
    {
        return Ok(claim);
    }
    let offset = slot(root).checked_mul(stride).ok_or_else(|| {
        eyre::eyre!("[daemons.{name}].port stride {stride} overflows the port range")
    })?;
    let port = base.checked_add(offset).filter(|p| *p > 0).ok_or_else(|| {
        eyre::eyre!(
            "[daemons.{name}].port base {base} with stride {stride} exceeds 65535 for this worktree; \
             choose a lower base or stride"
        )
    })?;
    Ok(PortClaim { port, base, stride })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree(dir: &Path, name: &str) -> std::path::PathBuf {
        let root = dir.join(name);
        std::fs::create_dir_all(&root).unwrap();
        // A real linked worktree points into the main checkout's worktrees dir.
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}/.git/worktrees/{name}\n", dir.display()),
        )
        .unwrap();
        root
    }

    #[test]
    fn primary_checkouts_keep_the_base_port() {
        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("project");
        std::fs::create_dir_all(primary.join(".git")).unwrap();
        assert!(is_primary(&primary));
        assert_eq!(slot(&primary), 0);
        let claim = resolve("db", &primary, None, 1, Some(5432), None).unwrap();
        assert_eq!(claim.port, 5432);

        // A project that is not a git checkout at all is also the only copy.
        let plain = tmp.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(slot(&plain), 0);
    }

    #[test]
    fn linked_worktrees_are_stable_and_distinct() {
        let tmp = tempfile::tempdir().unwrap();
        let one = worktree(tmp.path(), "feature-a");
        let two = worktree(tmp.path(), "feature-b");
        assert!(!is_primary(&one));
        assert_eq!(
            slot(&one),
            slot(&one),
            "same root resolves to the same slot"
        );
        assert_ne!(slot(&one), slot(&two));
        assert!((1..SLOTS).contains(&slot(&one)));

        let a = resolve("db", &one, None, 1, Some(5432), None).unwrap();
        let b = resolve("db", &two, None, 1, Some(5432), None).unwrap();
        assert_ne!(a.port, b.port);
        assert!(a.port > 5432 && a.port <= 5432 + SLOTS);
        assert_eq!(a, resolve("db", &one, None, 1, Some(5432), None).unwrap());
    }

    #[test]
    fn persisted_allocations_survive_a_hash_change() {
        let tmp = tempfile::tempdir().unwrap();
        let root = worktree(tmp.path(), "feature");
        let stored = PortClaim {
            port: 5999,
            base: 5432,
            stride: 1,
        };
        assert_eq!(
            resolve("db", &root, None, 1, Some(5432), Some(stored)).unwrap(),
            stored
        );
        // Changing the declared base re-derives instead of pinning the old port.
        let rederived = resolve("db", &root, Some(6000), 1, Some(5432), Some(stored)).unwrap();
        assert_eq!(rederived.base, 6000);
        assert_ne!(rederived.port, stored.port);
    }

    #[test]
    fn auto_requires_a_base_and_fits_the_port_range() {
        let tmp = tempfile::tempdir().unwrap();
        let root = worktree(tmp.path(), "feature");
        assert!(
            resolve("api", &root, None, 1, None, None)
                .unwrap_err()
                .to_string()
                .contains("needs a base port")
        );
        // The last port leaves no room for any worktree offset, and the widest
        // stride overruns the range from every slot. A moderate stride is not
        // tested here because whether it fits depends on this root's slot.
        assert!(resolve("api", &root, Some(65535), 1, None, None).is_err());
        assert!(resolve("api", &root, Some(3000), u16::MAX, None, None).is_err());
        // The same stride still resolves from the primary checkout, which has
        // no offset to apply.
        let primary = tmp.path().join("primary");
        std::fs::create_dir_all(&primary).unwrap();
        assert_eq!(
            resolve("api", &primary, Some(3000), u16::MAX, None, None)
                .unwrap()
                .port,
            3000
        );
    }

    #[test]
    fn nested_project_roots_inherit_their_checkout() {
        // A mise.toml well below the checkout root is the common case in a
        // monorepo. The enclosing worktree decides, not the config directory.
        let tmp = tempfile::tempdir().unwrap();
        let wt = worktree(tmp.path(), "feature");
        let nested = wt.join("packages").join("api");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(!is_primary(&nested), "nested root is still in the worktree");
        assert_ne!(slot(&nested), 0);

        // Sibling projects inside one worktree stay distinct from each other
        // and from the same project in another worktree.
        let sibling = wt.join("packages").join("web");
        std::fs::create_dir_all(&sibling).unwrap();
        assert_ne!(slot(&nested), slot(&sibling));
        let other_wt = worktree(tmp.path(), "second");
        let other_nested = other_wt.join("packages").join("api");
        std::fs::create_dir_all(&other_nested).unwrap();
        assert_ne!(slot(&nested), slot(&other_nested));

        // The primary checkout keeps the base port at any depth.
        let primary = tmp.path().join("primary");
        std::fs::create_dir_all(primary.join(".git")).unwrap();
        let primary_nested = primary.join("packages").join("api");
        std::fs::create_dir_all(&primary_nested).unwrap();
        assert_eq!(slot(&primary_nested), 0);
    }

    #[test]
    fn only_a_worktrees_gitdir_moves_off_the_base_port() {
        let tmp = tempfile::tempdir().unwrap();
        // A submodule and a separate-git-dir clone each have a .git *file*, but
        // both are the single copy of their project and keep the base port.
        for target in [
            "/repo/.git/modules/sub",
            "/elsewhere/detached-git-dir",
            "not a gitdir line",
        ] {
            let root = tmp.path().join(format!("c{}", target.len()));
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join(".git"), format!("gitdir: {target}\n")).unwrap();
            assert!(is_primary(&root), "{target} must keep the base port");
            assert_eq!(slot(&root), 0);
        }
    }

    #[test]
    fn port_declarations_are_parsed_and_validated() {
        assert_eq!(
            parse("api", toml::Value::Integer(3000)).unwrap(),
            PortRequest::Fixed(3000)
        );
        assert_eq!(
            parse("api", toml::Value::String("auto".into())).unwrap(),
            PortRequest::Auto {
                base: None,
                stride: DEFAULT_STRIDE
            }
        );
        assert_eq!(
            parse(
                "api",
                toml::Value::Table(toml::toml! { auto = true base = 3000 stride = 4 })
            )
            .unwrap(),
            PortRequest::Auto {
                base: Some(3000),
                stride: 4
            }
        );
        assert!(matches!(
            parse(
                "api",
                toml::Value::Table(toml::toml! { expect = [3000] bump = false })
            )
            .unwrap(),
            PortRequest::Passthrough(_)
        ));
        for invalid in [
            toml::Value::Integer(0),
            toml::Value::Integer(70000),
            toml::Value::String("bump".into()),
            toml::Value::Table(toml::toml! { auto = false }),
            toml::Value::Table(toml::toml! { auto = true base = 0 }),
            toml::Value::Table(toml::toml! { auto = true offset = 3 }),
        ] {
            assert!(parse("api", invalid).is_err());
        }
    }
}
