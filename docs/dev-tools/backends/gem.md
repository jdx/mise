---
description: "Install Ruby command-line applications from RubyGems into separate tool directories."
---

# gem Backend

The `gem` backend installs Ruby command-line applications from RubyGems into
separate tool directories. Keep application gems in your project's `Gemfile` and
install them with Bundler. The code for this is inside of the mise repository at [`./src/backend/gem.rs`](https://github.com/jdx/mise/blob/main/src/backend/gem.rs).

## Dependencies

This backend needs Ruby and its `gem` command. Gems with native extensions also
need the compiler and libraries required by that gem.

## Usage

Declare Ruby and RuboCop in the same project:

```sh
mise use ruby@3.4 gem:rubocop
mise exec -- rubocop --version
```

This writes both entries to `mise.toml`. Add `-g` for global configuration.

```toml
[tools]
ruby = "3.4"
"gem:rubocop" = "latest"
```

mise's wrappers set `GEM_HOME` for the selected tool. A RuboCop configuration
that uses project-specific plugins may be better run with `bundle exec rubocop`
from a Gemfile that declares those plugins.

## Ruby upgrades

If the Ruby version used by a gem package changes (whether managed by mise or the system), you may need to
reinstall the gem. This can be done with:

```sh
mise install -f gem:rubocop
```

Reinstall under the Ruby version you intend to use. On Unix, mise-managed Ruby
shebangs follow a minor-version path so patch upgrades can keep working; moving
to another minor version or changing native-extension compatibility can still
require a reinstall.

## Settings

Set these with `mise settings set [VARIABLE]=[VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="gem" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `gem` backend—these
go in `[tools]` in `mise.toml`.

### `install_env`

Set environment variables for the `gem install` command. For gems that build
native extensions, `MAKEFLAGS` controls parallel make jobs:

```toml
[tools]
"gem:rubocop" = { version = "latest", install_env = { MAKEFLAGS = "-j4" } }
```

### `source`

Install and resolve versions of one gem from a specific registry, without
changing the machine's `gem sources`:

```toml
[tools]
"gem:internal-cli" = { version = "latest", source = "https://{{ env.GEM_TOKEN }}@gems.example.com/acme" }
```

Private registries take credentials as basic auth in the URL (`token@host` or
`user:token@host`, per your registry). mise redacts them from output and install
metadata. A source with credentials must use `https` unless it is on
`localhost`.

An `https` source on `rubygems.pkg.github.com` without credentials uses
mise's GitHub token (see `mise token github`). It needs `read:packages`, which
a default `gh auth login` lacks (`gh auth refresh -s read:packages`). GitHub
Packages can't list versions, so pin one:

```toml
[tools]
"gem:internal-cli" = { version = "1.4.2", source = "https://rubygems.pkg.github.com/acme" }
```

The source is added to the other sources rather than replacing them, so
dependencies still resolve from rubygems.org, and so can a public gem with the
same name. Prefer a registry that proxies rubygems.org, or pin an exact version.
