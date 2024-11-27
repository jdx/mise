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
static values. The vast majority of `exec-env` scripts only export static values, but if you're
working with a plugin that has a dynamic `exec-env` submit
a ticket and we can try to figure out what to do.

Caching `exec-env` massively improved the performance of mise since it requires calling bash
every time mise is initialized. Ideally, we can keep this
behavior.
