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

/// Whether this root keeps the base port. The primary checkout, a project
/// outside git, a submodule, and a `--separate-git-dir` clone are each the
/// single copy of their project, so they take slot 0 and the default
/// single-checkout experience is unchanged.
///
/// A project root is the directory holding `mise.toml`, often nested well below
/// the checkout (`/repo/packages/api`), so the enclosing checkout decides, not
/// the config directory. That ancestor walk, and the rules that separate a real
/// worktree from a submodule, live in [`crate::git::in_linked_worktree`].
pub(crate) fn is_primary(root: &Path) -> bool {
    !crate::git::in_linked_worktree(root)
}

/// The slot for a project root: 0 for a primary checkout, otherwise a stable
/// value in 1..SLOTS derived from the canonical root path.
pub(crate) fn slot(root: &Path) -> u16 {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if is_primary(&root) {
        return 0;
    }
    hashed_slot(&root)
}

/// The slot a canonical root hashes to, in 1..SLOTS. Distinct roots draw from
/// a bounded number of slots, so two of them can land on the same one; what
/// this guarantees is that a given root always lands on the same slot and
/// never on slot 0, which belongs to the primary checkout.
fn hashed_slot(root: &Path) -> u16 {
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

    /// A linked worktree as `git worktree add` leaves it: a `.git` file naming
    /// a private dir under the main checkout's `worktrees/`, and that dir
    /// carrying the `commondir` pointer back to the shared git dir.
    fn worktree(dir: &Path, name: &str) -> std::path::PathBuf {
        let private = dir.join(".git").join("worktrees").join(name);
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(dir.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(private.join("commondir"), "../..\n").unwrap();
        let root = dir.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", private.display()),
        )
        .unwrap();
        root
    }

    /// The slot a canonical root must land on, worked out here rather than by
    /// calling [`hashed_slot`], so that a changed formula -- a wider modulus, a
    /// dropped `1 +` handing a worktree the primary checkout's port -- fails
    /// instead of moving both sides of the assertion together. The hash itself
    /// is still read through [`crate::hash::hash_to_str`]: a golden hex digest
    /// would pin `Path`'s `Hash` implementation, which std does not promise to
    /// keep stable across releases, and which `PortClaim` persistence is
    /// designed to tolerate changing.
    fn expected_slot_of(canonical: &Path) -> u16 {
        let hash = u64::from_str_radix(&crate::hash::hash_to_str(&canonical), 16).unwrap();
        1 + u16::try_from(hash % u64::from(SLOTS - 1)).unwrap()
    }

    /// [`expected_slot_of`] for a root that still has to be canonicalized, the
    /// way `slot` does. A root's slot is pinned by its own path; that two roots
    /// differ is not a property the code provides, since 511 slots make a
    /// collision between two unrelated paths likely enough to fail a run and a
    /// random tempdir decides when. So the tests pin the derivation instead of
    /// comparing two slots.
    fn expected_slot(root: &Path) -> u16 {
        expected_slot_of(&root.canonicalize().unwrap())
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
    fn linked_worktrees_get_their_own_stable_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let one = worktree(tmp.path(), "feature-a");
        let two = worktree(tmp.path(), "feature-b");
        assert!(!is_primary(&one));
        assert_eq!(
            slot(&one),
            slot(&one),
            "same root resolves to the same slot"
        );
        assert_eq!(slot(&one), expected_slot(&one), "each root picks its own");
        assert_eq!(slot(&two), expected_slot(&two));
        assert!((1..SLOTS).contains(&slot(&one)));

        let a = resolve("db", &one, None, 1, Some(5432), None).unwrap();
        let b = resolve("db", &two, None, 1, Some(5432), None).unwrap();
        assert_eq!(a.port, 5432 + slot(&one));
        assert_eq!(b.port, 5432 + slot(&two));
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

        // The project root is what gets hashed, not the checkout, so sibling
        // projects inside one worktree draw from their own slots, as does the
        // same project in another worktree.
        assert_eq!(slot(&nested), expected_slot(&nested));
        let sibling = wt.join("packages").join("web");
        std::fs::create_dir_all(&sibling).unwrap();
        assert_eq!(slot(&sibling), expected_slot(&sibling));
        let other_wt = worktree(tmp.path(), "second");
        let other_nested = other_wt.join("packages").join("api");
        std::fs::create_dir_all(&other_nested).unwrap();
        assert_eq!(slot(&other_nested), expected_slot(&other_nested));

        // The primary checkout keeps the base port at any depth.
        let primary = tmp.path().join("primary");
        std::fs::create_dir_all(primary.join(".git")).unwrap();
        let primary_nested = primary.join("packages").join("api");
        std::fs::create_dir_all(&primary_nested).unwrap();
        assert_eq!(slot(&primary_nested), 0);
    }

    #[test]
    fn every_worktree_of_a_bare_repository_is_offset() {
        // A bare repo plus worktrees has no ordinary checkout, so nothing holds
        // slot 0. Each worktree still gets its own stable slot.
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("repo.git");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let mut slots = Vec::new();
        for name in ["main", "feature"] {
            let private = bare.join("worktrees").join(name);
            std::fs::create_dir_all(&private).unwrap();
            std::fs::write(private.join("commondir"), "../..\n").unwrap();
            let root = tmp.path().join(name);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(
                root.join(".git"),
                format!("gitdir: {}\n", private.display()),
            )
            .unwrap();
            assert!(!is_primary(&root), "{name} is a worktree");
            assert_eq!(slot(&root), expected_slot(&root), "{name} hashes its root");
            slots.push(slot(&root));
        }
        assert!(slots.iter().all(|s| *s != 0), "none keeps the base port");
    }

    #[test]
    fn distinct_roots_spread_across_the_slot_range() {
        // The slot of a linked worktree comes from its path alone, so this
        // needs no filesystem and no tempdir: fixed paths always produce the
        // same slots. Slots are bounded, so two roots can share one; what must
        // hold is that they stay in range, never take slot 0, and spread out
        // instead of clumping onto a few.
        let roots: Vec<_> = (0..64)
            .map(|i| std::path::PathBuf::from(format!("/repo/worktrees/wt-{i}")))
            .collect();
        let slots: Vec<u16> = roots.iter().map(|r| hashed_slot(r)).collect();
        assert!(slots.iter().all(|s| (1..SLOTS).contains(s)));
        for (root, slot) in roots.iter().zip(&slots) {
            assert_eq!(*slot, expected_slot_of(root), "{}", root.display());
        }
        let distinct: std::collections::HashSet<u16> = slots.iter().copied().collect();
        assert!(
            distinct.len() > roots.len() / 2,
            "{} of {} roots shared a slot",
            roots.len() - distinct.len(),
            roots.len()
        );
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
            // The parent directory must be named `worktrees`; a path that
            // merely contains the word somewhere else is not a worktree.
            "/repo/worktrees/nested/.git/modules/sub",
            "/worktrees",
            // Shaped like a worktree, but the private dir does not exist, so
            // there is no checkout here to give its own port to.
            "/repo/.git/worktrees/pruned",
        ] {
            let root = tmp.path().join(crate::hash::hash_to_str(&target));
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
