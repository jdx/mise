---
description: "Require content-bound trust for every project config, turn off automatic trust, and re-verify provenance on each install."
socialDescription: "Require content-bound trust for project config and re-verify provenance on every install."
---

# Paranoid mode

Paranoid mode requires your approval before mise loads any project config file,
and again whenever that file changes. It also turns off automatic trust, including
in CI, and re-verifies provenance on every install. It does not sandbox the
commands you approve; see [Sandboxing](/sandboxing.html).

Turn it on for one command with `MISE_PARANOID=1`, or for every command:

```sh
mise settings set paranoid true
```

`mise settings set paranoid false` turns it off. The
[`paranoid`](/configuration/settings.html#paranoid) setting is global-only, so a
project cannot change it.

## Config files

Paranoid mode changes the [normal trust rules](/security.html#configuration-trust)
in these ways:

- Every config file outside global and system config needs trust, including
  files that normally load without it: `.tool-versions`, idiomatic version files
  such as `.nvmrc`, task files, and a `mise.toml` that only lists tools. The
  exception is `.miserc.toml`, which still loads without trust: it can only
  choose which config files load, and those files need trust.
- Trust is bound to content. `mise trust` stores a hash of the file, and any edit
  makes it untrusted again.
- Each file is trusted on its own. Trusting `mise.toml` does not trust
  `mise.local.toml` beside it.
- `mise run`, `mise install`, `mise exec`, `mise watch` and the `mise daemons`
  start, restart and register commands do not trust the config they load.
- CI does not make config trusted, and `--yes`, `MISE_YES=1` and `CI` do not
  answer the trust prompt. In a terminal, mise still asks.
- Trust is not shared between git worktrees.
- Trusting a monorepo root trusts only that file. Trust each subproject's config
  separately.

Read each file, then approve it. `mise trust --show` lists the config files from
the current directory up and whether each is trusted:

```sh
mise trust --show
mise trust path/to/mise.toml
```

For unattended runs, review the config and run `mise trust` before the job loads
it.

These stay trusted by path, without a hash, so editing them needs no new
approval:

- configs under [`trusted_config_paths`](/configuration/settings.html#trusted_config_paths)
- global and system config, which is where you turn paranoid mode on

A monorepo root that you trusted in normal mode, before you turned paranoid mode
on, still trusts every config below it without a hash. To remove that record,
run `MISE_PARANOID=0 mise trust --untrust` in the root, then trust the files you
reviewed.

If [safe mode](/security.html#safe-mode) is also on, it takes precedence: project
config loads without a trust prompt because it cannot run code, and nothing is
marked trusted.

### Remote includes

A remote [`include`](/configuration.html#include) must be pinned to a full
lowercase commit sha (`?ref=` followed by 40 or 64 hex digits) or to an OCI
digest (`@sha256:` followed by 64 lowercase hex digits), so that the hash of the
trusted file covers what it pulls in. A branch, a tag or a missing ref fails to
load:

```toml
include = [
  "git::https://github.com/myorg/platform.git//mise.toml?ref=0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c",
]
```

## Community plugins

In normal mode, installing a community plugin by short name asks for
confirmation. Paranoid mode refuses instead. Unlike config trust, `--yes`,
`MISE_YES=1`, CI and `--force` still allow the install.

A short name counts as trusted when its URL matches an asdf or vfox plugin in
mise's registry, or when the plugin is in the `mise-plugins` GitHub
organization. To install any other plugin, give its full Git URL on the command
line or in `[plugins]`. Naming the URL is your decision to trust that source:

```sh
mise plugins install example https://github.com/example/asdf-example
```

## Provenance re-verification

Paranoid mode turns on
[`locked_verify_provenance`](/configuration/settings.html#locked_verify_provenance).
Each install runs the backend's provenance checks again, such as SLSA, Cosign,
Minisign and GitHub attestations, instead of reusing the result recorded in
[`mise.lock`](/dev-tools/mise-lock.html#provenance-and-security). This can need
network access. It adds no check the backend does not support, and it does not
re-check a tool that is already installed.

## Attestations from mise-versions

mise asks [mise-versions](/configuration/settings.html#use_versions_host)
whether a public GitHub release artifact has GitHub attestations, so installs do
not spend your GitHub API rate limit. mise verifies any attestation it returns
and requires it to name the artifact's repository, so mise-versions cannot vouch
for an artifact. It can, however, answer that an artifact has none. Normal mode
accepts that answer, and a lockfile written afterwards records no provenance.

Paranoid mode confirms a "none" answer with GitHub before it skips verification.
This costs one GitHub API request per artifact without attestations.
