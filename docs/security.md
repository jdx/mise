---
description: "Control which project config can run code, verify downloads, hold back new releases, and restrict the commands mise starts."
socialDescription: "Control config trust, verify downloads, delay new releases, and restrict commands."
---

# Security overview

Each mise security control covers one stage, from loading a project's config to
running the commands it starts. The one you meet first is
[configuration trust](#configuration-trust): mise asks before it lets a
repository's config run code.

## What mise protects against

| Risk                                                                     | Control                                         | Default                                                      |
| ------------------------------------------------------------------------ | ----------------------------------------------- | ------------------------------------------------------------ |
| A repository's config runs code when you enter it or run mise there      | [Configuration trust](#configuration-trust)     | On                                                           |
| Automation has to read config that someone else wrote                    | [Safe mode](#safe-mode)                         | Off; set `MISE_SAFE=1`                                       |
| A trusted config changes after you reviewed it                           | [Paranoid mode](/paranoid.html)                 | Off                                                          |
| A download was tampered with or replaced                                 | [Download verification](#download-verification) | On, where the backend and the upstream project support it    |
| A newly published release is malicious                                   | [Minimum release age](#minimum-release-age)     | 24 hours for [most backends](#which-backends-have-a-default) |
| A task or command reads, writes or reaches more than it needs            | [Sandboxing](/sandboxing.html)                  | Off; Linux and macOS only                                    |
| A secret ends up in your shell, a log or a process that does not need it | [Secrets](#secrets)                             | Opt in                                                       |

Each control has its own scope. Trusting a config does not verify an upstream
release, and sandboxing a task does not sandbox mise while it evaluates config.

These controls do not review the software you choose to install. A tool, a
package or an asdf or vfox plugin runs with your permissions once you install
it. Global and system config, such as `~/.config/mise/config.toml`, belongs to
you, so mise trusts it without asking. mise itself runs as your user with no
sandbox.

## Configuration trust

Project config can run code. Templates can call `exec()`, hooks and `[env]`
directives run commands or change variables such as `PATH`, and tool options
such as `postinstall` run scripts. mise loads a config that can do any of this
only after it is trusted.

### What needs trust

A `mise.toml` loads without trust when it has no template syntax
(<span v-pre>`{{`</span>, `{%` or `{#`) and contains only:

- `min_version`
- `[tools]` entries whose values are version strings or arrays of version
  strings, with no options in the tool name (`"tool[opt=value]"`)
- `[tasks]`, as long as no task lists `secrets`

Anything else needs trust, including tool option tables, `[env]`, `[hooks]`,
`[settings]`, `[tool_alias]` and `include`. Other files follow the same idea:

- `.tool-versions` needs trust only when it contains template syntax or inline
  tool options such as `node[postinstall=...] 24`.
- Idiomatic version files such as `.nvmrc` never need trust.
- Task files, including TOML files from
  [`task_config.includes`](/tasks/task-discovery.html), need trust only when they
  contain template syntax or list `secrets`.
- A [`.miserc.toml`](/configuration.html#miserc) never needs trust. It only
  holds early settings that choose which config files load, and its templates
  cannot call `exec()` or `read_file()`.

[Paranoid mode](/paranoid.html) removes these exemptions apart from
`.miserc.toml`: every other config file outside global and system config needs
trust.

### When mise asks

When a command needs a config that is not trusted:

- In a terminal, mise asks
  `mise config files in <dir> are not trusted. Trust them?` with Yes, No and
  All, where All answers yes to every prompt in that command. No adds the
  directory to an ignore list, and mise skips that config without asking again
  until you run `mise trust` on it.
- Without a terminal, such as in a script or an editor extension, mise cannot
  ask, so the command fails with
  [`Config files in <path> are not trusted`](/errors.html#untrusted-config).
- Shell activation never asks. It skips the untrusted config, warns
  `<path> is not trusted, run mise trust to enable it`, and still applies the
  trusted ones.
- Commands that read every tracked config, such as `mise prune`, skip untrusted
  ones.

`mise trust --show` lists the trust status of the config directories from the
current directory up. A config that needs no trust still shows as `untrusted`.

### Implicit trust

Some commands trust the config they load without asking, because running them
is already a decision to run the project's code. In normal mode outside CI,
these are `mise run`, task shorthand such as `mise build`, `mise install`,
`mise exec`, `mise watch`, and `mise daemons start`, `restart` and `register`.
They record trust for every non-global config they load, including configs in
parent directories, as if you had run `mise trust`. A config on the ignore list
stays ignored.

mise also trusts a config file when it writes one, for example with
[`mise use`](/cli/use.html) or [`mise set`](/cli/set.html).

### CI and `--yes`

When mise detects CI, for example because `CI` is set, it treats every config as
trusted and records nothing. Configs on the ignore list or under
[`ignored_config_paths`](/configuration/settings.html#ignored_config_paths) still
do not load. `CI` also turns on [`yes`](/configuration/settings.html#yes).

Outside CI, `--yes` or `MISE_YES=1` answers the trust prompt with yes, which
records trust. Paranoid mode turns off both the CI exemption and `--yes` for
trust.

### Manage trust

```sh
mise trust --show              # trust status from here up
mise trust                     # trust the nearest untrusted config
mise trust path/to/mise.toml   # trust a reviewed file
mise trust --all               # trust configs here, above and below
mise trust --untrust           # remove trust
mise trust --ignore            # never load this config
```

In normal mode, trust belongs to a config root, usually the directory that holds
`mise.toml`, so it covers every config file there, such as `mise.local.toml`.
`mise trust --all` walks subdirectories, skipping hidden and gitignored ones and
`node_modules`, `vendor`, `target`, `dist` and `build`. Trust records are kept in
the [state directory](/directories.html). See [`mise trust`](/cli/trust.html)
for every flag.

Trust records use resolved paths. If a symlinked config, such as one installed
with GNU Stow, still reports untrusted after `mise trust`, trust the real file
path.

### Trust by path

Two settings decide trust by location:

- [`trusted_config_paths`](/configuration/settings.html#trusted_config_paths)
  trusts configs under these paths without a prompt, including projects you
  create there later. It overrides the ignore list, and `["/"]` trusts
  everything. It is global-only.
- [`ignored_config_paths`](/configuration/settings.html#ignored_config_paths)
  stops configs under these paths from loading. It overrides
  `trusted_config_paths` and `mise trust`. mise reads it before it looks for
  config files, so set it in a [`.miserc.toml`](/configuration.html#miserc) or
  in `MISE_IGNORED_CONFIG_PATHS`.

```toml
# ~/.config/mise/config.toml
[settings]
trusted_config_paths = ["~/src/work"]
```

### Monorepo roots

In normal mode, trusting a config with `monorepo_root = true` also trusts every
config file below its directory, so review the subprojects' config as part of
the repository. See [Monorepo tasks](/tasks/monorepo.html). In paranoid mode,
trusting the root trusts only that file.

### Git worktrees

A config in a linked git worktree is trusted when the same path in the main
checkout is trusted. `mise trust --untrust` in the worktree warns that the file
stays trusted for that reason; use `mise trust --ignore` there to stop it
loading. Paranoid mode does not share trust between worktrees.

## Safe mode

Set `MISE_SAFE=1` when automation has to read config it does not control, such
as a bot that resolves tool versions on pull request branches. Safe mode loads
project config without asking for trust, because it stops that config from
running code or changing the environment while mise resolves versions:

```sh
MISE_SAFE=1 mise lock --bump --dry-run --json
```

Remove `--dry-run` to update the lockfile. Safe mode does not make a command
read-only. The [`safe`](/configuration/settings.html#safe) setting is
global-only, so a project cannot turn it off for itself.

Safe mode is meant for resolution and metadata commands such as `mise lock`,
`mise ls`, `mise env` and `mise outdated`.

::: warning
Safe mode does not make other commands safe to run on config you have not
reviewed. [`mise dotfiles apply`](/cli/dotfiles/apply.html) and
[`mise bootstrap`](/cli/bootstrap.html) still apply a project's `[dotfiles]`
and `[bootstrap]` sections, apart from bootstrap hooks, so a repository can
write files such as `~/.bashrc`. Normal mode does not load that config until
you trust it. `mise install` can build a tool from source, which runs the
tool's build scripts, such as a `cargo:` crate's `build.rs`.
:::

### Refused operations

These fail with `<operation> is disabled in safe mode (MISE_SAFE=1)`:

- `exec()` and `read_file()` in templates, in any config.
- Running tasks.
- The tool options `postinstall` and `install_env`.
- asdf plugin scripts, so tools from the `asdf:` backend cannot list or install
  versions.
- Installing plugins. vfox plugins that are already installed or built in still
  run, because the operator chose them.
- mise secrets: `[secrets]` sources, `mise secrets ls`, tasks that list
  `secrets`, and `--secrets` and `--secrets-all` on `mise run` and `mise exec`.
- `mise daemons` commands and task daemons.
- The diagnostic commands in `[doctor.checks]` that `mise doctor project` runs.

### Ignored configuration

These are skipped without an error, so resolution commands keep working:

- Hooks and `[[watch_files]]` entries from every config, including global
  config, and bootstrap hooks, as with `--no-hooks`.
- Project `[env]`, `[vars]` and environment directives such as `_.path` and
  `_.file`.
- `_.source` in every config, including global config.
- Project `[shell_alias]`.
- Project `[wrappers]` and the `rust` tool's `mr_boxington` option.
- Project `[settings]`, so a repository cannot turn off verification or redirect
  a backend.
- Remote `include` entries in project config, which mise does not fetch.
- Project daemon declarations. Daemons also do not start or stop when you change
  directories.

Global and system config still applies, apart from these rules. Review it and the
environment of the process that runs mise.

### What still works

Version resolution keeps working for backends that query HTTP APIs, such as core
tools, `aqua:`, `github:`, `gitlab:`, `http:`, `cargo:`, `pypi:`, `gem:`,
`dotnet:` and `npm:`. The `go:` backend runs `go list` with
`GOTOOLCHAIN=local`, so a project's `go.mod` cannot make Go download another
toolchain. Listing installed tools and refreshing `mise.lock` work as usual.

Safe mode loads untrusted config without a prompt or an error, in paranoid mode
too. It does not mark the config trusted for later runs without safe mode. Syntax
errors and refused operations still fail.

Safe mode limits what project config can do. It is not an operating-system
sandbox for mise, its network requests or the plugins the operator installed.
Use [sandboxing](/sandboxing.html) to restrict the commands mise starts.

## Download verification

<a id="software-verification"></a>

What mise checks depends on the backend and on what the upstream project
publishes. A checksum shows that the bytes match an expected digest. A signature
or provenance check also ties the artifact to a signer or a build. The expected
checksum, key or identity has to come from a source you trust.

To see which checks apply to a tool, run [`mise tool`](/cli/tool.html) and read
the `Security` line:

```sh
mise tool jq
# Security: checksum (sha256), github_attestations
```

| Backend or tool             | Checks                                                                                                                                       | Settings and options                                                                                                                                                                                                                                                                                                                                            |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aqua:`                     | Checksums, Cosign and Minisign signatures, SLSA provenance and GitHub artifact attestations, as the aqua registry declares them for the tool | [`aqua.cosign`](/configuration/settings.html#aqua.cosign), [`aqua.minisign`](/configuration/settings.html#aqua.minisign), [`aqua.slsa`](/configuration/settings.html#aqua.slsa), [`aqua.github_attestations`](/configuration/settings.html#aqua.github_attestations)                                                                                            |
| `github:`                   | GitHub artifact attestations when a github.com release publishes them; SLSA provenance when the tool names its signer                        | [`github.github_attestations`](/configuration/settings.html#github.github_attestations), [`github.slsa`](/configuration/settings.html#github.slsa), and the [`github_attestations`](/dev-tools/backends/github.html#github-attestations) and [`slsa_signer_identity`](/dev-tools/backends/github.html#slsa-signer-identity-and-slsa-signer-issuer) tool options |
| `packslip:`                 | A signed release manifest, artifact digests and a pinned signer; see [verification and policy](/dev-tools/packslip-verification.html)        | [Tool options](/dev-tools/backends/packslip.html#tool-options)                                                                                                                                                                                                                                                                                                  |
| Node.js                     | Checksums from `SHASUMS256.txt`, and its OpenPGP signature when the release publishes one                                                    | [`node.verify`](/configuration/settings.html#node.verify), [`node.gpg_verify`](/configuration/settings.html#node.gpg_verify)                                                                                                                                                                                                                                    |
| Swift on Linux              | OpenPGP signatures                                                                                                                           | [`swift.gpg_verify`](/configuration/settings.html#swift.gpg_verify)                                                                                                                                                                                                                                                                                             |
| Zig                         | Minisign signatures, always                                                                                                                  | None                                                                                                                                                                                                                                                                                                                                                            |
| Precompiled Ruby and Python | GitHub artifact attestations                                                                                                                 | [`ruby.github_attestations`](/configuration/settings.html#ruby.github_attestations), [`python.github_attestations`](/configuration/settings.html#python.github_attestations)                                                                                                                                                                                    |

These checks are on by default. Setting the global
[`github_attestations`](/configuration/settings.html#github_attestations) or
[`slsa`](/configuration/settings.html#slsa) setting to `false` turns that check
off for the `aqua:` and `github:` backends. For precompiled Python and Ruby,
`github_attestations` is only the default, and an explicit
`python.github_attestations` or `ruby.github_attestations` overrides it.

mise verifies OpenPGP, Cosign, Minisign, SLSA and attestations itself, so it
needs no `gpg`, `cosign` or `gh` executable. If a check fails, read the
underlying error before you turn a setting off; see
[checksum errors](/errors.html#checksum-mismatch).

A [lockfile](/dev-tools/mise-lock.html#provenance-and-security) records checksums
and the provenance mise verified, and later installs reuse that result instead
of checking again. Set
[`locked_verify_provenance`](/configuration/settings.html#locked_verify_provenance)
or turn on [paranoid mode](/paranoid.html) to re-verify on every install.
Neither re-checks a tool that is already installed.

mise checks its own releases too: `mise self-update` verifies each release's
signature before it replaces the binary. See
[self-update verification](/dev-tools/packslip-verification.html#self-update)
for the checks and [Updating mise](/installing-mise.html#updating) for the
commands.

## Minimum release age

mise skips versions published more recently than a cutoff when it picks a
version for a request such as `node@24` or `latest`. The default cutoff is 24
hours. The delay gives the community time to notice a compromised release; it
does not make an older release trustworthy.

```toml
[settings]
minimum_release_age = "7d" # raise the default 24h delay to 7 days
```

[`minimum_release_age`](/configuration/settings.html#minimum_release_age)
accepts relative durations (`7d`, `6mo`, `1y`) and dates (`2024-06-01`,
`2024-06-01T12:00:00Z`). Set `0s` to turn the delay off.

### Which backends have a default

Without a configured value, mise applies the 24-hour default to `aqua:`,
`cargo:`, core tools, `forgejo:`, `gem:`, `github:`, `gitlab:`, `go:`, `npm:`,
`packslip:`, `pypi:`, `spm:` and `ubi:` tools. Other backends, such as `http:`,
`s3:`, `conda:`, `dotnet:`, `asdf:` and `vfox:`, apply a cutoff only when you
set one.

A backend can filter only versions it knows a release date for:

| Capability                                       | Backends                                                                                                                        |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------- |
| Filter the tool's own versions                   | Backends that report release dates, such as `aqua:`, `cargo:`, `github:`, `gitlab:`, `go:`, `npm:`, `pypi:` and most core tools |
| Filter dependencies resolved during installation | `npm:` and `pypi:` (`pipx:` is an alias)                                                                                        |

Versions without a release date are not filtered. Other backends can hold back
the tool's own version, but not the dependencies its installer or compiler
fetches. See the [npm](/dev-tools/backends/npm.html) and
[PyPI](/dev-tools/backends/pypi.html) backend pages for how they pass the cutoff
to their package managers.

### What the cutoff applies to

The cutoff applies when mise has to choose a version, for a prefix such as
`node@24` or for `latest`. These are not filtered:

- An exact version such as `node@24.0.0`.
- A version recorded in [`mise.lock`](/dev-tools/mise-lock.html), so a reviewed
  selection stays reproducible while the release is inside the window. `npm:`
  and `pypi:` still apply the cutoff to transitive dependencies that the lockfile
  does not record.
- An installed version that matches the request. The cutoff limits which new
  versions mise downloads; it does not deactivate one you have. `mise lock` with
  a configured cutoff, rather than the default, re-checks installed matches.

For `packslip:` tools, mise compares the cutoff with the release's verified
transparency-log time; see
[packslip verification](/dev-tools/packslip-verification.html#minimum-release-age).
The installer script and `mise self-update` apply their own delay to mise
releases; see [Installer options](/installing-mise.html#installer-options) and
[Updating mise](/installing-mise.html#updating).

### Per-tool cutoffs and exclusions

Set `minimum_release_age` as a tool option to override the global value for one
tool:

```toml
[settings]
minimum_release_age = "7d"

[tools.trivy]
version = "latest"
minimum_release_age = "1d" # security scanner updates are time-sensitive
```

The `--minimum-release-age` flag on commands such as `mise install`, `mise use`,
`mise upgrade` and `mise lock` overrides both. The order is: the flag, then the
tool option, then the setting or the built-in default.

To exempt tools from the setting and the default, list them in
[`minimum_release_age_excludes`](/configuration/settings.html#minimum_release_age_excludes):

```toml
[settings]
minimum_release_age = "7d"
minimum_release_age_excludes = ["trivy", "npm:*"]
```

An entry can be a backend wildcard (`npm:*`), a tool short name (`trivy`) or a
full backend ID (`npm:prettier`). A tool option or the flag still applies to an
excluded tool. Exclusions from several config files are merged, so a project can
add to the global list without repeating it.

## Secrets

Keep secret values out of `mise.toml` and out of your shell where you can.
[mise secrets](/environments/secrets/fnox.html) (experimental) fetch values from a
secret manager and pass them only to the tasks that list them. sops and age
values are decrypted into the environment like any other `[env]` value. See
[Secrets](/environments/secrets/) to choose an approach, and
[redaction](/environments/secrets/#redaction) to mask values in task output and
CI logs.

A config whose tasks list `secrets` always needs trust, and safe mode refuses
mise secrets. The [environment cache](/configuration/settings.html#env_cache)
never stores an environment that includes age values, sops files or redacted
values.

## Reporting a vulnerability

Report vulnerabilities privately by email, as described in
[SECURITY.md](https://github.com/jdx/mise/blob/main/SECURITY.md), not in a public
issue or discussion. SECURITY.md also has a PGP key for encrypting the report.
Fixes go into the latest release only.
