<div align="center">
<h1><a href="https://github.com/jdxcode/rtx">rtx</a></h1>
<a href="https://crates.io/crates/rtx-cli"><img alt="Crates.io" src="https://img.shields.io/crates/v/rtx-cli?style=for-the-badge"></a>
<a href="https://github.com/jdxcode/rtx/blob/main/LICENSE"><img alt="GitHub" src="https://img.shields.io/github/license/jdxcode/rtx?color=%2320A920&style=for-the-badge"></a>
<a href="https://github.com/jdxcode/rtx/actions/workflows/rtx.yml"><img alt="GitHub Workflow Status" src="https://img.shields.io/github/actions/workflow/status/jdxcode/rtx/rtx.yml?color=%2320A920&style=for-the-badge"></a>
<a href="https://codecov.io/gh/jdxcode/rtx"><img alt="Codecov" src="https://img.shields.io/codecov/c/github/jdxcode/rtx?color=%2320A920&style=for-the-badge"></a>
<a href="https://discord.gg/mABnUDvP57"><img alt="Discord" src="https://img.shields.io/discord/1066429325269794907?color=%23738ADB&style=for-the-badge"></a>
<p><em>Polyglot runtime manager (asdf rust clone)</em></p>
</div>

## Features

- **asdf-compatible** - rtx is compatible with asdf plugins and `.tool-versions` files. It can be used as a drop-in replacement.
- **Polyglot** - compatible with any language, so no more figuring out how nvm, nodenv, pyenv, etc work individually—just use 1 tool.
- **Fast** - rtx is written in Rust and is very fast. 20x-200x faster than asdf.
- **No shims** - shims cause problems, they break `which`, and add overhead. By default, rtx
  does not use them—however you can if you want to.
- **Fuzzy matching and aliases** - It's enough to just say you want "v20" of node, or the "lts"
  version. rtx will figure out the right version without you needing to specify an exact version.
- **Arbitrary env vars** - Set custom env vars when in a project directory like `NODE_ENV=production` or `AWS_PROFILE=staging`.

## 30 Second Demo

The following shows using rtx to install different versions
of [node](https://nodejs.org).
Note that calling `which node` gives us a real path to node, not a shim.

[![demo](./docs/demo.gif)](./docs/demo.gif)

## Quickstart

Install rtx on macOS (other methods [here](#installation)):

```sh-session
$ curl https://rtx.pub/rtx-latest-macos-arm64 > ~/bin/rtx
$ chmod +x ~/bin/rtx
$ rtx --version
rtx 1.34.2
```

Hook rtx into to your shell (pick the right one for your shell):

```sh-session
echo 'eval "$(~/bin/rtx activate bash)"' >> ~/.bashrc
echo 'eval "$(~/bin/rtx activate zsh)"' >> ~/.zshrc
echo '~/bin/rtx activate fish | source' >> ~/.config/fish/config.fish
```

> **Warning**
>
> If you use direnv with `layout python` or other logic that needs to reference rtx runtimes inside
> of an `.envrc`, see the [direnv section](#direnv) below.

Install a runtime and set it as the global default:

```sh-session
$ rtx use --global node@20
$ node -v
v20.0.0
```

## Table of Contents

<!-- AUTO-GENERATED-CONTENT:START (TOC:collapse=true&collapseText=Click to expand&maxDepth=3) -->
<details>
<summary>Click to expand</summary>

- [Features](#features)
- [30 Second Demo](#30-second-demo)
- [Quickstart](#quickstart)
- [About](#about)
  - [How it works](#how-it-works)
  - [Common commands](#common-commands)
- [Installation](#installation)
  - [Download binary](#download-binary)
  - [Register shell hook](#register-shell-hook)
- [Uninstalling](#uninstalling)
- [Shebang](#shebang)
- [Configuration](#configuration)
  - [`.rtx.toml`](#rtxtoml)
  - [Legacy version files](#legacy-version-files)
  - [`.tool-versions`](#tool-versions)
  - [Scopes](#scopes)
  - [Global config: `~/.config/rtx/config.toml`](#global-config-configrtxconfigtoml)
  - [Environment variables](#environment-variables)
- [Aliases](#aliases)
- [Plugins](#plugins)
  - [Plugin Options](#plugin-options)
- [Versioning](#versioning)
  - [Calver Breaking Changes](#calver-breaking-changes)
- [Directories](#directories)
  - [`~/.config/rtx`](#configrtx)
  - [`~/.cache/rtx`](#cachertx)
  - [`~/.local/share/rtx`](#localsharertx)
- [Templates](#templates)
- [&#91;experimental&#93; Config Environments](#experimental-config-environments)
- [IDE Integration](#ide-integration)
- [Core Plugins](#core-plugins)
- [FAQs](#faqs)
  - [I don't want to put a `.tool-versions` file into my project since git shows it as an untracked file.](#i-dont-want-to-put-a-tool-versions-file-into-my-project-since-git-shows-it-as-an-untracked-file)
  - [What is the difference between "nodejs" and "node" (or "golang" and "go")?](#what-is-the-difference-between-nodejs-and-node-or-golang-and-go)
  - [What does `rtx activate` do?](#what-does-rtx-activate-do)
  - [`rtx activate` doesn't work in `~/.profile`, `~/.bash_profile`, `~/.zprofile`](#rtx-activate-doesnt-work-in-profile-bash_profile-zprofile)
  - [rtx is failing or not working right](#rtx-is-failing-or-not-working-right)
  - [Windows support?](#windows-support)
  - [How do I use rtx with http proxies?](#how-do-i-use-rtx-with-http-proxies)
  - [How do the shorthand plugin names map to repositories?](#how-do-the-shorthand-plugin-names-map-to-repositories)
  - [Does "node@20" mean the newest available version of node?](#does-node20-mean-the-newest-available-version-of-node)
  - [How do I migrate from asdf?](#how-do-i-migrate-from-asdf)
  - [How compatible is rtx with asdf?](#how-compatible-is-rtx-with-asdf)
  - [rtx isn't working when calling from tmux or another shell initialization script](#rtx-isnt-working-when-calling-from-tmux-or-another-shell-initialization-script)
  - [How do I disable/force CLI color output?](#how-do-i-disableforce-cli-color-output)
  - [Is rtx secure?](#is-rtx-secure)
- [Comparison to asdf](#comparison-to-asdf)
  - [Performance](#performance)
  - [Environment variables in rtx](#environment-variables-in-rtx)
  - [UX](#ux)
  - [CI/CD](#cicd)
  - [GitHub Actions](#github-actions)
- [Shims](#shims)
- [direnv](#direnv)
  - [rtx inside of direnv (`use rtx` in `.envrc`)](#rtx-inside-of-direnv-use-rtx-in-envrc)
  - [Do you need direnv?](#do-you-need-direnv)
- [Cache Behavior](#cache-behavior)
  - [Plugin/Runtime Cache](#pluginruntime-cache)
- [Commands](#commands)
  - [`rtx activate [OPTIONS] [SHELL_TYPE]`](#rtx-activate-options-shell_type)
  - [`rtx alias get <PLUGIN> <ALIAS>`](#rtx-alias-get-plugin-alias)
  - [`rtx alias ls [OPTIONS]`](#rtx-alias-ls-options)
  - [`rtx alias set <PLUGIN> <ALIAS> <VALUE>`](#rtx-alias-set-plugin-alias-value)
  - [`rtx alias unset <PLUGIN> <ALIAS>`](#rtx-alias-unset-plugin-alias)
  - [`rtx bin-paths`](#rtx-bin-paths)
  - [`rtx cache clear`](#rtx-cache-clear)
  - [`rtx completion [SHELL]`](#rtx-completion-shell)
  - [`rtx current [PLUGIN]`](#rtx-current-plugin)
  - [`rtx deactivate`](#rtx-deactivate)
  - [`rtx direnv activate`](#rtx-direnv-activate)
  - [`rtx doctor`](#rtx-doctor)
  - [`rtx env [OPTIONS] [TOOL@VERSION]...`](#rtx-env-options-toolversion)
  - [`rtx env-vars [OPTIONS] [ENV_VARS]...`](#rtx-env-vars-options-env_vars)
  - [`rtx exec [OPTIONS] [TOOL@VERSION]... [-- <COMMAND>...]`](#rtx-exec-options-toolversion----command)
  - [`rtx implode [OPTIONS]`](#rtx-implode-options)
  - [`rtx install [OPTIONS] [TOOL@VERSION]...`](#rtx-install-options-toolversion)
  - [`rtx latest [OPTIONS] <TOOL@VERSION>`](#rtx-latest-options-toolversion)
  - [`rtx link [OPTIONS] <TOOL@VERSION> <PATH>`](#rtx-link-options-toolversion-path)
  - [`rtx ls [OPTIONS]`](#rtx-ls-options)
  - [`rtx ls-remote <TOOL@VERSION> [PREFIX]`](#rtx-ls-remote-toolversion-prefix)
  - [`rtx outdated [TOOL@VERSION]...`](#rtx-outdated-toolversion)
  - [`rtx plugins install [OPTIONS] [NAME] [GIT_URL]`](#rtx-plugins-install-options-name-git_url)
  - [`rtx plugins link [OPTIONS] <NAME> [PATH]`](#rtx-plugins-link-options-name-path)
  - [`rtx plugins ls [OPTIONS]`](#rtx-plugins-ls-options)
  - [`rtx plugins ls-remote [OPTIONS]`](#rtx-plugins-ls-remote-options)
  - [`rtx plugins uninstall <PLUGIN>...`](#rtx-plugins-uninstall-plugin)
  - [`rtx plugins update [PLUGIN]...`](#rtx-plugins-update-plugin)
  - [`rtx prune [OPTIONS] [PLUGINS]...`](#rtx-prune-options-plugins)
  - [`rtx reshim`](#rtx-reshim)
  - [`rtx self-update`](#rtx-self-update)
  - [`rtx settings get <KEY>`](#rtx-settings-get-key)
  - [`rtx settings ls`](#rtx-settings-ls)
  - [`rtx settings set <KEY> <VALUE>`](#rtx-settings-set-key-value)
  - [`rtx settings unset <KEY>`](#rtx-settings-unset-key)
  - [`rtx shell [OPTIONS] [TOOL@VERSION]...`](#rtx-shell-options-toolversion)
  - [`rtx sync node <--brew|--nvm|--nodenv>`](#rtx-sync-node---brew--nvm--nodenv)
  - [`rtx sync python --pyenv`](#rtx-sync-python---pyenv)
  - [`rtx trust [OPTIONS] [CONFIG_FILE]`](#rtx-trust-options-config_file)
  - [`rtx uninstall <TOOL@VERSION>...`](#rtx-uninstall-toolversion)
  - [`rtx upgrade [TOOL@VERSION]...`](#rtx-upgrade-toolversion)
  - [`rtx use [OPTIONS] [TOOL@VERSION]...`](#rtx-use-options-toolversion)
  - [`rtx version`](#rtx-version)
  - [`rtx where <TOOL@VERSION>`](#rtx-where-toolversion)
  - [`rtx which [OPTIONS] <BIN_NAME>`](#rtx-which-options-bin_name)

</details>
<!-- AUTO-GENERATED-CONTENT:END -->

## About

_New developer? Try reading the [Beginner's Guide](https://dev.to/jdxcode/beginners-guide-to-rtx-ac4) for a gentler introduction._

rtx is a tool for managing programming language and tool versions. For example, use this to install
a particular version of node.js and ruby for a project. Using `rtx activate`, you can have your
shell automatically switch to the correct node and ruby versions when you `cd` into the project's
directory. Other projects on your machine can use a different set of versions.

rtx is inspired by [asdf](https://asdf-vm.com) and uses asdf's vast [plugin ecosystem](https://github.com/asdf-vm/asdf-plugins)
under the hood. However, it is _much_ faster than asdf and has a more friendly user experience.
For more on how rtx compares to asdf, [see below](#comparison-to-asdf).

rtx can be configured in many ways. The most typical is by `.rtx.toml`, but it's also compatible
with asdf `.tool-versions` files. It can also use idiomatic version files like `.node-version` and
`.ruby-version`. See [Configuration](#configuration) for more.

### How it works

rtx hooks into your shell (with `rtx activate zsh`) and sets the `PATH`
environment variable to point your shell to the correct runtime binaries. When you `cd` into a
directory containing a `.tool-versions`/`.rtx.toml` file, rtx will automatically set the
appropriate tool versions in `PATH`.

After activating, every time your prompt displays it will call `rtx hook-env` to fetch new
environment variables.
This should be very fast. It exits early if the directory wasn't changed or `.tool-versions`/`.rtx.toml` files haven't been modified.

Unlike asdf which uses shim files to dynamically locate runtimes when they're called, rtx modifies
`PATH` ahead of time so the runtimes are called directly. This is not only faster since it avoids
any overhead, but it also makes it so commands like `which node` work as expected. This also
means there isn't any need to run `asdf reshim` after installing new runtime binaries.

You should note that rtx does not directly install these tools.
Instead, it leverages plugins to install runtimes.
See [plugins](#plugins) below.

### Common commands

    rtx install node@20.0.0  Install a specific version number
    rtx install node@20      Install a fuzzy version number
    rtx use node@20          Use node-20.x in current project
    rtx use -g node@20       Use node-20.x as global default

    rtx install node         Install the current version specified in .tool-versions/.rtx.toml
    rtx use node@latest      Use latest node in current directory
    rtx use -g node@system   Use system node as global default

    rtx x node@20 -- node app.js  Run `node app.js` with the PATH pointing to node-20.x

## Installation

Installing rtx consists of two steps.
1. Download the binary.
   This depends on the device and operating system you are running rtx in.
1. Register a shell hook.
   This depends on the shell you are using.
   Read more about this step in the [FAQ](#what-does-rtx-activate-do).

### Download binary

#### Standalone

Note that it isn't necessary for `rtx` to be on `PATH`. If you run the activate script in your rc
file, rtx will automatically add itself to `PATH`.

```
curl https://rtx.pub/install.sh | sh
```

If you want to verify the install script hasn't been tampered with:

```
gpg --keyserver hkps://keyserver.ubuntu.com --recv-keys 0x29DDE9E0
curl https://rtx.pub/install.sh.sig | gpg --decrypt > install.sh
# ensure the above is signed with the rtx release key
sh ./install.sh
```

or if you're allergic to `| sh`:

```
curl https://rtx.pub/rtx-latest-macos-arm64 > /usr/local/bin/rtx
```

It doesn't matter where you put it. So use `~/bin`, `/usr/local/bin`, `~/.local/share/rtx/bin/rtx`
or whatever.

Supported architectures:

- `x64`
- `arm64`

Supported platforms:

- `macos`
- `linux`

If you need something else, compile it with [cargo](#cargo).
[Windows isn't currently supported.](https://github.com/jdxcode/rtx/discussions/66)

#### Homebrew

```
brew install rtx
```

Alternatively, use the custom tap (which is updated immediately after a release)):

```
brew install jdxcode/tap/rtx
```

#### Cargo

Build from source with Cargo:

```
cargo install rtx-cli
```

Do it faster with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```
cargo install cargo-binstall
cargo binstall rtx-cli
```

Build from the latest commit in main:

```
cargo install rtx-cli --git https://github.com/jdxcode/rtx --branch main
```

#### npm

rtx is available on npm as a precompiled binary. This isn't a node.js package—just distributed
via npm. This is useful for JS projects that want to setup rtx via `package.json` or `npx`.

```
npm install -g rtx-cli
```

Use npx if you just want to test it out for a single command without fully installing:

```
npx rtx-cli exec python@3.11 -- python some_script.py
```

#### GitHub Releases

Download the latest release from [GitHub](https://github.com/jdxcode/rtx/releases).

```
curl https://github.com/jdxcode/rtx/releases/download/v1.34.2/rtx-v1.34.2-linux-x64 > /usr/local/bin/rtx
chmod +x /usr/local/bin/rtx
```

#### apt

For installation on Ubuntu/Debian:

```
wget -qO - https://rtx.pub/gpg-key.pub | gpg --dearmor | sudo tee /usr/share/keyrings/rtx-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/usr/share/keyrings/rtx-archive-keyring.gpg arch=amd64] https://rtx.pub/deb stable main" | sudo tee /etc/apt/sources.list.d/rtx.list
sudo apt update
sudo apt install -y rtx
```

> **Warning**
>
> If you're on arm64 you'll need to run the following:
>
> ```
> echo "deb [signed-by=/usr/share/keyrings/rtx-archive-keyring.gpg arch=arm64] https://rtx.pub/deb stable main" | sudo tee /etc/apt/sources.list.d/rtx.list
> ```

#### dnf

For Fedora, CentOS, Amazon Linux, RHEL and other dnf-based distributions:

```
dnf install -y dnf-plugins-core
dnf config-manager --add-repo https://rtx.pub/rpm/rtx.repo
dnf install -y rtx
```

#### yum

```
yum install -y yum-utils
yum-config-manager --add-repo https://rtx.pub/rpm/rtx.repo
yum install -y rtx
```

#### apk

For Alpine Linux:

```
apk add rtx
```

_rtx lives in the [community repository](https://gitlab.alpinelinux.org/alpine/aports/-/blob/master/community/rtx/APKBUILD)._

#### aur

For Arch Linux:

```
git clone https://aur.archlinux.org/rtx.git
cd rtx
makepkg -si
```

#### nix

For the Nix package manager, at release 23.05 or later:

```
nix-env -iA rtx
```

You can also import the package directly using
`rtx-flake.packages.${system}.rtx`. It supports all default Nix
systems.

### Register shell hook

#### Bash

```
echo 'eval "$(rtx activate bash)"' >> ~/.bashrc
```

#### Fish

```
echo 'rtx activate fish | source' >> ~/.config/fish/config.fish
```

#### Nushell

```nushell
do {
  let rtxpath = ($nu.config-path | path dirname | path join "rtx.nu")
  run-external rtx activate nu --redirect-stdout | save $rtxpath -f
  $"\nsource "($rtxpath)"" | save $nu.config-path --append
}
```

#### Xonsh

Since `.xsh` files are [not compiled](https://github.com/xonsh/xonsh/issues/3953) you may shave a bit off startup time by using a pure Python import: add the code below to, for example, `~/.config/xonsh/rtx.py` config file and `import rtx` it in `~/.config/xonsh/rc.xsh`:

```xsh
from pathlib        	import Path
from xonsh.built_ins	import XSH

ctx = XSH.ctx
rtx_init = subprocess.run([Path('~/bin/rtx').expanduser(),'activate','xonsh'],capture_output=True,encoding="UTF-8").stdout
XSH.builtins.execx(rtx_init,'exec',ctx,filename='rtx')
```

Or continue to use `rc.xsh`/`.xonshrc`:

```xsh
echo 'execx($(~/bin/rtx activate xonsh))' >> ~/.config/xonsh/rc.xsh # or ~/.xonshrc
```

Given that `rtx` replaces both shell env `$PATH` and OS environ `PATH`, watch out that your configs don't have these two set differently (might throw `os.environ['PATH'] = xonsh.built_ins.XSH.env.get_detyped('PATH')` at the end of a config to make sure they match)

#### Something else?

Adding a new shell is not hard at all since very little shell code is
in this project.
[See here](https://github.com/jdxcode/rtx/tree/main/src/shell) for how
the others are implemented. If your shell isn't currently supported
I'd be happy to help you get yours integrated.

## Uninstalling

Use `rtx implode` to uninstall rtx. This will remove the rtx binary and all of its data. Use
`rtx implode --help` for more information.

Alternatively, manually remove the following directories to fully clean up:

- `~/.local/share/rtx` (can also be `RTX_DATA_DIR` or `XDG_DATA_HOME/rtx`)
- `~/.config/rtx` (can also be `RTX_CONFIG_DIR` or `XDG_CONFIG_HOME/rtx`)
- on Linux: `~/.cache/rtx` (can also be `RTX_CACHE_DIR` or `XDG_CACHE_HOME/rtx`)
- on macOS: `~/Library/Caches/rtx` (can also be `RTX_CACHE_DIR`)

## Shebang

You can specify a tool and its version in a shebang without needing to first
setup `.tool-versions`/`.rtx.toml` config:

```typescript
#!/usr/bin/env -S rtx x node@20 -- node
// "env -S" allows multiple arguments in a shebang
console.log(`Running node: ${process.version}`);
```

This can also be useful in environments where rtx isn't activated
(such as a non-interactive session).

## Configuration

### `.rtx.toml`

`.rtx.toml` is a new config file that replaces asdf-style `.tool-versions` files with a file
that has lot more flexibility. It supports functionality that is not possible with `.tool-versions`, such as:

- setting arbitrary env vars while inside the directory
- passing options to plugins like `virtualenv='.venv'` for [rtx-python](https://github.com/jdxcode/rtx-python#virtualenv-support).
- specifying custom plugin urls

Here is what an `.rtx.toml` looks like:

```toml
[env]
# supports arbitrary env vars so rtx can be used like direnv/dotenv
NODE_ENV = 'production'

[tools]
# specify single or multiple versions
terraform = '1.0.0'
erlang = ['23.3', '24.0']

# supports everything you can do with .tool-versions currently
node = ['16', 'prefix:20', 'ref:master', 'path:~/.nodes/14']

# send arbitrary options to the plugin, passed as:
# RTX_TOOL_OPTS__VENV=.venv
python = {version='3.10', virtualenv='.venv'}

[plugins]
# specify a custom repo url
# note this will only be used if the plugin does not already exist
python = 'https://github.com/jdxcode/rtx-python'

[settings] # project-local settings
verbose = true

[alias.node] # project-local aliases
my_custom_node = '20'
```

`.rtx.toml` files are hierarchical. The configuration in a file in the current directory will
override conflicting configuration in parent directories. For example, if `~/src/myproj/.rtx.toml`
defines the following:

```toml
[tools]
node = '20'
python = '3.10'
```

And `~/src/myproj/backend/.rtx.toml` defines:

```toml
[tools]
node = '18'
ruby = '3.1'
```

Then when inside of `~/src/myproj/backend`, `node` will be `18`, `python` will be `3.10`, and `ruby`
will be `3.1`. You can check the active versions with `rtx ls --current`.

You can also have environment specific config files like `.rtx.production.toml`, see
[Config Environments](#experimental-config-environments) for more details.

#### `[env]` - Arbitrary Environment Variables

The `[env]` section of .rtx.toml allows setting arbitrary environment variables.
These can be simple key/value entries like this:

```toml
[env]
NODE_ENV = 'production'
```

`PATH` is treated specially, it needs to be defined as an array in `env_path`:

```toml
env_path = [
    # adds an absolute path
    "~/.local/share/bin",
    # adds a path relative to the .rtx.toml, not PWD
    "./node_modules/.bin",
]
```

_Note: `env_path` is a top-level key, it does not go inside of `[env]`._

Environment variable values can be templates, see [Templates](#templates) for details.

```toml
[env]
LD_LIBRARY_PATH = "/some/path:{{env.LD_LIBRARY_PATH}}"
```

`env_file` can be used to specify a [dotenv](https://dotenv.org) file to load:

```toml
env_file = '.env'
```

_Note: `env_file` goes at the top of the file, above `[env]`._

```toml
[env]
NODE_ENV = false # unset a previously set NODE_ENV
```

### Legacy version files

rtx supports "legacy version files" just like asdf. They're language-specific files like `.node-version`
and `.python-version`. These are ideal for setting the runtime version of a project without forcing
other developers to use a specific tool like rtx/asdf.

They support aliases, which means you can have an `.nvmrc` file with `lts/hydrogen` and it will work
in rtx and nvm. Here are some of the supported legacy version files:

| Plugin     | "Legacy" (Idiomatic) Files                         |
|------------|----------------------------------------------------|
| crystal    | `.crystal-version`                                 |
| elixir     | `.exenv-version`                                   |
| go         | `.go-version`, `go.mod`                            |
| java       | `.java-version`, `.sdkmanrc`                       |
| node       | `.nvmrc`, `.node-version`                          |
| python     | `.python-version`                                  |
| ruby       | `.ruby-version`, `Gemfile`                         |
| terraform  | `.terraform-version`, `.packer-version`, `main.tf` |
| yarn       | `.yarnrc`                                          |

In rtx these are enabled by default. You can disable them with `rtx settings set legacy_version_file false`.
There is a performance cost to having these when they're parsed as it's performed by the plugin in
`bin/parse-version-file`. However these are [cached](#cache-behavior) so it's not a huge deal.
You may not even notice.

> **Note**
>
> asdf calls these "legacy version files" so we do too. I think this is a bad name since it implies
> that they shouldn't be used—which is definitely not the case IMO. I prefer the term "idiomatic"
> version files since they're version files not specific to asdf/rtx and can be used by other tools.
> (`.nvmrc` being a notable exception, which is tied to a specific tool.)

### `.tool-versions`

The `.tool-versions` file is asdf's config file and it can be used in rtx just like `.rtx.toml`.
It isn't as flexible so it's recommended to use `.rtx.toml` instead. It can be useful if you
already have a lot of `.tool-versions` files or work on a team that uses asdf.

Here is an example with all the supported syntax:

```
node        20.0.0       # comments are allowed
ruby        3            # can be fuzzy version
shellcheck  latest       # also supports "latest"
jq          1.6
erlang      ref:master   # compile from vcs ref
go          prefix:1.19  # uses the latest 1.19.x version—needed in case "1.19" is an exact match
shfmt       path:./shfmt # use a custom runtime
node        lts          # use lts version of node (not supported by all plugins)

node        sub-2:lts      # install 2 versions behind the latest lts (e.g.: 18 if lts is 20)
python      sub-0.1:latest # install python-3.10 if the latest is 3.11
```

See [the asdf docs](https://asdf-vm.com/manage/configuration.html#tool-versions) for more info on this file format.

### Scopes

Both `.rtx.toml` and `.tool-versions` support "scopes" which modify the behavior of the version:

* `ref:<SHA>` - compile from a vcs (usually git) ref
* `prefix:<PREFIX>` - use the latest version that matches the prefix. Useful for Go since `1.20`
  would only match `1.20` exactly but `prefix:1.20` will match `1.20.1` and `1.20.2` etc.
* `path:<PATH>` - use a custom compiled version at the given path. One use-case is to re-use
  Homebrew tools (e.g.: `path:/opt/homebrew/opt/node@20`).
* `sub-<PARTIAL_VERSION>:<ORIG_VERSION>` - subtracts PARTIAL_VERSION from ORIG_VERSION. This can
  be used to express something like "2 versions behind lts" such as `sub-2:lts`. Or 1 minor
  version behind the latest version: `sub-0.1:latest`.

### Global config: `~/.config/rtx/config.toml`

rtx can be configured in `~/.config/rtx/config.toml`. It's like local `.rtx.toml` files except that
it is used for all directories.

```toml
[tools]
# global tool versions go here
# you can set these with `rtx use -g`
node = 'lts'
python = ['3.10', '3.11']

[settings]
# plugins can read the versions files used by other version managers (if enabled by the plugin)
# for example, .nvmrc in the case of node's nvm
legacy_version_file = true                     # enabled by default (unlike asdf)
legacy_version_file_disable_tools = ['python'] # disable for specific tools

# configure `rtx install` to always keep the downloaded archive
always_keep_download = false        # deleted after install by default
always_keep_install = false         # deleted on failure by default

# configure how frequently (in minutes) to fetch updated plugin repository changes
# this is updated whenever a new runtime is installed
# (note: this isn't currently implemented but there are plans to add it: https://github.com/jdxcode/rtx/issues/128)
plugin_autoupdate_last_check_duration = '1 week' # set to 0 to disable updates

# config files with these prefixes will be trusted by default
trusted_config_paths = [
    '~/work/my-trusted-projects',
]

verbose = false     # set to true to see full installation output, see `RTX_VERBOSE`
asdf_compat = false # set to true to ensure .tool-versions will be compatible with asdf, see `RTX_ASDF_COMPAT`
jobs = 4            # number of plugins or runtimes to install in parallel. The default is `4`.
raw = false         # set to true to directly pipe plugins to stdin/stdout/stderr

shorthands_file = '~/.config/rtx/shorthands.toml' # path to the shorthands file, see `RTX_SHORTHANDS_FILE`
disable_default_shorthands = false # disable the default shorthands, see `RTX_DISABLE_DEFAULT_SHORTHANDS`
disable_tools = ['node']           # disable specific tools, generally used to turn off core tools

experimental = false # enable experimental features
log_level = 'debug' # log verbosity, see `RTX_LOG_LEVEL`

[alias.node]
my_custom_node = '20'  # makes `rtx install node@my_custom_node` install node-20.x
                       # this can also be specified in a plugin (see below in "Aliases")
```

These settings can also be managed with `rtx settings ls|get|set|unset`.

### Environment variables

rtx can also be configured via environment variables. The following options are available:

#### `RTX_DATA_DIR`

This is the directory where rtx stores plugins and tool installs. The default location is `~/.local/share/rtx`.

#### `RTX_CACHE_DIR`

This is the directory where rtx stores internal cache. The default location is `~/.cache/rtx` on
Linux and
`~/Library/Caches/rtx` on macOS.

#### `RTX_CONFIG_FILE`

This is the path to the config file. The default is `~/.config/rtx/config.toml`.
(Or `$XDG_CONFIG_HOME/config.toml` if that is set)

#### `RTX_DEFAULT_TOOL_VERSIONS_FILENAME`

Set to something other than ".tool-versions" to have rtx look for `.tool-versions` files but with
a different name.

#### `RTX_DEFAULT_CONFIG_FILENAME`

Set to something other than `.rtx.toml` to have rtx look for `.rtx.toml` config files with a different name.

#### [experimental] `RTX_ENV`

Enables environment-specific config files such as `.rtx.development.toml`.
Use this for different env vars or different tool versions in
development/staging/production environments. See
[Config Environments](#experimental-config-environments) for more on how
to use this feature.

#### `RTX_${PLUGIN}_VERSION`

Set the version for a runtime. For example, `RTX_NODE_VERSION=20` will use node@20.x regardless
of what is set in `.tool-versions`/`.rtx.toml`.

#### `RTX_LEGACY_VERSION_FILE=1`

Plugins can read the versions files used by other version managers (if enabled by the plugin)
for example, `.nvmrc` in the case of node's nvm. See [legacy version files](#legacy-version-files) for more
information.

Set to "0" to disable legacy version file parsing.

#### `RTX_LEGACY_VERSION_FILE_DISABLE_TOOLS=node,python`

Disable legacy version file parsing for specific tools. Separate with `,`.

#### `RTX_USE_TOML=0`

Set to `1` to default to using `.rtx.toml` in `rtx local` instead of `.tool-versions` for
configuration. This will be default behavior once we hit the [Calver](#versioning) release.

For now this is not used by `rtx use` which will only use `.rtx.toml` unless `--path` is specified.

#### `RTX_TRUSTED_CONFIG_PATHS`

This is a list of paths that rtx will automatically mark as
trusted. They can be separated with `:`.

#### `RTX_LOG_LEVEL=trace|debug|info|warn|error`

These change the verbosity of rtx.

You can also use `RTX_DEBUG=1`, `RTX_TRACE=1`, and `RTX_QUIET=1` as well as
`--log-level=trace|debug|info|warn|error`.

#### `RTX_LOG_FILE=~/rtx.log`

Output logs to a file.

#### `RTX_LOG_FILE_LEVEL=trace|debug|info|warn|error`

Same as `RTX_LOG_LEVEL` but for the log _file_ output level. This is useful if you want
to store the logs but not have them litter your display.

#### `RTX_ALWAYS_KEEP_DOWNLOAD=1`

Set to "1" to always keep the downloaded archive. By default it is deleted after install.

#### `RTX_ALWAYS_KEEP_INSTALL=1`

Set to "1" to always keep the install directory. By default it is deleted on failure.

#### `RTX_VERBOSE=1`

This shows the installation output during `rtx install` and `rtx plugin install`.
This should likely be merged so it behaves the same as `RTX_DEBUG=1` and we don't have
2 configuration for the same thing, but for now it is its own config.

#### `RTX_ASDF_COMPAT=1`

Only output `.tool-versions` files in `rtx local|global` which will be usable by asdf.
This disables rtx functionality that would otherwise make these files incompatible with asdf.

#### `RTX_JOBS=1`

Set the number plugins or runtimes to install in parallel. The default is `4`.

#### `RTX_RAW=1`

Set to "1" to directly pipe plugin scripts to stdin/stdout/stderr. By default stdin is disabled
because when installing a bunch of plugins in parallel you won't see the prompt. Use this if a
plugin accepts input or otherwise does not seem to be installing correctly.

Sets `RTX_JOBS=1` because only 1 plugin script can be executed at a time.

#### `RTX_SHORTHANDS_FILE=~/.config/rtx/shorthands.toml`

Use a custom file for the shorthand aliases. This is useful if you want to share plugins within
an organization.

The file should be in this toml format:

```toml
elixir = "https://github.com/my-org/rtx-elixir.git"
node = "https://github.com/my-org/rtx-node.git"
```

#### `RTX_DISABLE_DEFAULT_SHORTHANDS=1`

Disables the shorthand aliases for installing plugins. You will have to specify full urls when
installing plugins, e.g.: `rtx plugin install node https://github.com/asdf-vm/asdf-node.git`

#### `RTX_DISABLE_TOOLS=python,node`

Disables the specified tools. Separate with `,`. Generally used for core plugins but works with
all.

#### `RTX_CONFIRM=yes|no`

This will automatically answer yes or no to prompts. This is useful for scripting.

#### `RTX_EXPERIMENTAL=1`

Enables experimental features.

## Aliases

rtx supports aliasing the versions of runtimes. One use-case for this is to define aliases for LTS
versions of runtimes. For example, you may want to specify `lts/hydrogen` as the version for node@20.x
so you can use set it with `node lts/hydrogen` in `.tool-versions`/`.rtx.toml`.

User aliases can be created by adding an `alias.<PLUGIN>` section to `~/.config/rtx/config.toml`:

```toml
[alias.node]
my_custom_20 = '20'
```

Plugins can also provide aliases via a `bin/list-aliases` script. Here is an example showing node.js
versions:

```bash
#!/usr/bin/env bash

echo "lts/hydrogen 18"
echo "lts/gallium 16"
echo "lts/fermium 14"
```

> **Note:**
>
> Because this is rtx-specific functionality not currently used by asdf it isn't likely to be in any
> plugin currently, but plugin authors can add this script without impacting asdf users.

## Plugins

rtx uses asdf's plugin ecosystem under the hood. These plugins contain shell scripts like
`bin/install` (for installing) and `bin/list-all` (for listing all of the available versions).

See https://github.com/asdf-vm/asdf-plugins for the list of built-in plugins shorthands. See asdf's
[Create a Plugin](https://asdf-vm.com/plugins/create.html) for how to create your own or just learn
more about how they work.

### Plugin Options

rtx has support for "plugin options" which is configuration specified in `.rtx.toml` to change behavior
of plugins. One example of this is virtualenv on python runtimes:

```toml
[tools]
python = {version='3.11', virtualenv='.venv'}
```

This will be passed to all plugin scripts as `RTX_TOOL_OPTS__VIRTUALENV=.venv`. The user can specify
any option and it will be passed to the plugin in that format.

Currently this only supports simple strings, but we can make it compatible with more complex types
(arrays, tables) fairly easily if there is a need for it.

## Versioning

rtx is currently a new project and is under very rapid development. Slight behavior changes may
occur between releases.
Features marked as "experimental" may change significantly or be removed entirely.

Starting August 6, 2023\*, rtx will move to [Calver](https://calver.org/) versioning (`2023.6.1`). After the move to Calver, rtx's design will become mostly permanent and you will be able to rely on
its behavior for the long term.
Breaking changes will be few but when they do happen,
they will be communicated in the CLI with plenty of notice whenever possible.

Rather than have semver major releases to communicate change in large releases,
new functionality and changes can be opted-into with settings like `experimental = true`.
This way plugin authors and users can
test out new functionality immediately without waiting for a major release.

The numbers in Calver (YYYY.MM.RELEASE) simply represent the date of the release—not compatibility
or how many new features were added.
Each release will be small and incremental.

_\*This plan is tentative and the details may change, but the rough idea of making many changes now so we can have stability later is the goal._

### Calver Breaking Changes

When we switch to Calver, we'll immediately make some notable design changes to rtx. This will
be the first and last time that such a change is made and I actually want to make sure we make
as many as we can—because we'll be stuck with these decisions.

Here are a list of the changes that will be made:

- `rtx local` will default to creating `.rtx.toml` instead of `.tool-versions`. (If the config
  already exists the format will be preserved.)
- `rtx global` will modify `~/.config/rtx/config.toml` instead of `~/.tool-versions`. This path
  can be changed with `RTX_CONFIG_FILE`.
- (more to be added)

## Directories

The following are the directories that rtx uses.
These are the default directories, see
[Configuration](#configuration) for information on changing the locations.

> **Tip**
>
> If you often find yourself using these directories (as I do), I suggest setting all of them to `~/.rtx` for easy access.

### `~/.config/rtx`

This directory stores the global configuration file `~/.config/rtx/config.toml`.

### `~/.cache/rtx`

_On macOS this is `~/Library/Caches/rtx`._

Stores internal cache that rtx uses for things like the list of all available versions of a
plugin.
See [Cache Behavior](#cache-behavior) for more information.

### `~/.local/share/rtx`

This is the main directory that rtx uses and is where plugins and tools are installed into.
It is nearly identical to `~/.asdf` in asdf, so much so that you may be able to get by
symlinking these together and using asdf and rtx simultaneously. (Supporting this isn't a
project goal, however).

#### `~/.local/share/rtx/downloads`

This is where plugins may optionally cache downloaded assets such as tarballs. Use the
`always_keep_downloads` setting to prevent rtx from removing files from here.

#### `~/.local/share/rtx/plugins`

rtx installs plugins to this directory when running `rtx plugins install`. If you are working on a
plugin, I suggest
symlinking it manually by running:

```
ln -s ~/src/rtx-my-tool ~/.local/share/rtx/plugins/my-tool
```

#### `~/.local/share/rtx/installs`

This is where tools are installed to when running `rtx install`. For example, `rtx install
node@20.0.0` will install to `~/.local/share/rtx/installs/node/20.0.0`

This will also create other symlinks to this directory for version prefixes ("20" and "20.15")
and matching aliases ("lts", "latest").
For example:

```
20 -> ./20.15.0
20.15 -> ./20.15.0
latest -> ./20.15.0
lts -> ./20.15.0
```

#### `~/.local/share/rtx/shims`

This is where rtx places shims. Generally these are used for IDE integration or if `rtx activate`
does not work for some reason.

## Templates

> **Warning**
>
> This functionality is experimental and may change in the future.

Templates are used in the following locations:

- `.tool-versions` files
- `.rtx.toml` files for most configuration
- _(Submit a ticket if you want to see it used elsewhere!)_

The following context objects are available inside templates:

- `env: HashMap<String, String>` – current environment variables
- `config_root: PathBuf` – directory containing the `.rtx.toml` file

As well as these functions:

- `exec(command: &str) -> String` – execute a command and return the output

Templates are parsed with [tera](https://tera.netlify.app/docs)—which is quite powerful. For
example, this snippet will get the directory name of the project:

```toml
[env]
PROJECT_NAME = "{{config_root | split(pat='/') | last}}"
```

Here's another using `exec()`:

```toml
[aliases]
current = "{{exec(command='node --version')}}"
```

## [experimental] Config Environments

It's possible to have separate `.rtx.toml` files in the same directory for different
environments like `development` and `production`. To enable, set
`experimental = true` in `~/.config/rtx/config.toml`, then set `RTX_ENV` to an environment like
`development` or `production`. rtx will then look for a `.rtx.{RTX_ENV}.toml` file in the current directory.

rtx will also look for "local" files like `.rtx.local.toml` and `.rtx.{RTX_ENV}.local.toml` in
the current directory. These are intended to not be committed to version control.
(Add `rtx.*.local.toml` to your `.gitignore` file.)

The priority of these files goes in this order (bottom overrides top):

* `.rtx.toml`
* `.rtx.local.toml`
* `.rtx.{RTX_ENV}.toml`
* `.rtx.{RTX_ENV}.local.toml`

Use `rtx doctor` to see which files are being used.

_Note that currently modifying `RTX_DEFAULT_CONFIG_FILENAME` to something other than `.rtx.toml`
will not work with this feature. For now, it will disable it entirely. This may change in the
future._

## IDE Integration

IDEs work better with shims than they do environment variable modifications. The simplest way is
to add the rtx shim directory to PATH.

For IntelliJ and VSCode—and likely others, you can modify `~/.zprofile`
with the following:

```
export PATH="$HOME/.local/share/rtx/shims:$PATH"
```

This won't work for all of rtx's functionality. For example, arbitrary env vars in `[env]` will only be set
if a shim is executed. For this we need tighter integration with the IDE and a custom plugin. If you feel
ambitious, take a look at existing direnv extensions for your IDE and see if you can modify it to work for rtx.
Direnv and rtx work similarly and there should be a direnv extension that can be used as a starting point.

Alternatively, you may be able to get tighter integration with a direnv extension and using the
[`use_rtx`](#direnv) direnv function.

## Core Plugins

rtx comes with some plugins built into the CLI written in Rust. These are new and will improve over
time. They can be easily overridden by installing a plugin with the same name, e.g.: `rtx plugin install python`.

You can see the core plugins with `rtx plugin ls --core`.

* [experimental] [Python](./docs/python.md)
* [NodeJS](./docs/node.md)
* [Ruby (experimental)](./docs/ruby.md)
* [Go (experimental)](./docs/go.md)
* [Java (experimental)](./docs/java.md)

## FAQs

### I don't want to put a `.tool-versions` file into my project since git shows it as an untracked file.

You can make git ignore these files in 3 different ways:

- Adding `.tool-versions` to project's `.gitignore` file. This has the downside that you need to commit the change to the ignore file.
- Adding `.tool-versions` to project's `.git/info/exclude`. This file is local to your project so there is no need to commit it.
- Adding `.tool-versions` to global gitignore (`core.excludesFile`). This will cause git to ignore `.tool-versions` files in all projects. You can explicitly add one to a project if needed with `git add --force .tool-versions`.

### What is the difference between "nodejs" and "node" (or "golang" and "go")?

These are aliased. For example, `rtx use nodejs@14.0` is the same as `rtx install node@14.0`. This
means it is not possible to have these be different plugins.

This is for convenience so you don't need to remember which one is the "official" name. However if
something with the aliasing is acting up, submit a ticket or just stick to using "node" and "go".
Under the hood, when rtx reads a config file or takes CLI input it will swap out "nodejs" and
"golang".

While this change is rolling out, there is some migration code that will move installs/plugins from
the "nodejs" and "golang" directories to the new names. If this runs for you you'll see a message
but it should not run again unless there is some kind of problem. In this case, it's probably
easiest to just run `rm -rf ~/.local/share/rtx/installs/{golang,nodejs} ~/.local/share/rtx/plugins/{golang,nodejs}`.

Once most users have migrated over this migration code will be removed.

### What does `rtx activate` do?

It registers a shell hook to run `rtx hook-env` every time the shell prompt is *displayed*.
You may think that is excessive and it should only run on `cd`, however there are many
situations where it needs to run without the directory changing, for example if the `.rtx.toml`
was modified.

Note my emphasis on the word *displayed*. This means if you attempt to use `rtx activate` in a
non-interactive session (like a bash script), it will never call `rtx hook-env` and in effect will
never modify PATH. For this type of setup, you can either call `rtx hook-env` manually every time
you wish to update PATH, or use [shims](#shims) instead.

Or if you only need to use rtx for certain commands, just prefix the commands with
[`rtx x --`](#rtx-exec-options-toolversion----command).
For example, `rtx x -- npm test` or `rtx x -- ./my_script.sh`.

`rtx hook-env` will exit early in different situations if no changes have been made. This prevents
blocking your shell every time you run a command. You can run `rtx hook-env` yourself to see what it
outputs, however it is likely nothing if you're in a shell that has already been activated.

`rtx activate` also creates a shell function (in most shells) called `rtx`. This is a trick that makes it possible for `rtx shell`
and `rtx deactivate` to work without wrapping them in `eval "$(rtx shell)"`.

### `rtx activate` doesn't work in `~/.profile`, `~/.bash_profile`, `~/.zprofile`

`rtx activate` should only be used in `rc` files. These are the interactive ones used when
a real user is using the terminal. (As opposed to being executed by an IDE or something). The prompt
isn't displayed in non-interactive environments so PATH won't be modified.

For non-interactive setups, consider using shims instead which will route calls to the correct
directory by looking at `PWD` every time they're executed. You can also call `rtx exec` instead of
expecting things to be directly on PATH. You can also run `rtx env` in a non-interactive shell, however that
will only setup the global tools. It won't modify the environment variables when entering into a
different project.

Also see the [shebang](#shebang) example for a way to make scripts call rtx to get the runtime.
That is another way to use rtx without activation.

### rtx is failing or not working right

First try setting `RTX_DEBUG=1` or `RTX_TRACE=1` and see if that gives you more information.
You can also set `RTX_LOG_FILE_LEVEL=debug RTX_LOG_FILE=/path/to/logfile` to write logs to a file.

If something is happening with the activate hook, you can try disabling it and calling `eval "$(rtx hook-env)"` manually.
It can also be helpful to use `rtx env` which will just output environment variables that would be set.
Also consider using [shims](#shims) which can be more compatible.

If runtime installation isn't working right, try using the `--raw` flag which will install things in
series and connect stdin/stdout/stderr directly to the terminal. If a plugin is trying to interact
with you for some reason this will make it work.

Of course check the version of rtx with `rtx --version` and make sure it is the latest. Use `rtx self-update`
to update it. `rtx cache clean` can be used to wipe the internal cache and `rtx implode` can be used
to remove everything except config.

Before submitting a ticket, it's a good idea to test what you were doing with asdf. That way we can rule
out if the issue is with rtx or if it's with a particular plugin. For example, if `rtx install python@latest`
doesn't work, try running `asdf install python latest` to see if it's an issue with asdf-python.

Lastly, there is `rtx doctor` which will show diagnostic information and any warnings about issues
detected with your setup. If you submit a bug report, please include the output of `rtx doctor`.

### Windows support?

This is something we'd like to add! https://github.com/jdxcode/rtx/discussions/66

It's not a near-term goal and it would require plugin modifications, but it should be feasible.

### How do I use rtx with http proxies?

Short answer: just set `http_proxy` and `https_proxy` environment variables. These should be lowercase.

rtx doesn't really do anything with http itself. The only exception to that is checking for new versions
and `rtx self-update`. It uses `git` to clone plugins and the plugins themselves generally will download
files with `curl` or `wget`.

However this is really up to the plugin. If you're having a proxy-related issue installing something
you should post an issue on the plugin's repo.

### How do the shorthand plugin names map to repositories?

e.g.: how does `rtx plugin install node` know to fetch [https://github.com/rtx-plugins/rtx-nodejs]
(https://github.com/rtx-plugins/rtx-nodejs)?

asdf maintains [an index](https://github.com/asdf-vm/asdf-plugins) of shorthands that rtx uses as a base.
This is regularly updated every time that rtx has a release. This repository is stored directly into
the codebase [here](./src/default_shorthands.rs). The bottom of that file contains modifications that
rtx makes on top of asdf.

### Does "node@20" mean the newest available version of node?

It depends on the command. Normally, for most commands and inside of config files, "node@20" will
point to the latest _installed_ version of node-20.x. You can find this version by running
`rtx latest --installed node@20` or by seeing what the `~/.local/share/rtx/installs/node/20` symlink
points to:

```sh-session
$ ls -l ~/.local/share/rtx/installs/node/20
[...] /home/jdxcode/.local/share/rtx/installs/node/20 -> node-v20.0.0-linux-x64
```

There are some exceptions to this, such as the following:

* `rtx install node@20`
* `rtx latest node@20`
* `rtx upgrade node@20`

These will use the latest _available_ version of node-20.x. This generally makes sense because you
wouldn't want to install a version that is already installed.

### How do I migrate from asdf?

First, just install rtx with `rtx activate` like in the getting started guide and remove asdf from your
shell rc file.

Then you can just run `rtx install` in a directory with an asdf `.tool-versions` file and it will
install the runtimes. You could attempt to avoid this by copying the internal directory from asdf over
to rtx with `cp -r ~/.asdf ~/.local/share/rtx`. That _should_ work because they use the same structure,
however this isn't officially supported or regularly tested. Alternatively you can set `RTX_DATA_DIR=~/.asdf`
and see what happens.

### How compatible is rtx with asdf?

rtx should be able to read/install any `.tool-versions` file used by asdf. Any asdf plugin
should be usable in rtx. The commands in rtx are slightly
different, such as `rtx install node@20.0.0` vs `asdf install node 20.0.0`—this is done so
multiple tools can be specified at once. However, asdf-style syntax is still supported: (`rtx
install node 20.0.0`). This is the case for most commands, though the help for the command may
say that asdf-style syntax is supported.

When in doubt, just try asdf syntax and see if it works. If it doesn't open a ticket. It may
not be possible to support every command identically, but
we should attempt to make things as consistent as possible.

This isn't important for usability reasons so much as making it so plugins continue to work that
call asdf commands.

If you need to switch to/from asdf or work in a project with asdf users, you can set
[`RTX_ASDF_COMPAT=1`](#rtx_asdf_compat1). That prevents
rtx from writing `.tool-versions` files that will not be
compatible with asdf. Also consider using `.rtx.toml` instead which won't conflict with asdf setups.

### rtx isn't working when calling from tmux or another shell initialization script

`rtx activate` will not update PATH until the shell prompt is displayed. So if you need to access a
tool provided by rtx before the prompt is displayed you must manually call `hook-env`:

```bash
eval "$(rtx activate bash)"
eval "$(rtx hook-env)"
python --version # will work only after calling hook-env explicitly
```

For more information, see [What does `rtx activate` do?](#what-does-rtx-activate-do)

### How do I disable/force CLI color output?

rtx uses [console.rs](https://docs.rs/console/latest/console/fn.colors_enabled.html) which
honors the [clicolors spec](https://bixense.com/clicolors/):

* `CLICOLOR != 0`: ANSI colors are supported and should be used when the program isn’t piped.
* `CLICOLOR == 0`: Don’t output ANSI color escape codes.
* `CLICOLOR_FORCE != 0`: ANSI colors should be enabled no matter what.

### Is rtx secure?

Not as much as it should be, though currently a bit more secure than asdf. Work will happen in this area as secure
supply chains are incredibly important. See [SECURITY.md](./SECURITY.md) for more information.

## Comparison to asdf

rtx is mostly a clone of asdf, but there are notable areas where improvements have been made.

### Performance

asdf made (what I consider) a poor design decision to use shims that go between a call to a runtime
and the runtime itself. e.g.: when you call `node` it will call an asdf shim file `~/.asdf/shims/node`,
which then calls `asdf exec`, which then calls the correct version of node.

These shims have terrible performance, adding ~120ms to every runtime call. rtx does not use shims and instead
updates `PATH` so that it doesn't have any overhead when simply calling binaries. These shims are the main reason that I wrote this. Note that in the demo gif at the top of this README
that `rtx` isn't actually used when calling `node -v` for this reason. The performance is
identical to running node without using rtx.

I don't think it's possible for asdf to fix these issues. The author of asdf did a great writeup
of [performance problems](https://stratus3d.com/blog/2022/08/11/asdf-performance/). asdf is written
in bash which certainly makes it challenging to be performant, however I think the real problem is the
shim design. I don't think it's possible to fix that without a complete rewrite.

rtx does call an internal command `rtx hook-env` every time the directory has changed, but because
it's written in Rust, this is very quick—taking ~10ms on my machine. 4ms if there are no changes, 14ms if it's
a full reload.

tl;dr: asdf adds overhead (~120ms) when calling a runtime, rtx adds a small amount of overhead (~10ms)
when the prompt loads.

### Environment variables in rtx

asdf only helps manage runtime executables. However, some tools are managed via environment variables
(notably Java which switches via `JAVA_HOME`). This isn't supported very well in asdf and requires
a separate shell extension just to manage.

However asdf _plugins_ have a `bin/exec-env` script that is used for exporting environment variables
like [`JAVA_HOME`](https://github.com/halcyon/asdf-java/blob/master/bin/exec-env). rtx simply exports
the environment variables from the `bin/exec-env` script in the plugin but places them in the shell
for _all_ commands. In asdf it only exports those commands when the shim is called. This means if you
call `java` it will set `JAVA_HOME`, but not if you call some Java tool like `mvn`.

This means we're just using the existing plugin script but because rtx doesn't use shims it can be
used for more things. It would be trivial to make a plugin that exports arbitrary environment
variables like [dotenv](https://github.com/motdotla/dotenv) or [direnv](https://github.com/direnv/direnv).

### UX

Some commands are the same in asdf but others have been changed. Everything that's possible
in asdf should be possible in rtx but may use slightly different syntax. rtx has more forgiving commands,
such as using fuzzy-matching, e.g.: `rtx install node@20`. While in asdf you _can_ run
`asdf install node latest:20`, you can't use `latest:20` in a `.tool-versions` file or many other places.
In `rtx` you can use fuzzy-matching everywhere.

asdf requires several steps to install a new runtime if the plugin isn't installed, e.g.:

```sh-session
asdf plugin add node
asdf install node latest:20
asdf local node latest:20
```

In `rtx` this can all be done in a single step to set the local runtime version. If the plugin
and/or runtime needs to be installed it will prompt:

[![asciicast](https://asciinema.org/a/564031.svg)](https://asciinema.org/a/564031)

I've found asdf to be particularly rigid and difficult to learn. It also made strange decisions like
having `asdf list all` but `asdf latest --all` (why is one a flag and one a positional argument?).
`rtx` makes heavy use of aliases so you don't need to remember if it's `rtx plugin add node` or
`rtx plugin install node`. If I can guess what you meant, then I'll try to get rtx to respond
in the right way.

That said, there are a lot of great things about asdf. It's the best multi-runtime manager out there
and I've really been impressed with the plugin system. Most of the design decisions the authors made
were very good. I really just have 2 complaints: the shims and the fact it's written in Bash.

### CI/CD

Using rtx in CI/CD is a great way to synchronize tool versions for dev/build.

### GitHub Actions

Use [`jdxcode/rtx-action`](https://github.com/jdxcode/rtx-action):

```yaml
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: jdxcode/rtx-action@v1
      - run: node -v # will be the node version from `.rtx.toml`/`.tool-versions`
```

## Shims

While the PATH design of rtx works great in most cases, there are some situations where shims are
preferable. One example is when calling rtx binaries from an IDE.

To support this, rtx does have a shim dir that can be used. It's located at `~/.local/share/rtx/shims`.

```sh-session
$ rtx i node@20.0.0
$ rtx reshim # may be required if new shims need to be created
$ ~/.local/share/rtx/shims/node -v
v20.0.0
```

## direnv

[direnv](https://direnv.net) and rtx both manage environment variables based on directory. Because they both analyze
the current environment variables before and after their respective "hook" commands are run, they can sometimes conflict with each other.

If you have an issue, it's likely to do with the ordering of PATH. This means it would
really only be a problem if you were trying to manage the same tool with direnv and rtx. For example,
you may use `layout python` in an `.envrc` but also be maintaining a `.tool-versions` file with python
in it as well.

A more typical usage of direnv would be to set some arbitrary environment variables, or add unrelated
binaries to PATH. In these cases, rtx will not interfere with direnv.

### rtx inside of direnv (`use rtx` in `.envrc`)

If you do encounter issues with `rtx activate`, or just want to use direnv in an alternate way,
this is a simpler setup that's less likely to cause issues—at the cost of functionality.

This may be required if you want to use direnv's `layout python` with rtx. Otherwise there are
situations where rtx will override direnv's PATH. `use rtx` ensures that direnv always has control.

To do this, first use `rtx` to build a `use_rtx` function that you can use in `.envrc` files:

```
rtx direnv activate > ~/.config/direnv/lib/use_rtx.sh
```

Now in your `.envrc` file add the following:

```sh-session
use rtx
```

direnv will now call rtx to export its environment variables. You'll need to make sure to add `use_rtx`
to all projects that use rtx (or use direnv's `source_up` to load it from a subdirectory). You can also add `use rtx` to `~/.config/direnv/direnvrc`.

Note that in this method direnv typically won't know to refresh `.tool-versions` files
unless they're at the same level as a `.envrc` file. You'll likely always want to have
a `.envrc` file next to your `.tool-versions` for this reason. To make this a little
easier to manage, I encourage _not_ actually using `.tool-versions` at all, and instead
setting environment variables entirely in `.envrc`:

```
export RTX_NODE_VERSION=20.0.0
export RTX_PYTHON_VERSION=3.11
```

Of course if you use `rtx activate`, then these steps won't have been necessary and you can use rtx
as if direnv was not used.

If you continue to struggle, you can also try using the [shims method](#shims).

### Do you need direnv?

While making rtx compatible with direnv is, and will always be a major goal of this project, I also
want rtx to be capable of replacing direnv if needed. This is why rtx includes support for managing
env vars and [virtualenv](https://github.com/jdxcode/rtx-python#experimental-virtualenv-support)
for python using `.rtx.toml`.

If you find you continue to need direnv, please open an issue and let me know what it is to see if
it's something rtx could support. rtx will never be as capable as direnv with a DSL like `.envrc`,
but I think we can handle enough common use cases to make that unnecessary for most people.

## Cache Behavior

rtx makes use of caching in many places in order to be efficient. The details about how long to keep
cache for should eventually all be configurable. There may be gaps in the current behavior where
things are hardcoded, but I'm happy to add more settings to cover whatever config is needed.

Below I explain the behavior it uses around caching. If you're seeing behavior where things don't appear
to be updating, this is a good place to start.

### Plugin/Runtime Cache

Each plugin has a cache that's stored in `~/$RTX_CACHE_DIR/<PLUGIN>`. It stores
the list of versions available for that plugin (`rtx ls-remote <PLUGIN>`), the legacy filenames (see below),
the list of aliases, the bin directories within each runtime installation, and the result of
running `exec-env` after the runtime was installed.

Remote versions are updated daily by default. The file is zlib messagepack, if you want to view it you can
run the following (requires [msgpack-cli](https://github.com/msgpack/msgpack-cli)).

```sh-session
cat ~/$RTX_CACHE_DIR/node/remote_versions.msgpack.z | perl -e 'use Compress::Raw::Zlib;my $d=new Compress::Raw::Zlib::Inflate();my $o;undef $/;$d->inflate(<>,$o);print $o;' | msgpack-cli decode
```

Note that the caching of `exec-env` may be problematic if the script isn't simply exporting
static values. The vast majority of `exec-env` scripts only export static values, but if you're
working with a plugin that has a dynamic `exec-env` submit
a ticket and we can try to figure out what to do.

Caching `exec-env` massively improved the performance of rtx since it requires calling bash
every time rtx is initialized. Ideally, we can keep this
behavior.

<!-- RTX:COMMANDS -->
## Commands

### `rtx activate [OPTIONS] [SHELL_TYPE]`

```
Initializes rtx in the current shell

This should go into your shell's rc file.
Otherwise, it will only take effect in the current session.
(e.g. ~/.zshrc, ~/.bashrc)

This is only intended to be used in interactive sessions, not scripts.
rtx is only capable of updating PATH when the prompt is displayed to the user.
For non-interactive use-cases, use shims instead.

Typically this can be added with something like the following:

    echo 'eval "$(rtx activate)"' >> ~/.zshrc

However, this requires that "rtx" is in your PATH. If it is not, you need to
specify the full path like this:

    echo 'eval "$(/path/to/rtx activate)"' >> ~/.zshrc

Usage: activate [OPTIONS] [SHELL_TYPE]

Arguments:
  [SHELL_TYPE]
          Shell type to generate the script for

          [possible values: bash, fish, nu, xonsh, zsh]

Options:
      --status
          Show "rtx: <PLUGIN>@<VERSION>" message when changing directories

Examples:
  $ eval "$(rtx activate bash)"
  $ eval "$(rtx activate zsh)"
  $ rtx activate fish | source
  $ execx($(rtx activate xonsh))
```
### `rtx alias get <PLUGIN> <ALIAS>`

```
Show an alias for a plugin

This is the contents of an alias.<PLUGIN> entry in ~/.config/rtx/config.toml

Usage: alias get <PLUGIN> <ALIAS>

Arguments:
  <PLUGIN>
          The plugin to show the alias for

  <ALIAS>
          The alias to show

Examples:
 $ rtx alias get node lts/hydrogen
 20.0.0
```
### `rtx alias ls [OPTIONS]`

```
List aliases
Shows the aliases that can be specified.
These can come from user config or from plugins in `bin/list-aliases`.

For user config, aliases are defined like the following in `~/.config/rtx/config.toml`:

  [alias.node]
  lts = "20.0.0"

Usage: alias ls [OPTIONS]

Options:
  -p, --plugin <PLUGIN>
          Show aliases for <PLUGIN>

Examples:
  $ rtx aliases
  node    lts/hydrogen   20.0.0
```
### `rtx alias set <PLUGIN> <ALIAS> <VALUE>`

```
Add/update an alias for a plugin

This modifies the contents of ~/.config/rtx/config.toml

Usage: alias set <PLUGIN> <ALIAS> <VALUE>

Arguments:
  <PLUGIN>
          The plugin to set the alias for

  <ALIAS>
          The alias to set

  <VALUE>
          The value to set the alias to

Examples:
  $ rtx alias set node lts/hydrogen 18.0.0
```
### `rtx alias unset <PLUGIN> <ALIAS>`

```
Clears an alias for a plugin

This modifies the contents of ~/.config/rtx/config.toml

Usage: alias unset <PLUGIN> <ALIAS>

Arguments:
  <PLUGIN>
          The plugin to remove the alias from

  <ALIAS>
          The alias to remove

Examples:
  $ rtx alias unset node lts/hydrogen
```
### `rtx bin-paths`

```
List all the active runtime bin paths

Usage: bin-paths
```
### `rtx cache clear`

```
Deletes all cache files in rtx

Usage: cache clear
```
### `rtx completion [SHELL]`

```
Generate shell completions

Usage: completion [SHELL]

Arguments:
  [SHELL]
          Shell type to generate completions for

          [possible values: bash, elvish, fish, powershell, zsh]

Examples:
  $ rtx completion bash > /etc/bash_completion.d/rtx
  $ rtx completion zsh  > /usr/local/share/zsh/site-functions/_rtx
  $ rtx completion fish > ~/.config/fish/completions/rtx.fish
```
### `rtx current [PLUGIN]`

```
Shows current active and installed runtime versions

This is similar to `rtx ls --current`, but this only shows the runtime
and/or version. It's designed to fit into scripts more easily.

Usage: current [PLUGIN]

Arguments:
  [PLUGIN]
          Plugin to show versions of e.g.: ruby, node

Examples:
  # outputs `.tool-versions` compatible format
  $ rtx current
  python 3.11.0 3.10.0
  shfmt 3.6.0
  shellcheck 0.9.0
  node 20.0.0

  $ rtx current node
  20.0.0

  # can output multiple versions
  $ rtx current python
  3.11.0 3.10.0
```
### `rtx deactivate`

```
Disable rtx for current shell session

This can be used to temporarily disable rtx in a shell session.

Usage: deactivate

Examples:
  $ rtx deactivate bash
  $ rtx deactivate zsh
  $ rtx deactivate fish
  $ execx($(rtx deactivate xonsh))
```
### `rtx direnv activate`

```
Output direnv function to use rtx inside direnv

See https://github.com/jdxcode/rtx#direnv for more information

Because this generates the legacy files based on currently installed plugins,
you should run this command after installing new plugins. Otherwise
direnv may not know to update environment variables when legacy file versions change.

Usage: direnv activate

Examples:
  $ rtx direnv activate > ~/.config/direnv/lib/use_rtx.sh
  $ echo 'use rtx' > .envrc
  $ direnv allow
```
### `rtx doctor`

```
Check rtx installation for possible problems.

Usage: doctor

Examples:
  $ rtx doctor
  [WARN] plugin node is not installed
```
### `rtx env [OPTIONS] [TOOL@VERSION]...`

```
Exports env vars to activate rtx a single time

Use this if you don't want to permanently install rtx. It's not necessary to
use this if you have `rtx activate` in your shell rc file.

Usage: env [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to use

Options:
  -s, --shell <SHELL>
          Shell type to generate environment variables for

          [possible values: bash, fish, nu, xonsh, zsh]

      --json
          Output in JSON format

          [short aliases: J]

Examples:
  $ eval "$(rtx env -s bash)"
  $ eval "$(rtx env -s zsh)"
  $ rtx env -s fish | source
  $ execx($(rtx env -s xonsh))
```
### `rtx env-vars [OPTIONS] [ENV_VARS]...`

```
Manage environment variables

By default this command modifies ".rtx.toml" in the current directory.
You can specify the file name by either setting the RTX_DEFAULT_CONFIG_FILENAME environment variable, or by using the --file option.

Usage: env-vars [OPTIONS] [ENV_VARS]...

Arguments:
  [ENV_VARS]...
          Environment variable(s) to set
          e.g.: NODE_ENV=production

Options:
      --file <FILE>
          The TOML file to update

          Defaults to RTX_DEFAULT_CONFIG_FILENAME environment variable, or ".rtx.toml".

      --remove <ENV_VAR>
          Remove the environment variable from config file

          Can be used multiple times.
```
### `rtx exec [OPTIONS] [TOOL@VERSION]... [-- <COMMAND>...]`

```
Execute a command with tool(s) set

use this to avoid modifying the shell session or running ad-hoc commands with rtx tools set.

Tools will be loaded from .rtx.toml/.tool-versions, though they can be overridden with <RUNTIME> args
Note that only the plugin specified will be overridden, so if a `.tool-versions` file
includes "node 20" but you run `rtx exec python@3.11`; it will still load node@20.

The "--" separates runtimes from the commands to pass along to the subprocess.

Usage: exec [OPTIONS] [TOOL@VERSION]... [-- <COMMAND>...]

Arguments:
  [TOOL@VERSION]...
          Tool(s) to start e.g.: node@20 python@3.10

  [COMMAND]...
          Command string to execute (same as --command)

Options:
  -c, --command <C>
          Command string to execute

      --cd <CD>
          Change to this directory before executing the command

          [short aliases: C]

Examples:
  $ rtx exec node@20 -- node ./app.js  # launch app.js using node-20.x
  $ rtx x node@20 -- node ./app.js     # shorter alias

  # Specify command as a string:
  $ rtx exec node@20 python@3.11 --command "node -v && python -V"

  # Run a command in a different directory:
  $ rtx x -C /path/to/project node@20 -- node ./app.js
```
### `rtx implode [OPTIONS]`

```
Removes rtx CLI and all related data

Skips config directory by default.

Usage: implode [OPTIONS]

Options:
      --config
          Also remove config directory

      --dry-run
          List directories that would be removed without actually removing them
```
### `rtx install [OPTIONS] [TOOL@VERSION]...`

```
Install a tool version

This will install a tool version to `~/.local/share/rtx/installs/<PLUGIN>/<VERSION>`
It won't be used simply by being installed, however.
For that, you must set up a `.rtx.toml`/`.tool-version` file manually or with `rtx use`.
Or you can call a tool version explicitly with `rtx exec <TOOL>@<VERSION> -- <COMMAND>`.

Tools will be installed in parallel. To disable, set `--jobs=1` or `RTX_JOBS=1`

Usage: install [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to install e.g.: node@20

Options:
  -f, --force
          Force reinstall even if already installed

  -v, --verbose...
          Show installation output

Examples:
  $ rtx install node@20.0.0  # install specific node version
  $ rtx install node@20      # install fuzzy node version
  $ rtx install node         # install version specified in .tool-versions or .rtx.toml
  $ rtx install                # installs everything specified in .tool-versions or .rtx.toml
```
### `rtx latest [OPTIONS] <TOOL@VERSION>`

```
Gets the latest available version for a plugin

Usage: latest [OPTIONS] <TOOL@VERSION>

Arguments:
  <TOOL@VERSION>
          Tool to get the latest version of

Options:
  -i, --installed
          Show latest installed instead of available version

Examples:
  $ rtx latest node@20  # get the latest version of node 20
  20.0.0

  $ rtx latest node     # get the latest stable version of node
  20.0.0
```
### `rtx link [OPTIONS] <TOOL@VERSION> <PATH>`

```
Symlinks a tool version into rtx

Use this for adding installs either custom compiled outside
rtx or built with a different tool.

Usage: link [OPTIONS] <TOOL@VERSION> <PATH>

Arguments:
  <TOOL@VERSION>
          Tool name and version to create a symlink for

  <PATH>
          The local path to the tool version
          e.g.: ~/.nvm/versions/node/v20.0.0

Options:
  -f, --force
          Overwrite an existing tool version if it exists

Examples:
  # build node-20.0.0 with node-build and link it into rtx
  $ node-build 20.0.0 ~/.nodes/20.0.0
  $ rtx link node@20.0.0 ~/.nodes/20.0.0

  # have rtx use the python version provided by Homebrew
  $ brew install node
  $ rtx link node@brew $(brew --prefix node)
  $ rtx use node@brew
```
### `rtx ls [OPTIONS]`

```
List installed and/or currently selected tool versions

Usage: ls [OPTIONS]

Options:
  -p, --plugin <PLUGIN>
          Only show tool versions from [PLUGIN]

  -c, --current
          Only show tool versions currently specified in a .tool-versions/.rtx.toml

  -i, --installed
          Only show tool versions that are installed Hides missing ones defined in .tool-versions/.rtx.toml but not yet installed

      --json
          Output in json format

          [short aliases: J]

  -m, --missing
          Display missing tool versions

      --prefix <PREFIX>
          Display versions matching this prefix

Examples:
  $ rtx ls
  node    20.0.0 ~/src/myapp/.tool-versions latest
  python  3.11.0 ~/.tool-versions           3.10
  python  3.10.0

  $ rtx ls --current
  node    20.0.0 ~/src/myapp/.tool-versions 20
  python  3.11.0 ~/.tool-versions           3.11.0

  $ rtx ls --json
  {
    "node": [
      {
        "version": "20.0.0",
        "install_path": "/Users/jdx/.rtx/installs/node/20.0.0",
        "source": {
          "type": ".rtx.toml",
          "path": "/Users/jdx/.rtx.toml"
        }
      }
    ],
    "python": [...]
  }
```
### `rtx ls-remote <TOOL@VERSION> [PREFIX]`

```
List runtime versions available for install

note that the results are cached for 24 hours
run `rtx cache clean` to clear the cache and get fresh results

Usage: ls-remote <TOOL@VERSION> [PREFIX]

Arguments:
  <TOOL@VERSION>
          Plugin to get versions for

  [PREFIX]
          The version prefix to use when querying the latest version
          same as the first argument after the "@"

Examples:
  $ rtx ls-remote node
  18.0.0
  20.0.0

  $ rtx ls-remote node@20
  20.0.0
  20.1.0

  $ rtx ls-remote node 20
  20.0.0
  20.1.0
```
### `rtx outdated [TOOL@VERSION]...`

```
[experimental] Shows outdated tool versions

Usage: outdated [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to show outdated versions for
          e.g.: node@20 python@3.10
          If not specified, all tools in global and local configs will be shown

Examples:
  $ rtx outdated
  Plugin  Requested  Current  Latest
  python  3.11       3.11.0   3.11.1
  node    20         20.0.0   20.1.0

  $ rtx outdated node
  Plugin  Requested  Current  Latest
  node    20         20.0.0   20.1.0
```
### `rtx plugins install [OPTIONS] [NAME] [GIT_URL]`

```
Install a plugin

note that rtx automatically can install plugins when you install a tool
e.g.: `rtx install node@20` will autoinstall the node plugin

This behavior can be modified in ~/.config/rtx/config.toml

Usage: plugins install [OPTIONS] [NAME] [GIT_URL]

Arguments:
  [NAME]
          The name of the plugin to install
          e.g.: node, ruby
          Can specify multiple plugins: `rtx plugins install node ruby python`

  [GIT_URL]
          The git url of the plugin

Options:
  -f, --force
          Reinstall even if plugin exists

  -a, --all
          Install all missing plugins
          This will only install plugins that have matching shorthands.
          i.e.: they don't need the full git repo url

  -v, --verbose...
          Show installation output

Examples:
  # install the node via shorthand
  $ rtx plugins install node

  # install the node plugin using a specific git url
  $ rtx plugins install node https://github.com/rtx-plugins/rtx-nodejs.git

  # install the node plugin using the git url only
  # (node is inferred from the url)
  $ rtx plugins install https://github.com/rtx-plugins/rtx-nodejs.git

  # install the node plugin using a specific ref
  $ rtx plugins install node https://github.com/rtx-plugins/rtx-nodejs.git#v1.0.0
```
### `rtx plugins link [OPTIONS] <NAME> [PATH]`

```
Symlinks a plugin into rtx

This is used for developing a plugin.

Usage: plugins link [OPTIONS] <NAME> [PATH]

Arguments:
  <NAME>
          The name of the plugin
          e.g.: node, ruby

  [PATH]
          The local path to the plugin
          e.g.: ./rtx-node

Options:
  -f, --force
          Overwrite existing plugin

Examples:
  # essentially just `ln -s ./rtx-node ~/.local/share/rtx/plugins/node`
  $ rtx plugins link node ./rtx-node

  # infer plugin name as "node"
  $ rtx plugins link ./rtx-node
```
### `rtx plugins ls [OPTIONS]`

```
List installed plugins

Can also show remotely available plugins to install.

Usage: plugins ls [OPTIONS]

Options:
  -c, --core
          The built-in plugins only
          Normally these are not shown

  -u, --urls
          Show the git url for each plugin
          e.g.: https://github.com/asdf-vm/asdf-node.git

      --refs
          Show the git refs for each plugin
          e.g.: main 1234abc

Examples:
  $ rtx plugins ls
  node
  ruby

  $ rtx plugins ls --urls
  node                        https://github.com/asdf-vm/asdf-node.git
  ruby                          https://github.com/asdf-vm/asdf-ruby.git
```
### `rtx plugins ls-remote [OPTIONS]`

```
List all available remote plugins

The full list is here: https://github.com/jdxcode/rtx/blob/main/src/default_shorthands.rs

Examples:
  $ rtx plugins ls-remote


Usage: plugins ls-remote [OPTIONS]

Options:
  -u, --urls
          Show the git url for each plugin e.g.: https://github.com/rtx-plugins/rtx-nodejs.git

      --only-names
          Only show the name of each plugin by default it will show a "*" next to installed plugins
```
### `rtx plugins uninstall <PLUGIN>...`

```
Removes a plugin

Usage: plugins uninstall <PLUGIN>...

Arguments:
  <PLUGIN>...
          Plugin(s) to remove

Examples:
  $ rtx uninstall node
```
### `rtx plugins update [PLUGIN]...`

```
Updates a plugin to the latest version

note: this updates the plugin itself, not the runtime versions

Usage: plugins update [PLUGIN]...

Arguments:
  [PLUGIN]...
          Plugin(s) to update

Examples:
  $ rtx plugins update              # update all plugins
  $ rtx plugins update node       # update only node
  $ rtx plugins update node@beta  # specify a ref
```
### `rtx prune [OPTIONS] [PLUGINS]...`

```
Delete unused versions of tools

rtx tracks which config files have been used in ~/.local/share/rtx/tracked_config_files
Versions which are no longer the latest specified in any of those configs are deleted.
Versions installed only with environment variables (`RTX_<PLUGIN>_VERSION`) will be deleted,
as will versions only referenced on the command line (`rtx exec <PLUGIN>@<VERSION>`).

Usage: prune [OPTIONS] [PLUGINS]...

Arguments:
  [PLUGINS]...
          Prune only versions from these plugins

Options:
      --dry-run
          Do not actually delete anything

Examples:
  $ rtx prune --dry-run
  rm -rf ~/.local/share/rtx/versions/node/20.0.0
  rm -rf ~/.local/share/rtx/versions/node/20.0.1
```
### `rtx reshim`

```
rebuilds the shim farm

This creates new shims in ~/.local/share/rtx/shims for CLIs that have been added.
rtx will try to do this automatically for commands like `npm i -g` but there are
other ways to install things (like using yarn or pnpm for node) that rtx does
not know about and so it will be necessary to call this explicitly.

If you think rtx should automatically call this for a particular command, please
open an issue on the rtx repo. You can also setup a shell function to reshim
automatically (it's really fast so you don't need to worry about overhead):

npm() {
  command npm "$@"
  rtx reshim
}

Usage: reshim

Examples:
  $ rtx reshim
  $ ~/.local/share/rtx/shims/node -v
  v20.0.0
```
### `rtx self-update`

```
Updates rtx itself

Uses whatever package manager was used to install rtx or just downloads
a binary from GitHub Releases if rtx was installed manually.
Supports: standalone, brew, deb, rpm

Usage: self-update
```
### `rtx settings get <KEY>`

```
Show a current setting

This is the contents of a single entry in ~/.config/rtx/config.toml

Note that aliases are also stored in this file
but managed separately with `rtx aliases get`

Usage: settings get <KEY>

Arguments:
  <KEY>
          The setting to show

Examples:
  $ rtx settings get legacy_version_file
  true
```
### `rtx settings ls`

```
Show current settings

This is the contents of ~/.config/rtx/config.toml

Note that aliases are also stored in this file
but managed separately with `rtx aliases`

Usage: settings ls

Examples:
  $ rtx settings
  legacy_version_file = false
```
### `rtx settings set <KEY> <VALUE>`

```
Add/update a setting

This modifies the contents of ~/.config/rtx/config.toml

Usage: settings set <KEY> <VALUE>

Arguments:
  <KEY>
          The setting to set

  <VALUE>
          The value to set

Examples:
  $ rtx settings set legacy_version_file true
```
### `rtx settings unset <KEY>`

```
Clears a setting

This modifies the contents of ~/.config/rtx/config.toml

Usage: settings unset <KEY>

Arguments:
  <KEY>
          The setting to remove

Examples:
  $ rtx settings unset legacy_version_file
```
### `rtx shell [OPTIONS] [TOOL@VERSION]...`

```
Sets a tool version for the current shell session

Only works in a session where rtx is already activated.

Usage: shell [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to use

Options:
  -u, --unset
          Removes a previously set version

Examples:
  $ rtx shell node@20
  $ node -v
  v20.0.0
```
### `rtx sync node <--brew|--nvm|--nodenv>`

```
Symlinks all tool versions from an external tool into rtx

For example, use this to import all Homebrew node installs into rtx

Usage: sync node <--brew|--nvm|--nodenv>

Options:
      --brew
          Get tool versions from Homebrew

      --nvm
          Get tool versions from nvm

      --nodenv
          Get tool versions from nodenv

Examples:
  $ brew install node@18 node@20
  $ rtx sync node --brew
  $ rtx use -g node@18 - uses Homebrew-provided node
```
### `rtx sync python --pyenv`

```
Symlinks all tool versions from an external tool into rtx

For example, use this to import all pyenv installs into rtx

Usage: sync python --pyenv

Options:
      --pyenv
          Get tool versions from pyenv

Examples:
  $ pyenv install 3.11.0
  $ rtx sync python --pyenv
  $ rtx use -g python@3.11.0 - uses pyenv-provided python
```
### `rtx trust [OPTIONS] [CONFIG_FILE]`

```
Marks a config file as trusted

This means rtx will parse the file with potentially dangerous
features enabled.

This includes:
- environment variables
- templates
- `path:` plugin versions

Usage: trust [OPTIONS] [CONFIG_FILE]

Arguments:
  [CONFIG_FILE]
          The config file to trust

Options:
      --untrust
          No longer trust this config

Examples:
  # trusts ~/some_dir/.rtx.toml
  $ rtx trust ~/some_dir/.rtx.toml

  # trusts .rtx.toml in the current or parent directory
  $ rtx trust
```
### `rtx uninstall <TOOL@VERSION>...`

```
Removes runtime versions

Usage: uninstall <TOOL@VERSION>...

Arguments:
  <TOOL@VERSION>...
          Tool(s) to remove

Examples:
  $ rtx uninstall node@18.0.0 # will uninstall specific version
  $ rtx uninstall node        # will uninstall current node version
```
### `rtx upgrade [TOOL@VERSION]...`

```
[experimental] Upgrades outdated tool versions

Usage: upgrade [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to upgrade
          e.g.: node@20 python@3.10
          If not specified, all current tools will be upgraded
```
### `rtx use [OPTIONS] [TOOL@VERSION]...`

```
Change the active version of a tool locally or globally.

This will install the tool if it is not already installed.
By default, this will use an `.rtx.toml` file in the current directory.
Use the --global flag to use the global config file instead.
This replaces asdf's `local` and `global` commands, however those are still available in rtx.

Usage: use [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to add to config file
          e.g.: node@20
          If no version is specified, it will default to @latest

Options:
      --pin
          Save exact version to config file
          e.g.: `rtx use --pin node@20` will save `node 20.0.0` to ~/.tool-versions

      --fuzzy
          Save fuzzy version to config file
          e.g.: `rtx use --fuzzy node@20` will save `node 20` to ~/.tool-versions
          this is the default behavior unless RTX_ASDF_COMPAT=1

      --remove <TOOL>
          Remove the tool(s) from config file

  -g, --global
          Use the global config file (~/.config/rtx/config.toml) instead of the local one

  -p, --path <PATH>
          Specify a path to a config file or directory

Examples:
  # set the current version of node to 20.x in .rtx.toml of current directory
  # will write the fuzzy version (e.g.: 20)
  $ rtx use node@20

  # set the current version of node to 20.x in ~/.config/rtx/config.toml
  # will write the precise version (e.g.: 20.0.0)
  $ rtx use -g --pin node@20
```
### `rtx version`

```
Show rtx version

Usage: version
```
### `rtx where <TOOL@VERSION>`

```
Display the installation path for a runtime

Must be installed.

Usage: where <TOOL@VERSION>

Arguments:
  <TOOL@VERSION>
          Tool(s) to look up
          e.g.: ruby@3
          if "@<PREFIX>" is specified, it will show the latest installed version
          that matches the prefix
          otherwise, it will show the current, active installed version

Examples:
  # Show the latest installed version of node
  # If it is is not installed, errors
  $ rtx where node@20
  /home/jdx/.local/share/rtx/installs/node/20.0.0

  # Show the current, active install directory of node
  # Errors if node is not referenced in any .tool-version file
  $ rtx where node
  /home/jdx/.local/share/rtx/installs/node/20.0.0
```
### `rtx which [OPTIONS] <BIN_NAME>`

```
Shows the path that a bin name points to

Usage: which [OPTIONS] <BIN_NAME>

Arguments:
  <BIN_NAME>
          The bin name to look up

Options:
      --plugin
          Show the plugin name instead of the path

      --version
          Show the version instead of the path

  -t, --tool <TOOL@VERSION>
          Use a specific tool@version
          e.g.: `rtx which npm --tool=node@20`

Examples:
  $ rtx which node
  /home/username/.local/share/rtx/installs/node/20.0.0/bin/node
  $ rtx which node --plugin
  node
  $ rtx which node --version
  20.0.0
```
<!-- RTX:COMMANDS -->
