# Tips & Tricks

An assortment of helpful tips for using `mise`.

## macOS Rosetta

If you have a need to run tools as x86_64 on Apple Silicon, this can be done with mise however you'll currently
need to use the x86_64 version of mise itself. There is an [outstanding issue](https://github.com/jdx/mise/issues/405)
to support this with an env var like MISE_ARCH=x86_64 to make it more seamless.

A common reason for doing this is to support compiling node <=14.

First, you'll need a copy of mise that's built for x86_64:

```sh
$ mkdir -p ~/.local/bin
$ curl https://mise.jdx.dev/mise-latest-macos-x64 > ~/.local/bin/mise-x64
$ chmod +x ~/.local/bin/mise-x64
$ ~/.local/bin/mise-x64 --version
mise 2024.x.x
```

::: warning
If `~/.local/bin` is not in PATH, you'll need to prefix all commands with `~/.local/bin/mise-x64`.
:::

Now you can use `mise-x64` to install tools:

```sh
$ mise-x64 use -g node@20
```

## Shebang

You can specify a tool and its version in a shebang without needing to first
setup `.tool-versions`/`.mise.toml` config:

```typescript
#!/usr/bin/env -S mise x node@20 -- node
// "env -S" allows multiple arguments in a shebang
console.log(`Running node: ${process.version}`);
```

This can also be useful in environments where mise isn't activated
(such as a non-interactive session).

## CI/CD

Using mise in CI/CD is a great way to synchronize tool versions for dev/build.

### GitHub Actions

mise is pretty easy to use without an action:

```yaml
jobs:
  build:
    steps:
    - run: |
        curl https://mise.jdx.dev/install.sh | sh
        echo "$HOME/.local/bin" >> $GITHUB_PATH
        echo "$HOME/.local/share/mise/shims" >> $GITHUB_PATH
```

Or you can use the custom action [`jdx/mise-action`](https://github.com/jdx/mise-action):

```yaml
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: jdx/mise-action@v1
      - run: node -v # will be the node version from `.mise.toml`/`.tool-versions`
```

## Fish

I've been a happy fish user for over a decade. I wholeheartedly recommend
it. Unlike zsh, you'll find that you really don't need to configure it to
make it "good". Out of the box it contains nearly everything you need.

The one thing I would say to consider (and isn't actually fish-specific)
is [starship](https://starship.rs) which I love.

One thing I really want to evangelize when using fish is not to manually
edit files in `~/.config/fish`. Instead, use fish commands. For example,
if you want to permanently set an env var, don't do what you would do in
zsh and add it to ~/.config/fish/config.fish, instead just run this:

```sh
$ set -Ux MISE_EXPERIMENTAL 1
```

Those "universal" variables are persisted through new shells so you set
it forever. Also for editing PATH:

```sh
$ fish_add_path ~/.local/bin
```

I avoided this for the first many years of using fish and I really regret
that now. It saves so much time doing things this way.

Now, for mise specifically, I really recommend the following aliases:

:::code-group
```sh [mise exec]
$ alias -s x "mise exec --"
$ x node --version
```
```sh [mise run]
$ alias -s r "mise run --"
$ r build
```
:::

:::tip
The `--` here makes it so mise knows that everything after is a "positional argument"
at least as far as mise is considered.

As of this writing I require it for all `mise exec` commands because I like the explicitness.
I did not go with that decision for `mise run` but the current behavior will
break if you do something like `mise run build --foo` where `--foo` is not
something that mise understands but should go to the task.

Since tasks are experimental I'm still trying to decide if that's the
way to go or not. While it's nice to be able to just run `mise run build`,
it also means the second you add a flag it breaks it and you need to
switch to `mise run -- build --foo`, which in my mind is more annoying
than if the `--` was always required in the first place.

Anyhow, the alias is great here. It does mean you can't pass any flags
if you intend them to go to mise, but in that case just don't use the
alias.
:::

## Bash/Zsh

Following the fish example above, you can setup the following aliases:

:::code-group
```sh [mise exec]
$ alias x="mise exec --"
$ x node --version
```
```sh [mise run]
$ alias r="mise run --"
$ r build
```
:::
