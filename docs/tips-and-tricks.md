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
mise-x64 use -g node@20
```

## Shebang

You can specify a tool and its version in a shebang without needing to first
setup `.tool-versions`/`mise.toml` config:

```typescript
#!/usr/bin/env -S mise x node@20 -- node
// "env -S" allows multiple arguments in a shebang
console.log(`Running node: ${process.version}`);
```

This can also be useful in environments where mise isn't activated
(such as a non-interactive session).

You can also download the https://mise.run script to use in a project bootstrap script:

```sh
curl https://mise.run > setup-mise.sh
chmod +x setup-mise.sh
./setup-mise.sh
```

::: tip
This file contains checksums so it's more secure to commit it into your project rather than
calling `curl https://mise.run` dynamically—though of course this means it will only fetch
the version of mise that was current when the script was created.
:::

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
      - run: node -v # will be the node version from `mise.toml`/`.tool-versions`
```

## `mise set`

Instead of manually editing `mise.toml` to add env vars, you can use [`mise set`](/cli/set.html) instead:

```sh
mise set NODE_ENV=production
```

## [`mise run`](/cli/run.html) shorthand

As long as the task name doesn't conflict with a mise-provided command you can skip the `run` part:

```sh
mise test
```

::: warning
Don't do this inside of scripts because mise may add a command in a future version and could conflict with your task.
:::

## Software verification

Install cosign, slsa-verify, and gpg (cosign and slsa-verify can be installed with mise) in order to verify tools automatically.

```sh
brew install gpg
mise use -g cosign slsa-verify
```

## [`mise up --bump`](/cli/upgrade.html)

Use `mise up --bump` to upgrade all software to the latest version and update `mise.toml` files. This keeps the same semver range as before,
so if you had `node = "20"` and node 22 is the latest, `mise up --bump node` will change `mise.toml` to `node = "22"`.

## cargo-binstall

cargo-binstall is sort of like ubi but specific to rust tools. It fetches binaries for cargo releases. mise will use this automatically for `cargo:` tools if it is installed
so if you use `cargo:` you should add this to make `mise i` go much faster.

```sh
mise use -g cargo-binstall
```

## [`mise cache clear`](/cli/cache.html)

mise caches things for obvious reasons but sometimes you want it to use fresh data (maybe it's not noticing a new release). Run `mise cache clear` to remove the cache which
basically just run `rm -rf ~/.cache/mise/*`.

## [`mise en`](/cli/en.html)

`mise en` is a great alternative to `mise activate` if you don't want to always be using mise for some reason. It sets up the mise environment in your current directory
but doesn't keep running and updating the env vars after that.

## Auto-install when entering a project

Auto-install tools when entering a project by adding the following to `mise.toml`:

```toml
[hooks]
enter = "mise i -q"
```

## [`mise tool [TOOL]`](/cli/tool.html)

Get information about what backend a tool is using and other information with `mise tool [TOOL]`:

```sh
❯ mise tool ripgrep
Backend:            aqua:BurntSushi/ripgrep
Installed Versions: 14.1.1                 
Active Version:     14.1.1                 
Requested Version:  latest                 
Config Source:      ~/src/mise/mise.toml   
Tool Options:       [none]
```

## [`mise cfg`](/cli/config.html)

List the config files mise is reading in a particular directory with `mise cfg`:

```sh
❯ mise cfg
Path                                    Tools                                   
~/.config/mise/config.toml              (none)                                  
~/.mise/config.toml                     (none)                                  
~/src/mise.toml                         (none)                                  
~/src/mise/.config/mise/conf.d/foo.toml (none)                                  
~/src/mise/mise.toml                    actionlint, bun, cargo-binstall, cargo:…
~/src/mise/mise.local.toml              (none)
```

This is helpful figuring out which order the config files are loaded in to figure out which one is overriding.
