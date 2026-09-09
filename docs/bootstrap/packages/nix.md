# Nix

The `nix` bootstrap package manager installs packages into the current user's
normal Nix profile. It works on Linux and macOS with Nix on `PATH`, independently
of whether the operating system is NixOS.

```toml
[bootstrap.packages]
"nix:ripgrep" = "latest"
"nix:jq" = "latest"
"nix:python3Packages.pip" = "latest"
```

```sh
mise bootstrap packages use nix:ripgrep
mise bootstrap packages apply --manager nix
mise bootstrap packages status --json
mise bootstrap packages upgrade --manager nix
```

`mise bootstrap` also applies these packages. They belong to the user's Nix
profile, not a project's toolset, and use the normal Nix profile `PATH` setup.
mise does not create shims for them or invoke sudo.

## Requirements and sources

Use Nix 2.24 or newer with `nix-command` and `flakes` enabled in your Nix
configuration. Profiles must use the modern `nix profile` format. mise reports
legacy `nix-env` profile errors without deleting or migrating the profile.

Shorthand names resolve through the machine's `nixpkgs` registry entry. To
select another source, use an explicit flake reference and attribute:

```toml
[bootstrap.packages]
"nix:my-packages#hello" = "latest"
"nix:path:/absolute/path/to/flake#hello" = "latest"
```

Revision-pinned references such as
`nix:github:NixOS/nixpkgs/<revision>#ripgrep` are also supported: replace
`<revision>` with a real commit. Relative local paths, arbitrary Nix expressions,
and `^output` selectors are not supported.

`latest` means the package supplied by the selected source. It does not lock
a moving source. Pin the source revision, or configure a pinned registry entry,
when you need to restore the same package selection. Package-version pins such
as `nix:ripgrep@14` are not supported; table-form version pins are reported and
skipped during installation and upgrades.

mise uses the existing Nix registries, substituters, and trusted keys. It does
not install Nix, configure caches, update flake lockfiles itself, or add fallback
sources. Nix may build a package when it is not available from a configured cache.

## Apply, status, and upgrades

Apply is additive: it installs missing source/attribute combinations and leaves
other profile entries alone. Repeating apply does not update an already-installed
package from a moving source; use `upgrade` for that. Upgrades target only matching
configured profile entries. A revision-pinned source stays pinned.

Status identifies packages by their source and attribute path, not by finding a
similarly named executable on `PATH`. Its installed-version field contains Nix
store paths, which identify the installed artifacts without guessing a version.
Status and dry-run do not initialize a profile or fetch package sources.

Removing a declaration does not uninstall a profile package. Native Nix removal
and rollback remain available through `nix profile`; bootstrap pruning and
`state = "absent"` removal are not supported for this manager.

## Export to NixOS

Use export when packages should be part of a NixOS system configuration instead
of a user profile. Add declarations without installing them:

```sh
mise bootstrap packages use --no-install nix:ripgrep nix:jq
mise bootstrap packages export --format nix > packages.nix
```

The generated module refers to the importing configuration's `pkgs`:

```nix
{ pkgs, ... }:
{
  environment.systemPackages = [
    pkgs."jq"
    pkgs."ripgrep"
  ];
}
```

Place `packages.nix` beside your existing NixOS configuration and add
`./packages.nix` to its `imports`. For a configuration managed in Git, include
the generated file in the repository so flake evaluation can see it. Then use
that configuration's normal rebuild workflow, for example:

```sh
sudo nixos-rebuild switch --flake /path/to/system-config#hostname
```

Export itself does not require Nix and does not build, install, or activate
anything. It reads the merged declarations for the current mise platform and
environment, honors manager exclusions, and omits `state = "absent"` entries.
Only `nix:` shorthand attribute paths are exported. Explicit flake references
and version pins fail before any output is emitted, because the consuming
configuration owns the package set, its pins, and its overlays.

After removing a declaration, regenerate the module and rebuild. The package
then stops being contributed by this module; another module may still require
it. User-profile installations remain independent of the generated system
configuration and are not rolled back by switching NixOS generations.

For a system-export workflow, use `--no-install` and export rather than running
`packages apply` on these same declarations: apply installs into the user profile.
