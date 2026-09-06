---
editLink: false
---

# Registry

<script setup>
import Registry from '/components/registry.vue';
</script>

The registry maps short tool names to one or more installation backends. Search the
[tool list](#tools) below, or inspect the registry bundled with your installed mise:

```sh
mise registry
mise registry aws-cli
mise use aws-cli
```

A shorthand can carry backend-specific options as well as a backend name. For example,
`aws-cli` currently selects Aqua with registry-provided executable-link options. Choosing
`aqua:aws/aws-cli` explicitly selects that backend, but does not mean every shorthand option
is identical.

If a tool has no shorthand, use its full [backend identifier](/dev-tools/backends/), such as
`github:owner/repo`. The backend must support that project's release layout or package
format; a repository existing on GitHub is not sufficient by itself.

## Floating registries

By default, mise uses the mise and aqua registry snapshots that were tested and bundled with that
mise release. Users whose system package manager ships mise updates slowly can opt in to current
registry data without replacing the mise executable:

```shell
mise settings set registry_floating true
```

With this enabled, mise fetches the shorthand registry published with the latest mise release and
the current official aqua registry. The bundled snapshots remain available as fallbacks when the
remote registries cannot be loaded. Fast and offline commands never refresh the mise registry; they
use an existing cached copy or the bundled snapshot.
The mise registry is cached for [`registry_cache_ttl`](/configuration/settings.html#registry_cache_ttl),
which defaults to one hour; aqua continues to use
[`aqua.registry_cache_ttl`](/configuration/settings.html#aqua.registry_cache_ttl), which defaults to
one week. `mise cache clear` forces both to be downloaded again on their next online use.

This behavior is opt-in because a floating registry may contain changes that were made after the
installed mise version was tested. Updating mise remains preferable when an updated package is
available.

## Backends

In addition to built-in [core tools](/core-tools.html), `mise` supports a variety of [backends](/dev-tools/backends/) to install tools.

These tiers apply to **new registry submissions**, not to backends you may use in your
own configuration. New entries must already be widely used, normally with thousands of
GitHub stars, and must list installable versions. See [Contributing](/contributing.html)
before submitting a shorthand. Backend availability alone does not qualify a tool.

Backends fall into the following acceptance tiers for new registry entries:

**Tier 1 — preferred, routinely accepted:**

- [packslip](./dev-tools/backends/packslip.html) - preferred when the project publishes signed release manifests; verifies the signer and artifact digests without a plugin or separate package manager

**Tier 2 — routinely accepted:**

- [aqua](./dev-tools/backends/aqua.html) - curated registry metadata, SLSA verification, and per-version logic for tools without packslips
- [github](./dev-tools/backends/github.html) - for tools that are not available in the aqua registry, but are available on GitHub
- [gitlab](./dev-tools/backends/gitlab.html) - for tools that are not available in the aqua registry, but are available on GitLab

**Tier 3 — high bar, but lower than tier 4:**

- [conda](./dev-tools/backends/conda.html) - potentially accepted for tools that can't reasonably be supported via packslip/aqua/github/gitlab. The bar is lower than tier 4 because mise's conda backend does not require a separately-installed package manager — packages are fetched and extracted directly from anaconda.org with no `conda`/`mamba`/`micromamba` needed on PATH.

**Tier 4 — very high bar, rarely accepted:**

- [pipx](./dev-tools/backends/pipx.html) - Python applications; uses uv by default, which can provision Python
- [npm](./dev-tools/backends/npm.html) - only for node tools, requires `node` on PATH
- [gem](./dev-tools/backends/gem.html) - only for ruby tools, requires `ruby` on PATH
- [go](./dev-tools/backends/go.html) - only for go tools, requires `go` to be installed to compile. Because go tools can be distributed as a single binary, packslip/aqua/github/gitlab are preferred.
- [cargo](./dev-tools/backends/cargo.html) - only for rust tools, requires `cargo` to be installed to compile. Because rust tools can be distributed as a single binary, packslip/aqua/github/gitlab are preferred.
- [dotnet](./dev-tools/backends/dotnet.html) - only for dotnet tools, requires `dotnet` to be installed to compile. Because dotnet tools can be distributed as a single binary, packslip/aqua/github/gitlab are preferred.

These integrations depend on a language runtime or toolchain and can require extra setup.
For example, npm tools need Node at runtime, and Ruby gems depend on the Ruby installation
used to install them. Prefer release binaries for registry entries when available; consult
each backend page for its actual runtime, installer, and binary-download behavior.

**Not accepted:**

- New `vfox` and `asdf` tools are not accepted for supply-chain security reasons — use [`packslip`](./dev-tools/backends/packslip.html) (preferred), [`aqua`](./dev-tools/backends/aqua.html), [`github`](./dev-tools/backends/github.html), or [`gitlab`](./dev-tools/backends/gitlab.html) instead.
- The `ubi` backend is deprecated and is not accepted for new registry entries.

Users can still install via any backend themselves with explicit syntax (`mise use vfox:owner/repo`, `mise use cargo:name`, etc.) — they just don't get a registry shorthand for it.

### Backends Priority

An explicitly installed plugin can override its matching shorthand. Otherwise, a shorthand
lists backends in preference order. Platform support, disabled backends, and
version boundaries affect which eligible backend is selected. If you would like to disable a backend, you can do so with the following command:

```shell
mise settings set disable_backends asdf
```

This replaces the configured disabled-backend list with `asdf`; include any other backends
you want disabled in the same comma-separated value. It disables the [asdf](./dev-tools/backends/asdf.html) backend. See [Aliases](/dev-tools/aliases.html) for a way to set a default backend for a tool. Note that the `asdf` backend is disabled by default on Windows.

You can also specify the full name for a tool using `mise use aqua:1password/cli` if you want to use a specific backend.

### Version-specific backends

A registry backend can declare the first tool version it supports. mise skips
it for older version requests and uses the next eligible backend. For example,
`mise use hk@1.58.0` uses Aqua, while `mise use hk@1.58.1` uses Packslip.
Older prefixes such as `hk@1.57` also use Aqua; `latest` and prefixes spanning
the boundary keep the normal backend priority.

These boundaries apply to registry shorthands. You can still choose a backend
explicitly, and matching lockfile entries preserve their recorded backend.
See [minimum backend versions](/contributing.html#minimum-backend-versions)
for the registry format.

### Environment Variable Overrides

You can override the backend for any tool using environment variables with the pattern `MISE_BACKENDS_<TOOL>`. This takes the highest priority and overrides any registry or alias configuration:

```shell
# Use vfox backend for php
export MISE_BACKENDS_PHP='vfox:mise-plugins/vfox-php'
mise install php@latest
```

The tool name in the environment variable should be in SHOUTY_SNAKE_CASE (uppercase with underscores). For example, `my-tool` becomes `MISE_BACKENDS_MY_TOOL`.

Source: <https://github.com/jdx/mise/blob/main/registry/>

## Tools {#tools}

Note that [`mise registry`](/cli/registry.html) can be used to list all tools in the registry. [`mise use`](/cli/use.html) without any arguments will show a `tui` to select a tool to install.

<Registry />
