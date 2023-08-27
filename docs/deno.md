# Deno in rtx

The following are instructions for using the deno rtx core plugin. This is used when there isn't a 
git plugin installed named "deno".

If you want to use [asdf-deno](https://github.com/asdf-community/asdf-deno)
then run `rtx plugins install deno https://github.com/asdf-community/asdf-deno`.

The code for this is inside the rtx repository at
[`./src/plugins/core/deno.rs`](https://github.com/jdx/rtx/blob/main/src/plugins/core/deno.rs).

## Usage

The following installs deno and makes it the global default:

```sh-session
$ rtx use -g deno@1       # install deno 1.x
$ rtx use -g deno@latest  # install latest deno
```

See available versions with `rtx ls-remote deno`.
