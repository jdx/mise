# Cache Behavior

mise makes use of caching in many places in order to be efficient. The details about how long to keep
cache for should eventually all be configurable. There may be gaps in the current behavior where
things are hardcoded, but I'm happy to add more settings to cover whatever config is needed.

Below I explain the behavior it uses around caching. If you're seeing behavior where things don't appear
to be updating, this is a good place to start.

## Plugin/Runtime Cache

Each plugin has a cache that's stored in `~/$MISE_CACHE_DIR/<PLUGIN>`. It stores
the list of versions available for that plugin (`mise ls-remote <PLUGIN>`), the idiomatic filenames (see below),
the list of aliases, the bin directories within each runtime installation, and the result of
running `exec-env` after the runtime was installed.

Remote versions are updated daily by default. The file is zlib messagepack, if you want to view it you can
run the following (requires [msgpack-cli](https://github.com/msgpack/msgpack-cli)).

```sh
cat ~/$MISE_CACHE_DIR/node/remote_versions.msgpack.z | perl -e 'use Compress::Raw::Zlib;my $d=new Compress::Raw::Zlib::Inflate();my $o;undef $/;$d->inflate(<>,$o);print $o;' | msgpack-cli decode
```

Note that the caching of `exec-env` may be problematic if the script isn't simply exporting
static values. The vast majority of `exec-env` scripts only export static values.

Caching `exec-env` massively improved the performance of mise since it requires calling bash
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
_.source = { file = "dynamic.sh", cacheable = false }
```

## Cache auto-pruning

mise will automatically delete old files in its cache directory (configured with [`cache_prune_age`](https://mise.jdx.dev/configuration/settings.html#cache_prune_age)). Much of
the contents are also ignored by mise if they are >24 hours old or a few days. For this reason, it's likely wasteful to store this directory in CI jobs.
