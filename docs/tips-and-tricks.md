# Tips & Tricks

An assortment of helpful tips for using `mise`.

## macOS Rosetta

If you have a need to run tools as x86_64 on Apple Silicon, this can be done with mise however you'll currently
need to use the x86_64 version of mise itself. A common reason for doing this is to support compiling node <=14.

You can do this either with the [`MISE_ARCH`](https://mise.jdx.dev/configuration/settings.html#arch)
setting or by using a dedicated rosetta mise bin as described below:

First, you'll need a copy of mise that's built for x86_64:

```sh
$ curl https://mise.run | MISE_INSTALL_PATH=~/.local/bin/mise-x64 MISE_INSTALL_ARCH=x64 sh
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
set up a `mise.toml`/`.tool-versions` config:

```typescript
#!/usr/bin/env -S mise x node@20 -- node
// "env -S" allows multiple arguments in a shebang
console.log(`Running node: ${process.version}`);
```

This can also be useful in environments where mise isn't activated
(such as a non-interactive session).

## Bootstrap script

You can download the <https://mise.run> script to use in a project bootstrap script:

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

## Installation via zsh zinit

[Zinit](https://github.com/zdharma-continuum/zinit) is a plugin manager for ZSH, which this snippet you will get mise (and usage for shell completion):

```sh
zinit as="command" lucid from="gh-r" for \
    id-as="usage" \
    atpull="%atclone" \
    jdx/usage
    #atload='eval "$(mise activate zsh)"' \

zinit as="command" lucid from="gh-r" for \
    id-as="mise" mv="mise* -> mise" \
    atclone="./mise* completion zsh > _mise" \
    atpull="%atclone" \
    atload='eval "$(mise activate zsh)"' \
    jdx/mise
```

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
      - uses: jdx/mise-action@v2
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

mise provides **native software verification** for aqua tools without requiring external dependencies. For aqua tools, Cosign/Minisign signatures, SLSA provenance, and GitHub attestations are verified automatically using mise's built-in implementation.

For other verification needs (like GPG), you can install additional tools:

```sh
brew install gpg
# Note: cosign and slsa-verifier are no longer needed for aqua tools
# mise now handles verification natively
```

To configure aqua verification (all enabled by default):

```sh
# Disable specific verification methods if needed
export MISE_AQUA_COSIGN=false
export MISE_AQUA_SLSA=false
export MISE_AQUA_GITHUB_ATTESTATIONS=false
export MISE_AQUA_MINISIGN=false
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

## `mise.lock`

If you enable experimental mode, mise will update `mise.lock` with full versions and tarball checksums (if supported by the backend).
These can be updated with [`mise up`](/cli/upgrade.html). You need to manually create the lockfile, then mise will add the tools to it:

```sh
touch mise.lock
mise i
```

The lockfile uses a consolidated format with `[tools.name.assets]` sections to organize asset information under each tool. Asset information includes checksums, file sizes, and optional download URLs. Legacy lockfiles with separate `[tools.name.checksums]` and `[tools.name.sizes]` sections are automatically migrated to the new format.

Note that at least currently mise needs to actually install the tool to get the tarball checksum (otherwise it would need to download the tarball just
to get the checksum of it since normally that gets deleted). So you may need to run something like `mise uninstall --all` first in order to have it
reinstall everything. It will store the full versions even if it doesn't know the checksum though so it'll still lock the version just not have a checksum
to go with it.

## Lockfile URL Tracking (Avoiding Rate Limits)

When you use a lockfile (`mise.lock`), mise stores the exact download URLs for each tool asset. This means that after the initial install, future `mise install` runs will use the URLs from the lockfile instead of making API calls to GitHub (or other providers). This has several benefits:

- **Avoids GitHub API rate limits**: No need to make repeated API calls for every install, which can quickly exhaust your rate limit, especially in CI or large teams.
- **No need for GITHUB_TOKEN**: Since the URLs are already known, you don’t need to set up a `GITHUB_TOKEN` for simple installs.
- **Faster installs**: Skipping API lookups speeds up repeated installs.

This is especially useful in CI/CD or when working in environments with strict network or authentication requirements.
