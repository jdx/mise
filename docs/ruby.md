# Ruby in rtx

The following are instructions for using the ruby rtx core plugin. This is used when there isn't a 
git plugin installed named "ruby".

If you want to use [asdf-ruby](https://github.com/asdf-vm/asdf-ruby)
or [rtx-ruby](https://github.com/rtx-plugins/rtx-ruby)
then use `rtx plugins install ruby GIT_URL`.

The code for this is inside the rtx repository at
[`./src/plugins/core/ruby.rs`](https://github.com/jdx/rtx/blob/main/src/plugins/core/ruby.rs).

## Usage

The following installs the latest version of ruby-3.2.x (if some version of 3.2.x is not already
installed) and makes it the global default:

```sh-session
$ rtx use -g ruby@3.2
```

Behind the scenes, rtx uses [`ruby-build`](https://github.com/rbenv/ruby-build) to compile ruby
from source. You can check its
[README](https://github.com/rbenv/ruby-build/blob/master/README.md)
for additional settings and some troubleshooting.

## Configuration

`ruby-build` already has a
[handful of settings](https://github.com/nodenv/node-build#custom-build-configuration),
in additional to that rtx has a few extra configuration variables:

- `RTX_RUBY_INSTALL` [bool]: Build with ruby-install instead of ruby-build
- `RTX_RUBY_APPLY_PATCHES` [string]: A list of patches (files or URLs) to apply to the ruby source code
- `RTX_RUBY_VERBOSE_INSTALL` [bool]: Show verbose output during installation (passes --verbose to ruby-build)
- `RTX_RUBY_BUILD_OPTS` [string]: Command line options to pass to ruby-build when installing
- `RTX_RUBY_INSTALL_OPTS` [string]: Command line options to pass to ruby-install when installing (if RTX_RUBY_INSTALL=1)
- `RTX_RUBY_DEFAULT_PACKAGES_FILE` [string]: location of default gems file, defaults to `$HOME/.default-gems`

## Default gems

rtx can automatically install a default set of gems right after installing a new ruby version. 
To enable this feature, provide a `$HOME/.default-gems` file that lists one gem per line, for 
example:

```
# supports comments
pry
bcat ~> 0.6.0 # supports version constraints
rubocop --pre # install prerelease version
```

## `.ruby-version` and `Gemfile` support

rtx uses a `.tool-versions` or `.rtx.toml` file for auto-switching between software versions.
However it can also read ruby-specific version files `.ruby-version` or `Gemfile`
(if it specifies a ruby version).

Create a `.ruby-version` file for the current version of ruby:

```sh-session
$ ruby -v > .ruby-version
```

### Manually updating ruby-build

ruby-build should update daily, however if you find versions do not yet exist you can force an 
update:

```bash
rtx cache clean
rtx ls-remote ruby
```
