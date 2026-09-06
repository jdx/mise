# Paranoid

Paranoid mode requires explicit content-bound trust for non-global configuration and rechecks supported
provenance during installation. Use it when you want configuration edits to require renewed
approval. It does not sandbox a command after you approve it; see [Security](/security.html)
for the scope of each control.

Enable it for one invocation with `MISE_PARANOID=1`, or persist it globally:

```sh
mise settings set paranoid true
```

To restore normal mode, run `mise settings set paranoid false`. The setting is global-only;
a project cannot enable or disable it for itself.

## Config files

In normal mode, simple configuration can load without trust, and execution commands such
as `mise run`, `mise install`, and `mise exec` automatically trust their active configuration.
Other commands may prompt, fail, or skip an untrusted file depending on how they discover it.
See [`mise trust`](/cli/trust.html) for normal-mode rules.

Paranoid mode requires explicit trust for every non-global config file, including formats
that normally do not need it. It hashes the contents, so editing a file requires renewed
trust. Automatic trust for execution commands and the usual CI trust exemption are disabled.
Trust is not shared between Git worktrees in this mode.

Inspect the file before accepting it:

```sh
mise trust --show
mise trust path/to/mise.toml
```

Replace the path with the configuration you reviewed. Trusting a broad directory is not a
substitute for content-bound approval in paranoid mode. Global and system configuration is
operator-owned and remains exempt, allowing paranoid mode itself to be set globally.

[Safe mode](/security.html#safe-mode) takes precedence if both modes are enabled. It suppresses
project execution and environment injection, so the configuration can load without a trust
prompt; syntax errors and refused operations still fail. Loading in safe mode does not grant
trust for a later normal or paranoid-mode invocation.

## Community plugins

Paranoid mode refuses to install an untrusted community plugin by short name
unless automatic confirmation is enabled with `--yes` or `MISE_YES=1`, mise is
running in CI, or the installation uses `--force`. A short-name plugin is trusted
when its resolved URL matches an asdf or vfox remote in mise's built-in registry,
or when it is maintained under the `mise-plugins` GitHub organization.

To install any other community plugin, specify its full Git repository URL on the
command line or in `[plugins]` configuration. Explicitly providing the URL bypasses
the registry trust check because you are choosing and trusting that source:

```sh
mise plugin install example https://github.com/example/asdf-example
```

In normal mode, mise may instead warn and ask for confirmation before installing
an untrusted community plugin by short name.

## Provenance re-verification

A supported backend can reuse a lockfile's recorded provenance when installing an artifact
whose checksum matches. This avoids repeating checks and API calls, and relies on the
lockfile having been generated correctly.

In paranoid mode, supported and enabled provenance methods (such as SLSA, Cosign, Minisign,
and GitHub attestations) run again during installation instead of being skipped because a
provenance entry exists. This can require network access. It does not add verification that
the backend does not support, or rescan an already-installed tool that mise skips.

This behavior can also be enabled independently via the
[`locked_verify_provenance`](/configuration/settings.html#locked_verify_provenance) setting.

## See also

- [Safe mode](/security.html#safe-mode) for processing untrusted project metadata.
- [Sandboxing](/sandboxing.html) for restrictions on executed commands.
- [Lockfiles](/dev-tools/mise-lock.html) for checksums, provenance, and backend coverage.
- [Contact](/contact.html) to suggest improvements or report unexpected behavior.
