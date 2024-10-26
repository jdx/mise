---
head:
  - - link
    - rel: canonical
      href: https://mise.jdx.dev/lang/deno
---

# Deno

The following are instructions for using the deno mise core plugin. This is used when there isn't a
git plugin installed named "deno".

If you want to use [asdf-deno](https://github.com/asdf-community/asdf-deno)
then run `mise plugins install deno https://github.com/asdf-community/asdf-deno`.

The code for this is inside the mise repository at
[`./src/plugins/core/deno.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/deno.rs).

## Usage

The following installs deno and makes it the global default:

```sh
mise use -g deno@1       # install deno 1.x
mise use -g deno@latest  # install latest deno
```

See available versions with `mise ls-remote deno`.
