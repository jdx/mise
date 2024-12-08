---
# https://vitepress.dev/reference/default-theme-home-page
layout: home
title: Home

hero:
  name: mise-en-place
  tagline: |
    The front-end to your dev env
    <span class="formerly">Pronounced "MEEZ ahn plahs"</span>
  actions:
    - theme: brand
      text: Getting Started
      link: /getting-started
    - theme: alt
      text: About
      link: /about
    - theme: alt
      text: GitHub
      link: https://github.com/jdx/mise
    - theme: alt
      text: Discord
      link: https://discord.gg/UBa7pJUN7Z

features:
  - title: <a href="/dev-tools/">Dev Tools</a>
    details: mise is a polyglot tool version manager. It replaces tools like <a href="https://asdf-vm.com">asdf</a>, nvm, pyenv, rbenv, etc.
  - title: <a href="/environments.html">Environments</a>
    details: mise allows you to switch sets of env vars in different project directories. It can replace <a href="https://github.com/direnv/direnv">direnv</a>.
  - title: <a href="/tasks/">Tasks</a>
    details: mise is a task runner that can replace <a href="https://www.gnu.org/software/make">make</a>, or <a href="https://docs.npmjs.com/cli/v10/using-npm-scripts">npm scripts</a>.
---

## Powerful polyglot tool version manager
Mise can manage multiple tools and runtimes like `node`, `python`, `java`, `go`, `ruby`, `terraform`, etc. in one place. 

With a simple configuration file, you can specify which version of each tool to use. `mise` will automatically switch between different versions of tools based on the directory you're in.

```shell
~/my-project > mise use node@22 python@3 go@latest
# mise go@1.x.x ✓ installed
# mise node@22.x.x ✓ installed
# mise python@3.x.x ✓ installed
# mise ~/my-project/mise.toml tools: go@1.x.x, python@3.x.x, node@22.x.x                                                              
```
::: code-group
```toml [mise.toml]
[tools]
node = "22"
python = "3"
go = "latest"
```
:::

```shell
~/my-project > node -v
# v22.x.x

~/my-project > which node
# ~/.local/mise/installs/node/22.x.x/bin/node
```

## Manage environment variables
Mise can manage environment variables for different project directories. Like `direnv`, it will automatically switch between different sets of environment variables as you move between projects.

```shell
~/my-project > mise env set FOO=bar
```
::: code-group
```toml [mise.toml]
# ...
[env]
FOO = "bar"
```
:::

```shell
cd ~/my-project
echo $FOO
# bar
```

## Task runner

Powerful task runner that can replace `make`, `just`, `npm scripts`, etc. leveraging tools and environment variables.

::: code-group
```toml [mise.toml]
# ...
[task.test]
run = "echo 'runing tests...'"

[tasks.my_task]
run = "node -e 'console.log(process.version); console.log(process.env.FOO)'"
depends = ["test"]
```
:::
```shell
> mise tasks ls
# my_task     ~/.my-project/mise.toml
# test        ~/.my-project/mise.toml

> mise run my_task
# [test] runing tests...
# [my_task] v22.x.x
# [my_task] bar
```

## mise works with your existing setup
You do not need to use `mise` for everything. You can use it only for `tool` or for `tasks`. 
`mise` interoperates with your extisting setup! 

### Idiomatic files and asdf compatibility
mise works with idiomatic files like `.nvmrc`, `.python-version`, etc. or asdf `.tool-versions` files.

::: code-group
```txt [.nvmrc]
22
```
```txt [.python-version]
3.13
```
```txt [.tool-versions]
go 1.23.4
```
:::
```shell
> mise install
# mise go@1.23.4 ✓ installed
# mise node@22.12.0 ✓ installed
# mise python@3.13.1 ✓ installed
```

### `.env` files support
mise can load `.env` files.
```toml
[env]
_.file = ".env"
```

### Standalone file tasks
mise also works with your existing shell scripts. Tasks can be as standalone files!
```shell
> cat mise-tasks/build
| #!/usr/bin/env bash
| #MISE description="Build the CLI"
| cargo build
```

```shell
> mise task ls
# build   Build the CLI   ~/.my-project/mise-tasks
```

```shell
> mise run build
# cargo build
```

<center style="margin-top: 2em">
    <a href="/getting-started" class="get-started-button">Getting Started</a>
</center>

<style>
.formerly {
    font-size: 0.7em;
    color: #666;
}

a.get-started-button {
    display: inline-block;
    padding: 0.5em 1em;
    border-radius: 0.25em;
    color: var(--vp-button-brand-text);
    text-decoration: none;
    background-color: var(--vp-button-brand-bg);
}
a.get-started-button:hover {
    color: var(--vp-button-brand-hover-text);
}

</style>
