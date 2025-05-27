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

## Default gems

mise can automatically install a default set of gems right after installing a new ruby version.
To enable this feature, provide a `$HOME/.default-gems` file that lists one gem per line, for
example:

```text
# supports comments
pry
bcat ~> 0.6.0 # supports version constraints
rubocop --pre # install prerelease version
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
