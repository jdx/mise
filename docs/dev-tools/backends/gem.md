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

Install one gem from a specific registry instead of the configured default:

```toml
[tools]
"gem:internal-cli" = { version = "2.2.0", source = "https://gems.example.com/acme" }
```

Without this, the only way to install from a private registry is to make it the
machine's primary `gem sources` entry, which redirects every other `gem install`
on that machine as well.

The source applies to version resolution as well as installation, so `latest`
resolves against the same registry the gem is installed from.

### Authenticating

A private registry authenticates with basic-auth credentials on the source URL
itself. RubyGems reads the userinfo from the source it is fetching from; the
API key `gem signin` writes to `~/.gem/credentials` is for publishing commands
such as `gem push` and does not authenticate a download.

Keep the token out of the file by taking it from the environment, since tool
options are templated:

```toml
[tools]
"gem:internal-cli" = { version = "latest", source = "https://{{ env.GEM_TOKEN }}@gems.example.com" }
```

Some registries expect the token in the user position with no password, which
is what the example above does. Others want `user:token@host`. Follow whichever
your registry documents.

mise registers the credential for redaction, so it is replaced with
`[redacted]` wherever mise renders the source: log output, `MISE_LOG_FILE`, the
`gem install` command line, error messages, and the gem command's own output.
Credentials are also stripped from the URL before it is recorded in mise's
install metadata, so no token is written under the data directory. Even so,
prefer a token scoped to reading that registry, since the rendered value does
exist in the process environment and in whatever supplies it.

Dependencies are still resolved from the other configured sources, so a private
gem whose dependencies live on rubygems.org installs normally. `source` is added
to the source list rather than replacing it, which is what makes that work.

That cuts both ways, and it is worth being plain about the consequence. mise
pins the version it resolved from your registry, but `--source` appends, so
rubygems.org is still in RubyGems' source list and a public gem of the same
name and version can satisfy the install. Whoever holds that name publicly can
therefore influence what a private `latest` installs, and if the name is
already taken you cannot fix it by choosing a different one.

**Prefer a registry that proxies rubygems.org.** Point `source` at the proxy so
the private gem and its dependencies both resolve from one place, and no other
source is in play. Where that is not possible, use a private gem name unlikely
to be claimed publicly, and pin an exact version rather than `latest`.
