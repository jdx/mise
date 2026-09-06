# Cache Behavior

mise caches version metadata, computed environments, and task results separately. Start with
the cache related to the symptom: clearing a version list does not reinstall a tool, and
clearing an environment cache does not change your configuration.

```sh
mise cache path                 # show the actual cache directory
mise cache clear node           # clear Node's tool metadata
mise cache prune --dry-run      # preview stale cache files
```

## Tool Cache

Backends store metadata below [`MISE_CACHE_DIR`](/directories.html#cache-mise), including
remote version lists and, where applicable, aliases, executable directories, and plugin
`exec-env` results. The exact files depend on the backend. Inspect values through mise's
commands, such as `mise ls-remote node`, rather than depending on internal cache formats.

Remote version lists are fresh for one hour by default, controlled by
[`fetch_remote_versions_cache`](/configuration/settings.html#fetch_remote_versions_cache).
To check again before that period expires:

```sh
mise cache clear node
mise ls-remote node
```

Some metadata also comes from the [versions host](/troubleshooting.html#new-version-of-a-tool-is-not-available).
Clearing the local cache does not refresh that remote service. A lockfile or explicit version
pin can also keep an install on an older version even after the metadata has been refreshed.

asdf plugins' `exec-env` output is cached to avoid starting Bash for every environment
calculation. Plugin authors should use it for environment values tied to the installation;
dynamic project configuration belongs in [environment directives](/environments/).

## Environment Caching

The experimental [`env_cache`](/configuration/settings.html#env_cache) setting caches computed
environments on disk. It can help with expensive environment providers and nested mise calls:

```toml
# ~/.config/mise/config.toml
[settings]
env_cache = true
env_cache_ttl = "1h" # optional; the default is one hour
```

The cache lives under the state directory's `env-cache/`, not the tool metadata cache.
`mise activate` and `mise exec` establish an encryption key inherited by nested commands.
Cache reuse requires the same key; starting an unrelated session does not guarantee a cache
hit. The cache is encrypted on disk, but a process that inherits the session key can read it.

The cache key includes config paths and modification times, resolved tool versions, relevant
settings, the base `PATH`, and the mise version. Entries also expire after `env_cache_ttl`,
and plugin-declared watched files can invalidate them. File-watch coverage depends on the
directive: edits to dotenv files or `_.source` scripts can still leave a nested command using
a cached environment. Clear or disable the cache if those edits are not reflected.
Changes in an external service, such as a
rotated secret, are not file changes: choose a suitable TTL or disable environment caching.

For a command that must recompute environment values, set `MISE_ENV_CACHE=0` before starting
mise. For example, in a Node.js project with a `test` script:

```sh
MISE_ENV_CACHE=0 mise exec -- npm test
```

To disable the cache for all commands, set `env_cache = false`. Ordinary environment
directives do not currently support a per-value `cacheable = false` option. A timestamp template can
therefore be reused while an environment cache is valid.

Env plugins declare cacheability and watched files in their `MiseEnv` return value.
See [Env Plugin Development](/env-plugin-development.html)
for the Lua return format.

To refresh cached environments and metadata together, run `mise cache clear`. There is no
need to remove installed tools or trust records to refresh an environment.

## Task caches

Tasks can skip work based on source/output freshness or restore previously cached outputs.
These are separate from version and environment caches. For a task named `build`:

```sh
mise cache task build
mise cache clear --task build
```

See [task caching](/tasks/caching.html) for configuration, cache keys, and rerun behavior.
`--task` resolves task names in the current configuration and removes entries whose ownership
can be verified. It skips legacy entries without verifiable task ownership. A full
`mise cache clear` removes all entries under the cache roots, including other projects
and those legacy entries, as well as the environment cache.

## Cache auto-pruning

mise occasionally prunes files that have not been accessed within
[`cache_prune_age`](/configuration/settings.html#cache_prune_age), which defaults to 30 days.
This is different from a cache entry's freshness period: an expired version list may be
refetched long before the file becomes old enough to prune.

```sh
mise cache prune --dry-run
mise cache prune
```

Environment entries use their own TTL during pruning. Set `cache_prune_age = "0s"` to disable
automatic age-based pruning. Preview an explicit prune command before relying on its effect.

For [CI](/continuous-integration.html), caching installed tools usually saves the most work.
Metadata caches can still help repeated jobs; choose cache keys for the runner platform and
project configuration, and make sure the pipeline also works with no restored cache.
