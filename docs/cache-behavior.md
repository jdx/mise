# Cache Behavior

mise uses caching in many places to be efficient. How long each cache is kept should eventually
be fully configurable. There may be gaps in the current behavior where things are hardcoded, but
I'm happy to add more settings to cover whatever config is needed.

Below I explain mise's caching behavior. If things don't appear to be updating, this is a good place
to start.

## Tool Cache

Each tool/backend has a cache that's stored in `$MISE_CACHE_DIR/<TOOL>` (by default `~/.cache/mise/<TOOL>`). It stores
the list of versions available for that tool (`mise ls-remote <TOOL>`), the idiomatic filenames,
the list of aliases, the bin directories within each tool installation, and the result of
running `exec-env` after the tool was installed.

Remote versions are refreshed after 1 hour by default, as configured by
[`fetch_remote_versions_cache`](/configuration/settings.html#fetch_remote_versions_cache). The file
is zlib-compressed MessagePack; to view it, run the following (requires
[msgpack-cli](https://github.com/msgpack/msgpack-cli)):

```sh
cat "${MISE_CACHE_DIR:-$HOME/.cache/mise}"/node/remote_versions.msgpack.z | perl -e 'use Compress::Raw::Zlib;my $d=new Compress::Raw::Zlib::Inflate();my $o;undef $/;$d->inflate(<>,$o);print $o;' | msgpack-cli decode
```

Caching `exec-env` may be problematic if the script does more than export static values, but
the vast majority of `exec-env` scripts only export static values.

Caching `exec-env` massively improved mise's performance, since running it requires calling bash
every time mise is initialized.

## Environment Caching

For more advanced caching needs (including dynamic environment providers like secret managers),
mise provides the [`env_cache`](/configuration/settings.html#env_cache) setting. When enabled,
mise caches the computed environment to disk with encryption.

```toml
# ~/.config/mise/config.toml
[settings]
env_cache = true
env_cache_ttl = "1h"  # optional, default is 1h
```

Cache invalidation happens automatically when:

- Any config file changes (mise.toml, .tool-versions, etc.)
- Tool versions change
- Settings change
- mise version changes
- TTL expires (configurable via `env_cache_ttl`)
- Any watched files change (from modules or `_.source` directives)

Env plugins (vfox modules) can declare themselves cacheable by returning `{cacheable = true, watch_files = [...]}`
from their `MiseEnv` hook. See [Env Plugin Development](/env-plugin-development.html) for details.

Directives can opt out of caching by setting `cacheable = false`:

```toml
[env]
TIMESTAMP = { value = "{{ now() }}", cacheable = false }
_.source = { path = "dynamic.sh", cacheable = false }
```

## Cache auto-pruning

mise automatically deletes old files in its cache directory (configured with [`cache_prune_age`](https://mise.jdx.dev/configuration/settings.html#cache_prune_age)). mise also
ignores much of the contents once they are more than 24 hours (or, for some entries, a few days) old. For this reason, storing this directory in CI jobs is likely wasteful.
