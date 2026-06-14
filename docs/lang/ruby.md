# Ruby

Like `rvm`, `rbenv`, or `asdf`, `mise` can manage multiple versions of [Ruby](https://www.ruby-lang.org/) on the same system.

> The following are instructions for using the ruby mise core plugin. This is used when there isn't a
> git plugin installed named "ruby". If you want to use [asdf-ruby](https://github.com/asdf-vm/asdf-ruby)
> then use `mise plugins install ruby GIT_URL`.

The code for this is inside the mise repository at
[`./src/plugins/core/ruby.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/ruby.rs).

## Usage

The following installs the latest version of ruby-3.2.x (if some version of 3.2.x is not already
installed) and makes it the global default:

```sh
mise use -g ruby@3.2
```

Behind the scenes, mise uses [`ruby-build`](https://github.com/rbenv/ruby-build) to compile ruby
from source. Ensure that you have the necessary
[dependencies](https://github.com/rbenv/ruby-build/wiki#suggested-build-environment) installed.
You can check its [README](https://github.com/rbenv/ruby-build/blob/master/README.md) for additional settings and some
troubleshooting.

## Precompiled Binaries

Mise can download precompiled Ruby binaries instead of
compiling from source. This significantly reduces installation time.

Precompiled binaries will become the default in 2026.8.0. To opt in now:

```sh
mise settings ruby.compile=false
mise use ruby@3.4.1
```

Precompiled binaries are sourced from [jdx/ruby](https://github.com/jdx/ruby) and are available
for the following platforms:

- macOS (arm64/Apple Silicon only)
- Linux arm64
- Linux x86_64

If a precompiled binary is not available for your platform or Ruby version, mise automatically
falls back to compiling from source using ruby-build.

### Precompiled build revisions

Precompiled Ruby binaries are released from `jdx/ruby`. Sometimes the binary for a Ruby version is rebuilt without changing the Ruby version itself. Those rebuilds use build revision release tags like `3.3.11-1` or `3.3.11-2`.
Mise uses these build revision tags for `jdx/ruby` precompiled binaries instead
of the floating base release tag.

Rebuilds are for changes to the portable binary package, not changes to Ruby's
own version number. The `jdx/ruby` release history includes rebuilds for
reasons such as:

- native gem packaging fixes
- CA certificate lookup fixes
- RI documentation packaging changes
- SLSA/provenance workflow fixes
- mass regeneration of existing releases

This list is not exhaustive.

Mise still treats the Ruby version as `3.3.11`. Without a `mise.lock`, mise
uses the latest available precompiled build revision when resolving the install.
That means reinstalling the same Ruby version later may pick up a newer rebuild
if one was published.

With a `mise.lock`, the download URL records which precompiled build revision is
used:

```toml
[[tools.ruby]]
version = "3.3.11"

[tools.ruby.platforms.linux-x64]
url = "https://github.com/jdx/ruby/releases/download/3.3.11-1/ruby-3.3.11.x86_64_linux.tar.gz"
```

To see which precompiled build revision you have, inspect the release tag in the platform `url`:

- `/releases/download/3.3.11-1/` means build revision `1`
- `/releases/download/3.3.11-2/` means build revision `2`

If the lockfile already points at a build revision such as `3.3.11-1`, mise keeps using that exact revision for reproducibility. To update to the newest precompiled build revision for the same Ruby version, remove the entire Ruby entry from `mise.lock` or remove every Ruby platform `url`, then regenerate the lock entry and reinstall:

```sh
mise lock ruby
mise install --force ruby
```

Commit the updated `mise.lock` so other machines and CI use the same precompiled build revision.

To always compile from source even when precompiled binaries are available:

```sh
mise settings ruby.compile=true
```

You can also use a custom source for precompiled binaries by setting `ruby.precompiled_url` to
either a GitHub repo (e.g., `owner/repo`) or a full URL template.

You can also install a specific ruby flavour. To get the latest version from a flavour, just use the
flavour prefix.

```sh
mise use -g ruby@truffleruby            # latest version of truffleruby
```

## Default gems

::: warning Planned deprecation
Default package files are deprecated. They are still supported for now, but mise will start warning
in `2026.11.0` and support will be removed in `2027.11.0`.

For Ruby CLIs, install the tool directly with the [gem backend](/dev-tools/backends/gem.html):

```toml
[tools]
"gem:rubocop" = "latest"
```

For gems that really should be installed into every Ruby version, use a tool-level `postinstall`
hook:

```toml
[tools]
ruby = { version = "3.4", postinstall = "gem install rubocop" }
```

:::

mise can automatically install a default set of gems right after installing a new ruby version.
To use this legacy feature, provide a `$HOME/.default-gems` file that lists one gem per line, for
example:

```text
# supports comments
pry
bcat ~> 0.6.0 # supports version constraints
rubocop --pre # install prerelease version
```

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `ruby` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for ruby-build or ruby-install and default gem installation:

```toml
[tools]
ruby = { version = "latest", install_env = { RUBY_CONFIGURE_OPTS = "--disable-install-doc" } }
```

## `.ruby-version` and `Gemfile` support

mise uses a `mise.toml` or `.tool-versions` file for auto-switching between software versions.
However, it can also read ruby-specific version files `.ruby-version` or `Gemfile`
(if it specifies a ruby version).

Create a `.ruby-version` file for the current version of ruby:

```sh
ruby -v > .ruby-version
```

Enable idiomatic version file reading for ruby:

```sh
mise settings add idiomatic_version_file_enable_tools ruby
```

See [idiomatic version files](/configuration.html#idiomatic-version-files) for more information.

## Manually updating ruby-build

ruby-build should update daily, however if you find versions do not yet exist you can force an
update:

```bash
mise cache clean
mise ls-remote ruby
```

## Settings

`ruby-build` already has a
[handful of settings](https://github.com/rbenv/ruby-build?tab=readme-ov-file#custom-build-configuration),
in additional to that mise has a few extra settings:

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="ruby" :level="3" />
