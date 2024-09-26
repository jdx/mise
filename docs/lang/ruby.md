# Ruby

The following are instructions for using the ruby mise core plugin. This is used when there isn't a
git plugin installed named "ruby".

If you want to use [asdf-ruby](https://github.com/asdf-vm/asdf-ruby)
then use `mise plugins install ruby GIT_URL`.

The code for this is inside the mise repository at
[`./src/plugins/core/ruby.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/ruby.rs).

## Usage

The following installs the latest version of ruby-3.2.x (if some version of 3.2.x is not already
installed) and makes it the global default:

```sh
mise use -g ruby@3.2
```

Behind the scenes, mise uses [`ruby-build`](https://github.com/rbenv/ruby-build) to compile ruby
from source. You can check its
[README](https://github.com/rbenv/ruby-build/blob/master/README.md)
for additional settings and some troubleshooting.

## Configuration

`ruby-build` already has a
[handful of settings](https://github.com/rbenv/ruby-build?tab=readme-ov-file#custom-build-configuration),
in additional to that mise has a few extra settings:

### `ruby.apply_patches`

* Type: `Option<String>`
* Env: `MISE_RUBY_APPLY_PATCHES`
* Default: `None`

A list of patches (files or URLs) to apply to the ruby source code.

### `ruby.verbose_install`

* Type: `Option<bool>`
* Env: `MISE_RUBY_VERBOSE_INSTALL`
* Default: `None` (behaves like `false` unless --verbose is passed to mise)

Show verbose output during installation (passes --verbose to ruby-build)

### `ruby.ruby_build_repo`

* Type: `String`
* Env: `MISE_RUBY_BUILD_REPO`
* Default: `https://github.com/rbenv/ruby-build.git`

The git repository to use for getting ruby-build.

### `ruby.ruby_build_opts`

* Type: `Option<String>`
* Env: `MISE_RUBY_BUILD_OPTS`
* Default: `None`

Command line options to pass to ruby-build when installing

### `ruby.ruby_install`

* Type: `bool`
* Env: `MISE_RUBY_INSTALL`
* Default: `false`

Build with [ruby-install](https://github.com/postmodern/ruby-install) instead of [ruby-build](https://github.com/rbenv/ruby-build).

### `ruby.ruby_install_repo`

* Type: `String`
* Env: `MISE_RUBY_INSTALL_REPO`
* Default: `https://github.com/postmodern/ruby-install.git`

The git repository to use for getting ruby-install.

### `ruby.ruby_install_opts`

* Type: `Option<String>`
* Env: `MISE_RUBY_INSTALL_OPTS`
* Default: `None`

Command line options to pass to ruby-install when installing (if MISE_RUBY_INSTALL=1)

### `ruby.default_packages_file`

* Type: `String`
* Env: `MISE_RUBY_DEFAULT_PACKAGES_FILE`
* Default: `$HOME/.default-gems`

Location of file containing a list of gems to install after installing a new ruby version.

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

mise uses a `.tool-versions` or `.mise.toml` file for auto-switching between software versions.
However it can also read ruby-specific version files `.ruby-version` or `Gemfile`
(if it specifies a ruby version).

Create a `.ruby-version` file for the current version of ruby:

```sh
ruby -v > .ruby-version
```

### Manually updating ruby-build

ruby-build should update daily, however if you find versions do not yet exist you can force an
update:

```bash
mise cache clean
mise ls-remote ruby
```
