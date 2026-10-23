---
description: "Require content-bound trust for project config and env files, refuse unregistered plugins, and re-verify provenance, each selectable on its own."
socialDescription: "Pick which paranoid protections to turn on: config trust, env files, plugins, provenance."
---

# Paranoid mode

Paranoid mode makes mise ask before it trusts things it would normally load on
its own. It has four categories, and you can turn on all of them or only the ones
you need:

| Category     | What it requires                                                                                  |
| ------------ | ------------------------------------------------------------------------------------------------- |
| `config`     | Approval of every project config file, bound to its content. No automatic trust, including in CI. |
| `env_files`  | Approval of the files that `env._.file`, `env._.source` and the `env_file` setting load.          |
| `plugins`    | Refuses to install an unregistered community plugin by short name.                                |
| `provenance` | Re-verifies provenance on every install and confirms a mise-versions "no attestations" answer.    |

Paranoid mode does not sandbox the commands you approve; see
[Sandboxing](/sandboxing.html).

Turn on every category for one command with `MISE_PARANOID=1`, or for every
command:

```sh
mise settings paranoid=true
```

`mise settings paranoid=false` turns it all off. Name categories to turn on only
those:

```sh
mise settings paranoid=env_files,plugins
```

```toml
# ~/.config/mise/config.toml
[settings]
paranoid = ["env_files", "plugins"]
```

`MISE_PARANOID=config,env_files` selects categories from the environment. An
unknown name is an error that lists the valid ones. The
[`paranoid`](/configuration/settings.html#paranoid) setting is global-only, so a
project cannot change it.

## Approving only dotenv files {#approving-only-env-files}

A global `find_up` entry loads a dotenv file from whichever repository you are
in. To approve those files without changing how project config is trusted, turn
on `env_files` alone:

```toml
# ~/.config/mise/config.toml
[settings]
paranoid = ["env_files"]

[env]
_.file = [{ path = ".env", find_up = true }]
```

In a repository with a `.env`, mise now refuses to load it until you approve its
current contents, and again after every edit:

```sh
mise trust .env
```

Project config, CI, plugins and provenance behave as they do without paranoid
mode.

## Config files {#config}

The `config` category changes the [normal trust rules](/security.html#configuration-trust)
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

### Environment files {#environment-files}

The `env_files` category applies to this. A file that [`env._.file`](/environments/#env-file) loads, or a script that
[`env._.source`](/environments/#env-source) runs, is not covered by the trust of
the config that names it, because an edit to it changes the environment of every
command. Each existing file needs its own trust, bound to its content, and fails
with an error that names the path and prints none of its values until you
approve it. An edit makes it untrusted again:

```sh
mise trust
```

`mise trust` with no path trusts the nearest untrusted config and every env file
and script that config reads, and again after an edit. `mise trust .env` trusts
only that file.

This applies to every `_.file` format, to `_.source` scripts, and to the files the
[`env_file`](/configuration/settings.html#env_file) setting finds. It includes a
file that a global `find_up = true` entry or a glob finds in a repository. It
does not apply to a global `_.file` or `_.source` entry that names one file
without a glob, which you chose.

Approval is bound to the file's content whether or not `config` is on. To load a file without approving each change, put it under
[`trusted_config_paths`](/configuration/settings.html#trusted_config_paths).
Like the other settings that stay trusted by path, it is global-only, so a
project cannot opt its own files out.

## Community plugins {#plugins}

The `plugins` category changes this. In normal mode, installing a community plugin by short name asks for
confirmation. Paranoid mode refuses instead. Unlike config trust, `--yes`,
`MISE_YES=1`, CI and `--force` still allow the install.

A short name counts as trusted when its URL matches an asdf or vfox plugin in
mise's registry, or when the plugin is in the `mise-plugins` GitHub
organization. To install any other plugin, give its full Git URL on the command
line or in `[plugins]`. Naming the URL is your decision to trust that source:

```sh
mise plugins install example https://github.com/example/asdf-example
```

## Provenance re-verification {#provenance}

The `provenance` category turns on
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

The `provenance` category confirms a "none" answer with GitHub before it skips verification.
This costs one GitHub API request per artifact without attestations.

Neither mode accepts "none" for a release the mise registry says must have
attestations: a `github:` backend, or an `aqua:` backend whose recipe declares
them, at or after the backend's
[`attestations_since`](/contributing/registry.html#required-attestations)
version. There a missing attestation fails the install and `mise lock`.

In either mode, when mise-versions fails to answer (a timeout, a lost
connection, or a `429` or `5xx` response), mise asks GitHub's attestation API
instead and verifies what GitHub returns the same way.
[`provenance_api_failures_fatal`](/configuration/settings.html#provenance_api_failures_fatal)
then decides what happens if GitHub fails too.
