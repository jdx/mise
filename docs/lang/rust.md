---
head:
  - - link
    - rel: canonical
      href: https://mise.jdx.dev/lang/rust
---

# Rust

Rust is not currently offered as a core plugin. In fact, I don't think you
should actually use mise for rust development. Rust has an official version
manager called [`rustup`](https://rustup.rs/) that is better than what any of
the current mise plugins offer.

You install [rustup](https://rustup.rs/) with the following:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

That said, rust is still one of the most popular languages to use in mise.
A lot of users have success with it so if you'd like to keep all of your
languages configured the same, don't feel like using mise is a bad idea either. Especially if you're only a casual rust user.

If you're a relatively heavy rust user making use of things like channel
overrides, components, and cross-compiling, then I think you really should
just be using rustup though. The experience will be better.

If one day we could figure out a way to provide an equivalent experience with
mise, we could revisit this. We have discussed potentially using mise as a
"front-end" to rustup where there is one rustup install that mise just manages
so you could do something like this:

```toml
[tools]
rust = "nightly"
```

Where that would basically be equivalent to:

```sh
rustup override set nightly
```

Frankly though, this isn't high on my priority list. Use rustup. It's great.

Kudos for writing rust too btw, I've really enjoyed it so far—this is my first rust project.

## Default crates

mise can automatically install a default set of creates right after installing a new rust version.
To enable this feature, provide a `$HOME/.default-cargo-crates` file that lists one crate per line, for
example:

```text
cargo-edit
stylua
```
