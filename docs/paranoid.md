# Paranoid

Paranoid is an optional behavior that locks mise down more to make it harder
for a bad actor to compromise your system. These are settings that I
personally do not use on my own system because I find the behavior too
restrictive for the benefits.

Paranoid mode can be enabled with either `MISE_PARANOID=1` or a global setting:

```sh
mise settings paranoid=1
```

The setting is global-only, so a project config cannot enable or disable paranoid
mode for itself.

## Config files

Normally `mise` will make sure some config files are "trusted" before loading
them. This can prompt you to confirm that you want to load the file, e.g.:

```sh
$ mise env
mise ~/src/mise/mise.toml is not trusted. Trust it [y/n]?
```

In normal mode, `mise run`, naked task invocations such as `mise <TASK>`,
`mise install`, `mise exec`, and `mise watch` automatically trust their active
config because they explicitly execute project-defined behavior. Automatic shell
activation through `hook-env` does not.

Other commands check trust before parsing `mise.toml` files because they can
contain behavior that executes code or affects the environment. Some discovery
paths that look at previously tracked configs may skip untrusted files instead
of prompting. Commands that directly need an untrusted config can fail with an
untrusted-config error when mise cannot prompt. When mise detects that it is
running in CI, configs are assumed to be trusted unless paranoid mode is enabled.

Under paranoid, all config files must be trusted first, including formats that
normally do not require trust. Automatic trust for execution commands is disabled.
In normal mode, a config file only needs to be trusted a single time. In paranoid,
the contents of the file are hashed to check if it changes.
If you change your config file, you'll need to trust it again.

Note that global and system config files (e.g., `~/.config/mise/config.toml`) are implicitly trusted and exempt from this check. This allows paranoid mode to be enabled in a global config without requiring a trust prompt for that file itself.

[Safe mode](/security.html#safe-mode) takes precedence when both modes are enabled.
Safe mode disables project-defined code execution and environment injection while
retaining non-executable configuration such as tool definitions, task metadata,
plugin declarations, and tool aliases. It therefore loads untrusted config without
a trust prompt or untrusted-config error. Other configuration errors are still
reported.

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

Normally, when a lockfile contains both a checksum and a provenance entry for a tool,
`mise install` trusts the lockfile and skips provenance re-verification to avoid
redundant API calls (e.g., to GitHub). This is safe when you trust the lockfile was
generated correctly.

In paranoid mode, `mise install` always re-verifies provenance (SLSA, cosign, minisign,
GitHub artifact attestations) at install time, even when the lockfile already has a
provenance entry. This ensures that cryptographic verification happens on every install,
not just when the lockfile is first generated.

This behavior can also be enabled independently via the
[`locked_verify_provenance`](/configuration/settings.html#locked_verify_provenance) setting.

## See also

[Safe mode](/security.html#safe-mode) (`MISE_SAFE=1`) is a related but distinct
control: paranoid tightens _trust_ (which configs are loaded and re-verified),
while safe mode is a hard boundary on _code execution_ for running mise against
configuration you do not control.

## More?

If you have suggestions for more that could be added to paranoid, please let
me know.
