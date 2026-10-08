---
description: "Install Ruby command-line applications from RubyGems, each in its own directory and GEM_HOME."
---

# gem backend

The `gem` backend installs Ruby command-line applications from
[RubyGems](https://rubygems.org/), or another gem registry, with `gem install`.
Each tool gets its own directory. Keep your application's gems in its `Gemfile`
and install them with Bundler.

## Requirements

<span id="dependencies"></span>

Install Ruby, which provides the `gem` command. When `ruby` is in your config,
mise installs it before your gems. Gems with native extensions also need a
compiler and the libraries the gem builds against.

## Usage

Declare Ruby and RuboCop in the same project:

```sh
mise use ruby@3.4 gem:rubocop
mise exec -- rubocop --version
```

This writes both entries to `mise.toml`. Add `-g` to `mise use` for your global
config.

```toml
[tools]
ruby = "3.4"
"gem:rubocop" = "latest"
```

mise sets `GEM_HOME` to the tool's own directory, so a `gem:` install cannot
see your bundle's gems. If your `.rubocop.yml` loads plugins such as
`rubocop-rails`, declare RuboCop and its plugins in your `Gemfile` and run
`bundle exec rubocop` instead.

## After a Ruby upgrade {#ruby-upgrades}

A gem is installed against one Ruby. After switching to another Ruby minor
version (for example 3.3 to 3.4), or when the gem has native extensions,
reinstall it with the new Ruby selected:

```sh
mise install --force gem:rubocop
```

Patch upgrades of a mise-managed Ruby keep working on Unix, because mise points
the gem's executables at Ruby's minor-version path (for example
`.../ruby/3.4/bin/ruby`). Gems installed with a system Ruby run whichever
`ruby` is first on `PATH`.

## Tool options

Set these on the tool's entry in `[tools]`. Options every backend accepts are
described under [tool options](/dev-tools/#tool-options).

### `install_env`

Set environment variables for `gem install`. For gems that build native
extensions, `MAKEFLAGS` controls parallel make jobs:

```toml
[tools]
"gem:rubocop" = { version = "latest", install_env = { MAKEFLAGS = "-j4" } }
```

### `source`

Install one gem, and list its versions, from a specific registry without
changing the machine's `gem sources`:

```toml
[tools]
"gem:internal-cli" = { version = "latest", source = "https://{{ env.GEM_TOKEN }}@gems.example.com/acme" }
```

Private registries take credentials as basic auth in the URL (`token@host` or
`user:token@host`, depending on the registry). mise redacts them from output and
install metadata. A source with credentials must use `https` unless it is on
`localhost`.

An `https` source on `rubygems.pkg.github.com` without credentials uses mise's
GitHub token (see [`mise token github`](/cli/token/github.html)). The token
needs the `read:packages` scope, which a default `gh auth login` lacks
(`gh auth refresh -s read:packages`). GitHub Packages cannot list versions, so
pin one:

```toml
[tools]
"gem:internal-cli" = { version = "1.4.2", source = "https://rubygems.pkg.github.com/acme" }
```

mise adds this source alongside rubygems.org; it does not replace it.
Dependencies still come from rubygems.org, and so can a public gem that has the
same name as your private one. To rule that out, pin an exact version or use a
registry that proxies rubygems.org.

## Troubleshooting

| Symptom                                          | What to do                                                                            |
| ------------------------------------------------ | ------------------------------------------------------------------------------------- |
| A gem stops working after a Ruby upgrade         | Reinstall it with `mise install --force`; see [after a Ruby upgrade](#ruby-upgrades). |
| A native extension fails to build                | Install the compiler and libraries the gem needs, then install again.                 |
| RuboCop cannot load a plugin from your `Gemfile` | Run it with `bundle exec` from your project; a `gem:` install has its own `GEM_HOME`. |

Implementation: [`src/backend/gem.rs`](https://github.com/jdx/mise/blob/main/src/backend/gem.rs).
