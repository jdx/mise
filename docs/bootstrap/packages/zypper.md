---
description: "System packages for openSUSE and SUSE Linux Enterprise via zypper."
---

# RPM packages (zypper)

Manage host packages on openSUSE and SUSE Linux Enterprise. Both `zypper` and
`rpm` must be on `PATH`.

```toml
[bootstrap.packages]
"zypper:libopenssl-devel" = "latest"
"zypper:ripgrep" = "latest"
"zypper:bash" = "5.2.37-2.1" # example version-release pin
```

## Preview and apply

```sh
mise bootstrap packages status
mise bootstrap packages apply --manager zypper --dry-run
mise bootstrap packages apply --manager zypper
```

To record and install a package together, use
`mise bootstrap packages use zypper:ripgrep`. Unavailable managers are skipped
when applying a shared configuration; explicitly selecting `--manager zypper`
fails if the manager is unavailable.

## Behavior

- State is checked through the local RPM database with `rpm -q`, without
  refreshing repositories or elevating privileges.
- Missing packages and mismatched pins use `zypper --non-interactive install`,
  elevated through mise's [sudo policy](/bootstrap/packages/#sudo).
- `apply --update` runs `zypper --non-interactive refresh` first. Otherwise,
  zypper follows the repositories' automatic refresh settings.
- `upgrade` refreshes metadata and installs the requested versions of configured,
  already-installed packages. Missing packages are skipped.
- Commands retain zypper's configured dependency, license, and signature policies.
  Non-interactive mode fails if a required confirmation cannot be answered.
- Reboot-required (102) and package-manager-restart-required (103) exit codes
  produce warnings. Other nonzero statuses, including skipped repositories and
  failed RPM scripts, are reported as errors.

## Version selection

Pins are passed as `name=version` or `name=version-release`. A version-only pin
accepts any release of that exact version; it does not match a version prefix.
Pinned installation allows downgrades with `--oldpackage`.

The version above illustrates syntax. Choose a version available in the host's
enabled repositories; mise does not add repositories or retrieve archived RPMs.
`"latest"` accepts an already-installed package. Use `upgrade` to request updates.

## Remove packages

```toml
[bootstrap.packages]
"zypper:ripgrep" = { state = "absent" }
```

`apply` removes an installed package with `zypper --non-interactive remove`.
An already-absent package is a no-op. Preview dependency removals with
`apply --dry-run` and the printed zypper command's native `--dry-run` option.

## Transactional systems

On read-only systems such as MicroOS, package changes require the distribution's
transactional tooling. Where zypper provides a transactional wrapper, its normal
snapshot and reboot requirements still apply; mise does not activate snapshots.
