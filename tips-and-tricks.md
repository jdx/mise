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
        curl https://mise.run | sh
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

## `mise set`

Instead of manually editing `.mise.toml` to add env vars, you can use `mise set` instead:

```sh
$ mise set NODE_ENV=production
```
