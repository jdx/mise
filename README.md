<div align="center">
<h1><a href="https://mise.jdx.dev">
  <img src="https://github.com/jdx/mise/assets/216188/27a8ea18-9383-4d86-a445-305b9a6248c1" alt="mise-logo" width="400" /><br />
  mise-en-place
</a></h1>
<!-- <a href="https://mise.jdx.dev"><picture> -->
<!--   <source media="(prefers-color-scheme: dark)" width="617" srcset="./docs/logo-dark@2x.png"> -->
<!--   <img alt="mise logo" width="617" src="./docs/logo-light@2x.png"> -->
<!-- </picture></a> -->
<a href="https://crates.io/crates/mise"><img alt="Crates.io" src="https://img.shields.io/crates/v/mise?style=for-the-badge"></a>
<a href="https://github.com/jdx/mise/blob/main/LICENSE"><img alt="GitHub" src="https://img.shields.io/github/license/jdx/mise?color=%2344CC11&style=for-the-badge"></a>
<a href="https://github.com/jdx/mise/actions/workflows/test.yml"><img alt="GitHub Workflow Status" src="https://img.shields.io/github/actions/workflow/status/jdx/mise/test.yml?style=for-the-badge"></a>
<a href="https://app.codacy.com/gh/jdx/mise/dashboard?utm_source=gh&utm_medium=referral&utm_content=&utm_campaign=Badge_coverage"><img alt="Codacy coverage (branch)" src="https://img.shields.io/codacy/coverage/af322e1f36ca41f0a296f49733a705f5/main?color=%2344CC11&style=for-the-badge"></a>
<a href="https://discord.gg/mABnUDvP57"><img alt="Discord" src="https://img.shields.io/discord/1066429325269794907?color=%23738ADB&style=for-the-badge"></a>
<p><em>The front-end to your dev env.</em></p>
</div>

## What is it?

- Like [asdf](https://asdf-vm.com) (or [nvm](https://github.com/nvm-sh/nvm) or [pyenv](https://github.com/pyenv/pyenv) but for any language) it manages [dev tools](https://mise.jdx.dev/dev-tools/) like node, python, cmake, terraform, and [hundreds more](https://mise.jdx.dev/plugins.html).
- Like [direnv](https://github.com/direnv/direnv) it manages [environment variables](https://mise.jdx.dev/environments.html) for different project directories.
- Like [make](https://www.gnu.org/software/make/manual/make.html) it manages [tasks](https://mise.jdx.dev/tasks/) used to build and test projects.

## 30 Second Demo

The following shows using mise to install different versions
of [node](https://nodejs.org).
Note that calling `which node` gives us a real path to node, not a shim.

[![demo](./docs/demo.gif)](./docs/demo.gif)

## Quickstart

Install mise (other methods [here](https://mise.jdx.dev/getting-started.html)):

```sh-session
$ curl https://mise.run | sh
$ ~/.local/bin/mise --version
2024.9.9 macos-arm64 (a1b2d3e 2024-09-25)
```

or install a specific a version:

```sh-session
$ curl https://mise.run | MISE_VERSION=v2024.5.16 sh
$ ~/.local/bin/mise --version
2024.5.16 macos-arm64 (8838098 2024-05-14)
```

Hook mise into your shell (pick the right one for your shell):

```sh-session
# note this assumes mise is located at ~/.local/bin/mise
# which is what https://mise.run does by default
echo 'eval "$(~/.local/bin/mise activate bash)"' >> ~/.bashrc
echo 'eval "$(~/.local/bin/mise activate zsh)"' >> ~/.zshrc
echo '~/.local/bin/mise activate fish | source' >> ~/.config/fish/config.fish
```

Install a runtime and set it as the global default:

```sh-session
$ mise use --global node@20
$ node -v
v20.0.0
```

## Full Documentation

See [mise.jdx.dev](https://mise.jdx.dev)

## Contributors

[![Contributors](https://contrib.rocks/image?repo=jdx/mise)](https://github.com/jdx/mise/graphs/contributors)
