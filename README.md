<div align="center">
<a href="https://rtx.jdx.dev"><picture>
  <source media="(prefers-color-scheme: dark)" width="617" srcset="./docs/logo-dark@2x.png">
  <img alt="rtx logo" width="617" src="./docs/logo-light@2x.png">
</picture></a>
<br/>
<a href="https://crates.io/crates/rtx-cli"><img alt="Crates.io" src="https://img.shields.io/crates/v/rtx-cli?style=for-the-badge"></a>
<a href="https://github.com/jdx/rtx/blob/main/LICENSE"><img alt="GitHub" src="https://img.shields.io/github/license/jdx/rtx?color=%2320A920&style=for-the-badge"></a>
<a href="https://github.com/jdx/rtx/actions/workflows/test.yml"><img alt="GitHub Workflow Status" src="https://img.shields.io/github/actions/workflow/status/jdx/rtx/test.yml?color=%2320A920&style=for-the-badge"></a>
<!-- <a href="https://codecov.io/gh/jdx/rtx"><img alt="Codecov" src="https://img.shields.io/codecov/c/github/jdx/rtx?color=%2320A920&style=for-the-badge"></a> -->
<a href="https://discord.gg/mABnUDvP57"><img alt="Discord" src="https://img.shields.io/discord/1066429325269794907?color=%23738ADB&style=for-the-badge"></a>
<p><em>The front-end to your dev env.</em></p>
</div>

## What is it?

* Like [asdf](https://asdf-vm.com) (or [nvm](https://github.com/nvm-sh/nvm) or [pyenv](https://github.com/pyenv/pyenv) but for any language) it manages dev tools like node, python, cmake, terraform, and [hundreds more](https://rtx.jdx.dev/plugins.html).
* Like [direnv](https://github.com/direnv/direnv) it manages [environment variables](https://rtx.jdx.dev/environments.html) for different project directories.
* Like [make](https://www.gnu.org/software/make/manual/make.html) it manages [tasks](https://rtx.jdx.dev/tasks.html) used to build and test projects.

## 30 Second Demo

The following shows using rtx to install different versions
of [node](https://nodejs.org).
Note that calling `which node` gives us a real path to node, not a shim.

[![demo](./docs/demo.gif)](./docs/demo.gif)

## Quickstart

Install rtx on macOS (other methods [here](https://rtx.jdx.dev/getting-started.html)):

```sh-session
$ curl https://rtx.jdx.dev/install.sh | sh
$ ~/.local/share/rtx/bin/rtx --version
rtx 2023.12.40
```

Hook rtx into your shell (pick the right one for your shell):

```sh-session
# note this assumes rtx is located at ~/.local/share/rtx/bin/rtx
# which is what install.sh does by default
echo 'eval "$(~/.local/share/rtx/bin/rtx activate bash)"' >> ~/.bashrc
echo 'eval "$(~/.local/share/rtx/bin/rtx activate zsh)"' >> ~/.zshrc
echo '~/.local/share/rtx/bin/rtx activate fish | source' >> ~/.config/fish/config.fish
```

Install a runtime and set it as the global default:

```sh-session
$ rtx use --global node@20
$ node -v
v20.0.0
```

## Full Documentation

See [rtx.jdx.dev](https://rtx.jdx.dev).
