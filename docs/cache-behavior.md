---
description: "Refresh or clear the version lists, environments, and task results mise caches, and see when mise refetches them."
socialDescription: "Refresh or clear the version lists, environments, and task results that mise caches."
---

# Caches

mise caches tool metadata, computed environments, and task results separately.
Start with the cache that matches the symptom: clearing a version list does not
reinstall a tool, and clearing the environment cache does not change your
config.

```sh
mise cache path              # print the cache directory
mise cache clear node        # clear Node.js metadata
mise cache prune --dry-run   # list stale cache files
```

## Tool metadata cache {#tool-cache}

Backends store metadata under the [cache directory](/directories.html#cache-mise),
including remote version lists and, depending on the backend, aliases,
executable directories, and plugin `exec-env` output. Read these values through
commands such as `mise ls-remote node` rather than the cache files, whose format
can change.

### When mise refreshes version lists {#version-list-refresh}

Commands that run constantly use cached version lists however old they are, so
they never wait on the network: shell activation, `mise exec`, `mise env`,
`mise ls`, `mise current`, `mise which`, `mise where`, and shims. They fetch a
list only when none is cached.

Other commands, such as `mise install`, `mise use`, `mise upgrade`,
`mise outdated`, and `mise ls-remote`, refetch a list once it is older than
[`fetch_remote_versions_cache`](/configuration/settings.html#fetch_remote_versions_cache).
Set [`prefer_offline`](/configuration/settings.html#prefer_offline) to make
every command use cached lists the same way, or
[`offline`](/configuration/settings.html#offline) to block network requests
entirely.

To see a new release before the cached list expires:

```sh
mise cache clear node
mise ls-remote node
```

Some metadata comes from the
[versions host](/troubleshooting.html#new-version-of-a-tool-is-not-available),
which clearing the local cache does not refresh. A lockfile or a pinned version
can also keep a tool on an older version after the list is refreshed.

asdf plugins' `exec-env` output is cached; run `mise cache clear <tool>` after
you change a plugin.

## Environment cache <Badge type="warning" text="experimental" /> {#environment-caching}

The experimental [`env_cache`](/configuration/settings.html#env_cache) setting
caches computed environments on disk. It helps when environment providers are
slow or when mise runs nested inside itself:

```toml [~/.config/mise/config.toml]
[settings]
env_cache = true
env_cache_ttl = "4h"
```

The cache lives in `env-cache/` in the
[state directory](/directories.html#local-state-mise), not in the cache
directory. `mise activate` and `mise exec` create a session key that nested
commands inherit, and a cached environment is reused only with the same key, so
an unrelated session computes its own. The cache is encrypted on disk, but any
process that inherits the session key can read it.

The cache key covers config paths and modification times, resolved tool
versions, relevant settings, the base `PATH`, and the mise version. Entries
expire after [`env_cache_ttl`](/configuration/settings.html#env_cache_ttl), and
files that env plugins declare as watched invalidate them. Edits to dotenv files
or `_.source` scripts can still leave a nested command with a cached
environment; clear or disable the cache when such an edit does not show up.
Changes in an external service, such as a rotated secret, are not file changes,
so choose a suitable TTL.

mise does not cache secrets. A config with an `age`-encrypted value, a
sops-encrypted `_.file`, a directive with `redact = true`, or an env plugin that
returns `redact = true` is never written to the environment cache; mise
recomputes that environment every time.

Environment directives cannot opt out of caching one at a time, so a value such
as a timestamp template stays the same until the cache expires. Set
`MISE_ENV_CACHE=0` for a command that needs fresh values:

```sh
MISE_ENV_CACHE=0 mise exec -- npm test
```

Set `env_cache = false` to turn the cache off for every command. Env plugins
declare cacheability and watched files in their `MiseEnv` return value; see
[Environment plugins](/env-plugin-development.html).

`mise cache clear` also clears cached environments. You do not need to remove
installed tools or trust records to refresh an environment.

## Task cache {#task-caches}

Tasks can skip work when their sources have not changed, or restore cached
outputs. This is separate from the metadata and environment caches. For a task
named `build`:

```sh
mise cache task build
mise cache clear --task build
```

`--task` clears only the entries mise can tie to that task in this project.
`mise cache clear` with no arguments clears everything, including other
projects' task caches and the environment cache. See
[Task caching](/tasks/caching.html) for configuration and cache keys.

## Pruning {#cache-auto-pruning}

mise occasionally deletes cache files that have not been read within
[`cache_prune_age`](/configuration/settings.html#cache_prune_age). This is
separate from freshness: a version list can be refetched long before its file
is old enough to prune.

```sh
mise cache prune --dry-run
mise cache prune
```

Cached environments expire by their own TTL whatever the prune age is. Set
`cache_prune_age = "0s"` to turn off age-based pruning; both the occasional
sweep and `mise cache prune` then stop.

For cache keys in CI, see [Continuous integration](/continuous-integration.html).
