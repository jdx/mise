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

## What makes me unique

I'm often working on mise itself. Even when I am working on other projects
I still use mise quite a bit but often I'm experimenting and trying new ideas.

I am also quite different personally. I have a bizarre obsession with
the CLI environment. I am constantly experimenting with new techniques
and tools. My environment is often in a state of disarray.

I _do not_ like shell extensions-of which `mise activate` is certainly one.
Now this isn't me telling you how I think you should be whatsoever,
(please don't think of _any_ of this doc as that). I will explain my
story around these a bit though.

Back when I first got out of college, I was a rails dev. I remember
people using `bundle exec` (often aliased to "be"), to run the CLIs
that gems provide. At the time, I found that maddening. Why on earth
would I run `be rails c` when I could just run `rails c`? I actually
don't think I understood what `be` actually did-though I'm positive my
exuberant personality at the time would claim I did.

This ties into my development with mise itself actually. I put myself
back in those shoes using rbenv or chruby or whatever I was using. I
really didn't want to know about shims and these version managers. I
was interested in ruby/rails at the time and the CLI stuff was a
distraction for me.

I empathize with that past self and that's the reason the
"golden path" for mise is to use `mise activate`. I'm making it for
my 25 year-old self that didn't want to understand this stuff so I've
tried to make it easy, reliable, and to do the right thing with as
minimal "magic" as necessary.

Now what I've discovered along the way is I actually don't really like
using version managers! Before mise, I would just manage path directly
and use `./configure && make && make install` (with a special PREFIX) 
to install even things like node and python! Once I started writing CLIs as my full time job,
I gained a much deeper understanding of things like `PATH` and how it is
intended to be used. I bet more than a few of you are rolling your eyes
at the idea that `PATH` could be complicated. I would argue, however,
that there is actually an art around these things that's actually quite
profound. It also certainly _becomes_ quite complicated once you start
asking questions like "how does xcode get PATH?", or "is it .profile or .bash_profile that runs first?", and a million other tiny questions
I find myself asking again and again because these small details actually matter a ton.

Anyways, not sure if I'm actually arguing with a mystery person there,
but what I'm getting at is I developed a deeper interest and understanding around the shell in general. I became quite picky about being explicit over making things easy for myself.

I actually don't think people should follow my lead here. Not unless
you also specifically have a passion for dev tools like me. I think
the average person should learn as little as possible about the shell
to get their work done. I think that idea is often mocked with engineers
but I think that's wrong.

There are whole universes of technology I have very little knowledge of.The shell is mine. You could say that mise is my attempt for me to take
the load off of others and put the burden of the shell onto myself.

Now, if that statement rubs you the wrong way, like maybe you think
it's not the tool for you, well I suspect this article actually might
be what you're looking for. If I'm able to have success with mise-and
that wasn't always the case, I didn't use it myself until relatively
recently-then I think you can too.

Perhaps one day there will be a "I don't care about the shell" and
"I care about the shell a lot" method for using mise. Not sure.

:::tip
I'll get to this later, but the essential difference here is that the
"I care a lot" approach is not using `mise activate` and instead relying
on `mise exec`, and `mise run`.
:::

## `mise activate`

Unlike probably nearly all mise users, I don't use `mise activate` or
shims at all unless I'm explicitly testing them-and that's rarely the
case. It certainly doesn't go into my `~/.config/fish/config.fish`.

Because I work on mise itself, I often need to rebuild it and run the code from my repo. For this, I have the following bash shim located in
`~/bin/@mise`:

```fish
#!/usr/bin/env bash
set -euo pipefail

cargo run -q --all-features --manifest-path ~/src/mise/Cargo.toml -- "$@"
```

:::info
The "@" prefix I use for things that will perform a rebuild-i.e.: they're slow.
:::

This way I can easily test mise in any directory with `@mise`. I probably
run this more often than without just out of habit. For example, if I want to test `mise activate` in zsh:

```sh
$ zsh
$ eval "$(@mise activate zsh)"
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
$ ln -s ~/.mise ~/.cache/mise
$ ln -s ~/.mise ~/.config/mise
$ ln -s ~/.mise ~/.local/share/mise
$ ln -s ~/.mise ~/.local/state/mise
```

It is good that mise generally follows XDG spec, but for tools that I interact
with a lot I like to put them at the top level like this. Obviously,
mise doesn't mind if all of these point to the same place or else it would
not work for me.
