---
description: "Install Ruby with mise from precompiled binaries or ruby-build, and select it per project."
---

# Ruby

mise installs [Ruby](https://www.ruby-lang.org/) from precompiled binaries where
they exist and compiles it with ruby-build otherwise. On Windows it installs
RubyInstaller2 builds.

## Quick start

Select Ruby for the current project and check its executable:

```sh
mise use ruby@4.0
mise exec -- ruby --version
```

`mise use` writes `ruby = "4.0"` to `mise.toml`. Use `mise use -g ruby@4.0` for
a personal default. In an existing Bundler project, run
`mise exec -- bundle install`, then prefix application commands with
`mise exec -- bundle exec`. See the [Ruby cookbook](/mise-cookbook/ruby.html)
for a Rails project.

## Choosing a version

| Request            | Selects                  |
| ------------------ | ------------------------ |
| `ruby@4.0`         | The newest 4.0.x release |
| `ruby@4.0.7`       | That release             |
| `ruby@latest`      | The newest CRuby release |
| `ruby@truffleruby` | The newest TruffleRuby   |
| `ruby@jruby`       | The newest JRuby         |

Implementations other than CRuby use the names of ruby-build's definitions and
always install through ruby-build (or ruby-install). List every version with
`mise ls-remote ruby`.

## Version files

mise can read `.ruby-version` and the Ruby version in a `Gemfile`. Enable them
for Ruby:

```sh
mise settings add idiomatic_version_file_enable_tools ruby
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead, so teammates get the same behavior. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

mise then reads `.ruby-version` (a leading `ruby-` is ignored), a
`ruby "4.0.7"` line in `Gemfile`, or Bundler's `ruby file: ".ruby-version"`,
which is resolved next to the `Gemfile`. To write `.ruby-version` from the
selected Ruby:

```sh
mise exec -- ruby -e 'puts RUBY_VERSION' > .ruby-version
```

## Gems and Bundler

`gem install` installs into the selected Ruby version, and so does
`bundle install` unless Bundler is configured with a `path`. Installed gems are
therefore not available after you switch versions. mise adds a RubyGems plugin
to each Ruby it installs. The plugin runs [`mise reshim`](/cli/reshim.html)
after gem and Bundler installs, so new executables get
[shims](/dev-tools/shims.html). While RubyGems runs, the plugin also adds the
Ruby's `lib/pkgconfig` directory to `PKG_CONFIG_PATH`, so native extensions
find the libraries bundled with precompiled Rubies. mise sets no Ruby
environment variables of its own.

To keep a Ruby CLI across Ruby versions, install it as its own tool with the
[gem backend](/dev-tools/backends/gem.html), for example
`mise use -g gem:rubocop`.

## How mise installs Ruby

### Precompiled binaries

By default mise downloads a precompiled CRuby from
[jdx/ruby](https://github.com/jdx/ruby) and verifies its GitHub artifact
attestation ([`ruby.github_attestations`](/lang/ruby.html#ruby.github_attestations)).
Precompiled binaries are available for:

- macOS on arm64 (Apple Silicon)
- Linux on arm64 and x86_64, with glibc (manylinux2014)

If no precompiled binary exists for your platform or Ruby version, mise
compiles Ruby with ruby-build instead. To always compile, set
[`ruby.compile`](/lang/ruby.html#ruby.compile) to `true`:

```sh
mise settings ruby.compile=true
```

jdx/ruby has no musl builds, so on musl systems such as Alpine mise compiles
Ruby. On Alpine and NixOS, mise also compiles by default, because
[`all_compile`](/configuration/settings.html#all_compile) defaults to `true`
there; this default is deprecated and changes in mise 2027.8.0. If you host
your own musl binaries with [`ruby.precompiled_url`](/lang/ruby.html#ruby.precompiled_url),
set [`ruby.precompiled_arch`](/lang/ruby.html#ruby.precompiled_arch) and
[`ruby.precompiled_os`](/lang/ruby.html#ruby.precompiled_os) to use them.

### Precompiled binaries only

Set `ruby.compile` to `false` on hosts without a build toolchain, where falling
back to ruby-build would fail late or pull in build dependencies:

```sh
mise settings ruby.compile=false
```

Installs then fail when no precompiled binary exists for the requested version
and platform. `mise ls-remote ruby` and prefixes such as `ruby = "4.0"` only
consider versions with a precompiled binary, so a prefix resolves to the newest
4.0.x that has one. With a custom `ruby.precompiled_url` template, mise cannot
list the available binaries and leaves version listings unfiltered.

### Rebuilt binaries {#precompiled-build-revisions}

jdx/ruby sometimes publishes a new build of a Ruby version without changing the
version, for example to fix packaging. Each build has its own release tag, such
as `3.3.11-1` or `3.3.11-2`; mise still treats the version as `3.3.11`. Without
`mise.lock`, an install picks the newest build. With `mise.lock`, the locked URL
fixes the build:

```toml [mise.lock]
[[tools.ruby]]
version = "3.3.11"

[tools.ruby."platforms.linux-x64"]
url = "https://github.com/jdx/ruby/releases/download/3.3.11-1/ruby-3.3.11.x86_64_linux.tar.gz"
```

To move to the newest build of the same version, delete the Ruby entry from
`mise.lock`, or every Ruby platform `url`, then run:

```sh
mise lock ruby
mise install --force ruby
```

Commit the updated `mise.lock` so other machines and CI use the same build.

### Compiling with ruby-build

Source builds use [ruby-build](https://github.com/rbenv/ruby-build) and need
its [build dependencies](https://github.com/rbenv/ruby-build/wiki#suggested-build-environment).
mise compares its copy of ruby-build with the latest ruby-build release before
every source build and updates it when they differ. Set
[`ruby.ruby_install`](/lang/ruby.html#ruby.ruby_install) to `true` to compile with
[ruby-install](https://github.com/postmodern/ruby-install) instead.

`ruby.ruby_build_cli_opts` passes flags to ruby-build itself, and
`ruby.ruby_build_opts` passes arguments to Ruby's `configure`. For example,
`--keep` preserves the source tree after installation, and
`RUBY_BUILD_BUILD_PATH` chooses where it is kept:

```toml [mise.toml]
[settings.ruby]
ruby_build_cli_opts = "--keep"

[env]
RUBY_BUILD_BUILD_PATH = "{{ config_root }}/.ruby-build"
```

Configure arguments, such as `--enable-yjit`, go in `ruby.ruby_build_opts`.
mise passes them after ruby-build's `--` separator:

```toml [mise.toml]
[settings.ruby]
ruby_build_opts = "--enable-yjit"
```

ruby-build also reads its own
[environment variables](https://github.com/rbenv/ruby-build#custom-build-configuration),
such as `RUBY_CONFIGURE_OPTS`, from the environment or from `install_env`.

### Windows

On Windows, mise installs [RubyInstaller2](https://rubyinstaller.org/) builds.
`ruby.compile` has no effect there.

## Migrating from other Ruby managers

Projects set up for rbenv, rvm or chruby usually have a `.ruby-version`; enable
it as described in [Version files](#version-files).

To reuse Rubies installed with Homebrew, link Homebrew's versioned `ruby@X.Y`
formulae into mise with [`mise sync ruby`](/cli/sync/ruby.html), then select
one with `mise use`:

```sh
mise sync ruby --brew
mise ls ruby --installed
```

## Troubleshooting

An installed plugin named `ruby` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

### Missing Ruby versions {#manually-updating-ruby-build}

Ruby's version list normally comes from mise's versions host and is cached for
[`fetch_remote_versions_cache`](/configuration/settings.html#fetch_remote_versions_cache).
If a version you expect is missing, clear the cache and list again:

```sh
mise cache clear ruby
mise ls-remote ruby
```

With [`use_versions_host`](/configuration/settings.html#use_versions_host) set
to `false`, or with `ruby.compile = false`, mise lists versions with ruby-build
itself and updates ruby-build first.

## Tool options

Ruby has no Ruby-specific options. `install_env` reaches ruby-build or
ruby-install, default gem installs and `postinstall` commands. Use it for a
per-project override; use `ruby.ruby_build_opts` for a default across projects:

```toml [mise.toml]
[tools]
ruby = { version = "4.0", install_env = { RUBY_CONFIGURE_OPTS = "--disable-install-doc" } }
```

Other generic options are described in [tool options](/dev-tools/#tool-options).

## Default gems file <Badge type="danger" text="deprecated" /> {#default-gems}

mise installs the gems listed in `~/.default-gems`
([`ruby.default_packages_file`](/lang/ruby.html#ruby.default_packages_file)) into each new
Ruby version. Each line names a gem, optionally followed by a version
constraint (`bcat ~> 0.6.0`) or `--pre`, and `#` starts a comment. mise warns
about this file from 2026.11.0 and stops reading it in 2027.11.0. Install Ruby
CLIs with the [gem backend](/dev-tools/backends/gem.html) instead, for example
`"gem:rubocop" = "latest"`, or use a
[`postinstall`](/dev-tools/#tool-postinstall-commands) command for gems every
Ruby version needs.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="ruby" :level="3" />
