<script setup lang="ts">
// Adapted from jdx/hk@37937824 docs/.vitepress/theme/HomeShowreel.vue.
//
// The reel is rendered to MP4 files by `mise run docs:showreel` (the docs
// deploy runs it), so this is a plain video player. Builds without a render
// leave the section out, and the page is what it was before the reel. Only
// showreel.data.ts is imported here: it draws nothing, so it is safe to
// server-render.
import { withBase } from "vitepress";
import { nextTick, onMounted, onUnmounted, ref } from "vue";
import { data as showreel } from "../showreel.data";

/** 3:26 as "3 minutes 26 seconds", for the labels a screen reader reads. */
function spoken(seconds: number) {
  const s = Math.round(seconds);
  const [m, r] = [Math.floor(s / 60), s % 60];
  const unit = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  return [m && unit(m, "minute"), r && unit(r, "second")]
    .filter(Boolean)
    .join(" ");
}
/** A chapter's start as the player shows it, 0:19. */
const clock = (seconds: number) => {
  const s = Math.floor(seconds + 1e-6);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};
const length = showreel ? spoken(showreel.seconds) : "";
const runtime = showreel ? clock(showreel.seconds) : "";
const chapters = (showreel?.chapters ?? []).map((c) => ({
  ...c,
  at: clock(c.start),
}));

// The page is served with the 60 fps file, which plays everywhere. Once it is
// mounted, and before anyone presses play, it switches to the 120 fps file if
// the browser says it decodes that smoothly and power-efficiently (in
// practice, in hardware). This tests the decoder, not the display, so a
// capable 60 Hz screen gets the larger file too. Nothing downloads until play.
const player = ref<HTMLVideoElement>();
const button = ref<HTMLButtonElement>();
const src = ref(showreel?.src ?? "");

// The native controls keep a fixed height while the video shrinks, so on a
// phone they cover the poster's caption band. Until someone starts the reel,
// the player shows its own play button instead, over the middle of the
// frame, which holds no text that must be read. The server-rendered page
// keeps the controls, so the player works without JavaScript; they go once
// the page is hydrated.
const hydrated = ref(false);
const started = ref(false);
function play() {
  started.value = true;
  const video = player.value;
  if (!video) return;
  // A failed start leaves the controls up to show it.
  video.play().catch(() => {});
  // The button is gone; keep keyboard focus on the player.
  void nextTick(() => video.focus({ preventScroll: true }));
}

// "Watch the demo" links here (/#showreel). The router scrolls; focus moves
// to the play button, so Enter or Space starts the reel.
function focusOnArrival() {
  if (location.hash !== "#showreel" || started.value) return;
  void nextTick(() => button.value?.focus({ preventScroll: true }));
}

onMounted(async () => {
  hydrated.value = true;
  focusOnArrival();
  window.addEventListener("hashchange", focusOnArrival);
  const video120 = showreel?.video120;
  if (!video120 || !navigator.mediaCapabilities) return;
  try {
    const { smooth, powerEfficient } =
      await navigator.mediaCapabilities.decodingInfo({
        type: "file",
        video: {
          // H.264 High at level 5.1, as the renderer encodes it.
          contentType: 'video/mp4; codecs="avc1.640033"',
          width: 1920,
          height: 1080,
          framerate: 120,
          bitrate: video120.bitrate,
        },
      });
    // Someone who already pressed play keeps the file that is playing.
    const idle =
      player.value?.paused &&
      player.value.readyState === HTMLMediaElement.HAVE_NOTHING;
    if (smooth && powerEfficient && idle) src.value = video120.src;
  } catch {
    // Older browsers reject the query; they keep the 60 fps file.
  }
});
onUnmounted(() => window.removeEventListener("hashchange", focusOnArrival));
</script>

<template>
  <section
    v-if="showreel"
    id="showreel"
    class="home-showreel"
    aria-labelledby="showreel-title"
  >
    <h2 id="showreel-title" class="sr-only">Showreel</h2>
    <figure>
      <div class="home-showreel-stage">
        <!-- No autoplay, and nothing downloads until someone presses play. -->
        <video
          ref="player"
          :src="withBase(src)"
          :poster="withBase(showreel.poster)"
          width="1920"
          height="1080"
          :controls="!hydrated || started"
          playsinline
          preload="none"
          :aria-label="`mise showreel, ${length}: dev tools, versions, environments, tasks, dotfiles, and a new machine, in terminal output recorded from real runs. Chapters are listed below.`"
          @play="started = true"
        >
          <!-- Generated from the reel's acts; see showreel/timeline.ts. -->
          <track
            kind="chapters"
            srclang="en"
            label="Chapters"
            :src="withBase(showreel.track)"
            default
          />
        </video>
        <!-- The whole frame is the target; the glyph sits in its middle. -->
        <button
          v-if="hydrated && !started"
          ref="button"
          type="button"
          class="home-showreel-play"
          :aria-label="`Play the mise showreel (${length})`"
          @click="play"
        >
          <svg viewBox="0 0 64 64" aria-hidden="true">
            <circle cx="32" cy="32" r="30" />
            <path d="M26 20.5v23L44.5 32z" />
          </svg>
        </button>
      </div>
      <ol class="sr-only" aria-label="Showreel chapters">
        <li v-for="c in chapters" :key="c.id">
          {{ c.label }}, at {{ c.at }}{{ c.text ? `: ${c.text}` : "." }}
        </li>
      </ol>
      <figcaption>
        A {{ runtime }} tour of mise, from a first <code>mise use</code> to
        setting up a new machine. Every terminal line was recorded in a real
        shell with a released mise, and the captions are on screen, so it works
        with the sound off.
        <a :href="withBase('/getting-started')"
          >Get started <span aria-hidden="true">→</span></a
        >
      </figcaption>
    </figure>
  </section>
</template>

<style scoped>
/* Under the hero's footnote strip, as wide as the landing page below it
   (.landing-page and .hero-footnote: 1160 px, with 24 px gutters on narrower
   screens). "What mise manages" brings its own top margin. */
.home-showreel {
  max-width: 1208px;
  margin: 0 auto;
  padding: 48px 24px 0;
}
figure {
  margin: 0;
}
/* Framed like the hero's workbench card: its border, radius and offset
   block. The reel is one dark stage in both site themes, so the element's
   own background is the stage's colour (bible.ts PALETTE.bg): no white or
   page-coloured box shows before the poster paints. */
video {
  display: block;
  width: 100%;
  height: auto;
  aspect-ratio: 16 / 9;
  background: #171417;
  border: 1px solid var(--vp-c-border);
  border-radius: 10px;
  box-shadow:
    0 20px 48px -28px #31152266,
    6px 6px 0 var(--vp-c-bg-soft);
}
/* On the dark page the stage sits one step below the page, so the shadow
   goes deeper and the border does the separating. */
.dark video {
  box-shadow:
    0 30px 80px -40px #000,
    6px 6px 0 var(--vp-c-bg-soft);
}
video:focus-visible,
.home-showreel-play:focus-visible {
  outline: 2px solid var(--vp-c-brand-1);
  outline-offset: 4px;
}
.home-showreel-stage {
  position: relative;
}
/* Covers the video exactly, so a click anywhere on the poster plays it. */
.home-showreel-play {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  border-radius: 10px;
  cursor: pointer;
}
/* Drawn in the reel's own colours (night, paper, and the tools pink as the
   site's dark brand), since it sits on the stage in both site themes. 10% of
   the frame's width (44 to 96 px) keeps it in the frame's middle, clear of
   the caption band below. */
.home-showreel-play svg {
  width: clamp(44px, 10%, 96px);
  height: auto;
  filter: drop-shadow(0 6px 18px rgb(0 0 0 / 45%));
  transition: transform 0.15s ease;
}
.home-showreel-play circle {
  fill: rgb(17 14 17 / 62%);
  stroke: #f4eee3;
  stroke-width: 2.5;
  transition:
    fill 0.15s ease,
    stroke 0.15s ease;
}
.home-showreel-play path {
  fill: #f4eee3;
}
.home-showreel-play:hover svg {
  transform: scale(1.06);
}
.home-showreel-play:hover circle,
.home-showreel-play:focus-visible circle {
  fill: rgb(17 14 17 / 82%);
  stroke: #ed9fbc;
}
@media (prefers-reduced-motion: reduce) {
  .home-showreel-play svg,
  .home-showreel-play circle {
    transition: none;
  }
  .home-showreel-play:hover svg {
    transform: none;
  }
}
figcaption {
  max-width: 72ch;
  margin-top: 16px;
  color: var(--vp-c-text-2);
  font-size: 0.875rem;
  line-height: 1.6;
}
/* As the hero's lede sets its code. */
figcaption code {
  font-size: 0.9em;
  color: var(--vp-c-text-1);
}
figcaption a {
  color: var(--vp-c-brand-1);
  font-weight: 500;
  white-space: nowrap;
}
figcaption a:hover {
  text-decoration: underline;
  text-underline-offset: 3px;
}
.sr-only {
  border: 0;
  clip: rect(0 0 0 0);
  clip-path: inset(50%);
  height: 1px;
  margin: -1px;
  overflow: hidden;
  padding: 0;
  position: absolute;
  white-space: nowrap;
  width: 1px;
}
@media (max-width: 960px) {
  .home-showreel {
    padding-top: 40px;
  }
}
@media (max-width: 640px) {
  .home-showreel {
    padding-top: 32px;
  }
  video {
    border-radius: 8px;
    box-shadow:
      0 16px 36px -24px #31152266,
      4px 4px 0 var(--vp-c-bg-soft);
  }
  .home-showreel-play {
    border-radius: 8px;
  }
}
</style>
