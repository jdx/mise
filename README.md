<div align="center">
<!-- <a href="https://mise.jdx.dev"><picture> -->
<!--   <source media="(prefers-color-scheme: dark)" width="617" srcset="./docs/logo-dark@2x.png"> -->
<!--   <img alt="mise logo" width="617" src="./docs/logo-light@2x.png"> -->
<!-- </picture></a> -->
<br/>
<a href="https://crates.io/crates/mise-cli"><img alt="Crates.io" src="https://img.shields.io/crates/v/mise-cli?style=for-the-badge"></a>
<a href="https://github.com/jdx/mise/blob/main/LICENSE"><img alt="GitHub" src="https://img.shields.io/github/license/jdx/mise?color=%2320A920&style=for-the-badge"></a>
<a href="https://github.com/jdx/mise/actions/workflows/test.yml"><img alt="GitHub Workflow Status" src="https://img.shields.io/github/actions/workflow/status/jdx/mise/test.yml?color=%2320A920&style=for-the-badge"></a>
<!-- <a href="https://codecov.io/gh/jdx/mise"><img alt="Codecov" src="https://img.shields.io/codecov/c/github/jdx/mise?color=%2320A920&style=for-the-badge"></a> -->
<a href="https://discord.gg/mABnUDvP57"><img alt="Discord" src="https://img.shields.io/discord/1066429325269794907?color=%23738ADB&style=for-the-badge"></a>
<p><em>The front-end to your dev env. (formerly called "rtx")</em></p>
</div>

## What is it?

* Like [asdf](https://asdf-vm.com) (or [nvm](https://github.com/nvm-sh/nvm) or [pyenv](https://github.com/pyenv/pyenv) but for any language) it manages dev tools like node, python, cmake, terraform, and [hundreds more](https://mise.jdx.dev/plugins.html).
* Like [direnv](https://github.com/direnv/direnv) it manages [environment variables](https://mise.jdx.dev/environments.html) for different project directories.
* Like [make](https://www.gnu.org/software/make/manual/make.html) it manages [tasks](https://mise.jdx.dev/tasks/) used to build and test projects.

## 30 Second Demo

The following shows using mise to install different versions
of [node](https://nodejs.org).
Note that calling `which node` gives us a real path to node, not a shim.

[![demo](./docs/demo.gif)](./docs/demo.gif)

## Quickstart

Install mise on macOS (other methods [here](https://mise.jdx.dev/getting-started.html)):

```sh-session
$ curl https://mise.jdx.dev/install.sh | sh
$ ~/.local/share/mise/bin/mise --version
mise 2024.0.0
```

Hook mise into your shell (pick the right one for your shell):

```sh-session
# note this assumes mise is located at ~/.local/share/mise/bin/mise
# which is what install.sh does by default
echo 'eval "$(~/.local/share/mise/bin/mise activate bash)"' >> ~/.bashrc
echo 'eval "$(~/.local/share/mise/bin/mise activate zsh)"' >> ~/.zshrc
echo '~/.local/share/mise/bin/mise activate fish | source' >> ~/.config/fish/config.fish
```

Install a runtime and set it as the global default:

```sh-session
$ mise use --global node@20
$ node -v
v20.0.0
```

## Full Documentation

See [mise.jdx.dev](https://mise.jdx.dev).
