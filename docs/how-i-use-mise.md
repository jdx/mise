# How I use mise

This is a very different doc than the rest of the site. It's not my
intention to make this valuable to anyone. In fact it may end up being
useful to 0 people.

I'm probably the strangest user of mise out there. My use case is the
most atypical for a number of reasons I'll get to. That said, I often
find myself saying to friends "you know the way I use mise..." and thought
it might be useful to actually write down the way I use it in case
anyone is interested.

This is an advanced article. I'm not going to take the time to explain
the tools and techniques here. If you're curious, file an issue or ask
me in the Discord.

## My setup

I use a mac with fish shell and am a heavy homebrew user. I've been using
both for over a decade.

My main editor(s) are JetBrains products (IntelliJ, RustRover, Webstorm).
I also use nvim as a secondary editor (Astronvim with some minimal config).
I probably spend 70% of my time in JetBrains.

I tend to keep a terminal open (kitty) while working in JetBrains. I do not
often run tests or builds with in the IDE. Not sure why, just never been
in the habit of that. (Because of that these docs and possibly mise support in
IDEs may not be what it should be-it's just not how I work personally).

## `mise activate`

Unlike most mise users, I don't use `mise activate` or
shims at all unless I'm explicitly testing them-and that's rarely the
case. It certainly doesn't go into my `~/.config/fish/config.fish`.

Because I work on mise itself, I often need to rebuild it and run the code from my repo. For this, I have the following bash shim located in
`~/bin/@mise`:

```fish
#!/usr/bin/env bash
set -euo pipefail

exec cargo run -q --all-features --manifest-path ~/src/mise/Cargo.toml -- "$@"
```

:::info
The "@" prefix I use for things that will perform a rebuild-i.e.: they're slow.
:::

This way I can easily test mise in any directory with `@mise`. I probably
run this more often than without just out of habit. For example, if I want to test `mise activate` in zsh:

```sh
zsh
eval "$(@mise activate zsh)"
```

## Minimal tools

Might be surprising to folks but I don't use too many mise plugins. Well
I have a lot in my config, but I don't actually use them. They're for
testing.

I tend to basically just use core plugins. I like mise for managing
things where I really care about the major version (like node). If it's
something like `shfmt` or `jq` I don't really care about the version.
I just want the latest and for me, I find `brew` to be better suited to
that purpose.

I recognize that some people really like locking down their versions
across a team to keep things consistent. I think that's great too.
Part of this is that I'm currently at Amazon where the tooling story
is complicated let's just say-not in a bad way, just one where
integrating mise into the setup isn't as straightforward as a smaller
company would be.

Outside of Amazon I have a handful of open source projects, mostly
mise-related and mostly fairly simple. Also mostly rust where I don't
use mise anyways.

The one big exception here is node which I quite like mise for. I assume
others do to because it's by far the most popular language. You'd
probably guess that since it's my example in nearly all of the docs.

That said, part of the reason for doing that in the docs is that it's 4
characters and everyone knows what it is.

## `.mise.local.toml`

I'm a heavy user of this concept. I rarely like to actually commit `.mise.toml`
files into projects. I tend to see my mise config as my personal config that
I use within other projects that I don't particularly want to share with others.

Of course, this goes into my global gitconfig so I can easily add this to
shared projects without submitting a PR.

One day when tasks is out of experimental, I may do this a lot less since I
think tasks are one thing I really want to share. For me, the `[tools]`
section is just so easy to write I don't mind doing it and don't like
imposing the way that **I** setup my machine on others.

There is a social aspect of this as well that I'm conscious of. I'm
the author of `mise`. To me it's a little self-serving to go into a project
and add a config for my own project. I'd love if _someone else_ did that
instead.

## `~/.mise`

I often need to access mise's internals so I do the following:

```sh
ln -s ~/.mise ~/.config/mise
ln -s ~/.mise ~/.local/share/mise
ln -s ~/.mise ~/.local/state/mise
ln -s ~/.mise/cache ~/.cache/mise
```

It is good that mise generally follows XDG spec, but for tools that I interact
with a lot I like to put them at the top level like this. Obviously,
mise doesn't mind if all of these point to the same place or else it would
not work for me.
