---
description: "Watch Meez Run, a music video about mise, with captions and lyrics."
---

<script setup>
import videoUrl from "./tapes/meez-run.mp4?url";
</script>

# Meez Run

Everything in its place, set to a beat. **Meez Run** is a music video about mise,
with animated lyrics and illustrations of tools, environments, and tasks.

<video style="max-width: 100%; height: auto;" controls preload="metadata" playsinline aria-label="Meez Run music video" poster="./tapes/meez-run.png" :src="videoUrl">
  <track kind="captions" src="/meez-run.en.vtt" srclang="en" label="English" />
  Your browser does not support embedded video. Use the download link below to watch it.
</video>

<a :href="videoUrl" download="meez-run.mp4">Download the video (MP4)</a> · [Read the lyrics](#lyrics)

To try mise in your own projects, start with [getting started](/getting-started.html)
or follow the [walkthrough](/walkthrough.html).

## Lyrics {#lyrics}

::: details Read the full lyrics

### Intro

meez... meez... meez-ahn-plahs\
Everything... in its place.\
Put your hands up if your shell just works!

### Verse 1

CI's red at three a.m.\
Pinned it once, it drifted again\
Now the lockfile holds the line\
Laptop, CI, same every time\
Hooked my shell, I cd with ease\
Everything in its place... meez

### Build-Up

Give me one file, give me ease\
Give me one command, give me meez\
One file! (One file!)\
One command! (One command!)\
When I say meez, you say run!\
Meez! (Run!)\
Meez! (Run!)\
Fifty thousand terminals in the air!\
Say...

### Drop

Meez run!\
Meez run!\
meez-meez-meez-meez\
Meez run!\
Meez! (Run!)\
Meez! (Run!)

### Verse 2

Who pinned Node? (mise use!)\
New hire Monday? (mise install!)\
Who runs the tests? (mise run test!)\
Where's the config? (mise.toml!)\
nvm? (Gone!)\
pyenv, rbenv? (Gone!)\
Makefile, direnv? (Gone! Gone! Gone!)\
One tool! (Meez!)

### Breakdown

Before the doors open\
Before the heat\
Every blade laid out\
And my heart on the beat\
Let the whole world rush\
I'll be standing at ease\
Everything in its place\
'Cause I've got my meez

### Build-Up 2

Somewhere out there, they're rebuilding the world... just to boil water.\
Your flake's still flaking (flake!)\
Your store's forty gigs (gigs!)\
Still experimental (what?)\
Year after year (year!)\
Error... infinite recursion encountered\
You spent the weekend configuring (config!)\
We spent it shipping (ship!)\
Ship! Ship! Ship! Ship!\
Meez is Nix for people with work to do!\
...so get to work.

### Drop 2

Meez run!\
Meez run!\
mee-mee-mee-meez!\
Meez run!\
Every-thing in its place!\
Meez! (Run!)\
Meez! (Run!)\
Let me hear you, people with work to do!\
Meez run!

### Outro

Everything in its place\
Everything in its place\
I've got my meez\
...exit code zero.

:::
