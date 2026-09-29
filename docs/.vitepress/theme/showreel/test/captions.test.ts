// from jdx/hk@37937824 docs/.vitepress/theme/showreel/test/captions.test.ts
// The captions' reading rules (type.ts): every must-read line holds long
// enough to read, two-line captions hold all their words, and captions never
// share the screen. Checked for every scene's captions, with no facts and
// with an empty set of them.
//
// Adapted for mise: 60 BPM, the frame grids of the 30 fps animatic as well
// as 60 and 120, word counts taken from plan v2's captions, and the retime's
// reading rule (STORYBOARD.md "Pacing" rule 5: up max(words/3 + 1 s, 3 s)
// from the first word, words/3 s once whole, and at most one caption change
// in 4 s). The scenes' captions are the storyboard's (storyboard.ts);
// test/storyboard.test.ts checks their text and anchors.

import assert from "node:assert/strict";
import { test } from "node:test";
import { BEAT, PALETTE, type ReelFacts, type SectionId, sec } from "../bible";
import { PACE } from "../kit/style";
import { scenes } from "../scenes";
import { boardOf } from "../storyboard";
import {
  type Caption,
  CAPTION_Y,
  CODE,
  entrance,
  PAPER,
  plain,
  readingTime,
  settledTime,
  timeCaptions,
  WIPE,
  WORD,
  wordCount,
} from "../type";

const VARIANTS: [string, ReelFacts | null][] = [
  ["no facts", null],
  ["empty facts", {}],
];

/** The frame rates the reel is rendered at. */
const FPS = [30, 60, 120];

/** Every scene's captions under each set of facts, in timeline order. */
function everyCaption(): {
  name: string;
  id: SectionId;
  caps: readonly Caption[];
}[] {
  return VARIANTS.flatMap(([what, facts]) =>
    scenes.map((s) => ({
      name: `${s.id} (${what})`,
      id: s.id,
      caps: s.captions?.(facts) ?? [],
    })),
  );
}

/** The first frame at or after `t` on a `fps` grid, as a frame number. */
const frameAt = (t: number, fps: number): number => Math.ceil(t * fps - 1e-6);

/**
 * Seconds a line is fully in on a `fps` grid: from the first frame that
 * shows its last word landed to the first frame of its wipe.
 */
function heldOnFrames(landed: number, out: number, fps: number): number {
  return (frameAt(out, fps) - frameAt(landed, fps)) / fps;
}

test("captions have one or two lines of at most 36 characters, landing in order", () => {
  for (const { name, id, caps } of everyCaption()) {
    const beats = sec(id).beats;
    for (const c of caps) {
      assert.ok(
        c.lines.length >= 1 && c.lines.length <= 2,
        `${name}: ${c.lines.length} lines`,
      );
      c.lines.forEach((l, i) => {
        assert.ok(
          plain(l.text).length <= 36,
          `${name}: "${plain(l.text)}" is too long`,
        );
        assert.equal(
          (l.text.match(/`/g) ?? []).length % 2,
          0,
          `${name}: unclosed code in "${l.text}"`,
        );
        assert.ok(l.in < c.out, `${name}: "${l.text}" leaves before it lands`);
        if (i > 0)
          assert.ok(
            l.in >= c.lines[i - 1].in,
            `${name}: "${l.text}" lands before the line above it`,
          );
        // The first word starts inside the section.
        assert.ok(
          entrance(l.text, l.in * BEAT) >= -1e-9,
          `${name}: "${l.text}" starts before its section`,
        );
      });
      // The wipe finishes inside the section.
      assert.ok(
        c.out * BEAT + WIPE <= beats * BEAT + 1e-9,
        `${name}: the wipe at b${c.out} runs past the section`,
      );
    }
  }
});

test("every out is on the half-beat grid and every in on the quarter-beat grid", () => {
  for (const { name, caps } of everyCaption()) {
    for (const c of caps) {
      assert.ok(Number.isInteger(c.out * 2), `${name}: out b${c.out}`);
      for (const l of c.lines)
        assert.ok(
          Number.isInteger(l.in * 4),
          `${name}: "${l.text}" lands on b${l.in}`,
        );
    }
  }
});

/** When caption `c`'s first word starts to rise, section beats. */
const startOf = (c: Caption): number =>
  entrance(c.lines[0].text, c.lines[0].in * BEAT) / BEAT;

test("every caption stays up max(words / 3 + 1 s, 3 s) from its first word, on the beat grid and on every frame grid", () => {
  for (const { name, id, caps } of everyCaption()) {
    const s = sec(id);
    for (const c of caps) {
      const words = c.lines.reduce((n, l) => n + wordCount(l.text), 0);
      const need = readingTime(words);
      const first = s.beat(startOf(c));
      const out = s.beat(c.out);
      const text = c.lines.map((l) => plain(l.text)).join(" / ");
      assert.ok(
        out - first >= need - 1e-9,
        `${name}: "${text}" is up ${(out - first).toFixed(3)} s, needs ${need}`,
      );
      for (const fps of FPS) {
        const held = heldOnFrames(first, out, fps);
        assert.ok(
          held >= need - 1e-9,
          `${name} at ${fps} fps: "${text}" is up ${held.toFixed(4)} s, needs ${need}`,
        );
      }
    }
  }
});

test("once whole, a caption holds a third of a second a word, on every frame grid", () => {
  for (const { name, id, caps } of everyCaption()) {
    const s = sec(id);
    for (const c of caps) {
      const words = c.lines.reduce((n, l) => n + wordCount(l.text), 0);
      const need = settledTime(words);
      const whole = s.beat(Math.max(...c.lines.map((l) => l.in)));
      const out = s.beat(c.out);
      const text = c.lines.map((l) => plain(l.text)).join(" / ");
      for (const fps of FPS) {
        const held = heldOnFrames(whole, out, fps);
        assert.ok(
          held >= need - 1e-9,
          `${name} at ${fps} fps: "${text}" holds ${held.toFixed(4)} s whole, needs ${need}`,
        );
      }
    }
  }
});

test("the caption area changes at most once in 4 s: an arrival, or a leave with nothing arriving", () => {
  for (const [what, facts] of VARIANTS) {
    const changes: { t: number; what: string }[] = [];
    for (const scene of scenes) {
      const s = sec(scene.id);
      const caps = scene.captions?.(facts) ?? [];
      caps.forEach((c, i) => {
        const text = c.lines.map((l) => plain(l.text)).join(" / ");
        changes.push({ t: s.beat(startOf(c)), what: `"${text}" arrives` });
        const next = caps[i + 1];
        // A leave that the next caption's arrival replaces is one change.
        if (!next || startOf(next) - c.out >= 0.5)
          changes.push({ t: s.beat(c.out), what: `"${text}" leaves` });
      });
    }
    changes.sort((a, b) => a.t - b.t);
    for (let i = 1; i < changes.length; i++)
      assert.ok(
        changes[i].t - changes[i - 1].t >= PACE.captionEvery - 1e-9,
        `${what}: ${changes[i - 1].what} at ${changes[i - 1].t} s, then ${changes[i].what} at ${changes[i].t} s`,
      );
  }
});

test("captions never overlap: the next lands at least half a beat after the last starts to leave", () => {
  for (const { name, caps } of everyCaption()) {
    const sorted = [...caps].sort((a, b) => a.lines[0].in - b.lines[0].in);
    for (let i = 1; i < sorted.length; i++) {
      const prev = sorted[i - 1];
      const next = sorted[i];
      const lands = Math.min(...next.lines.map((l) => l.in));
      assert.ok(
        lands >= prev.out + 0.5,
        `${name}: a caption lands on b${lands}, the last leaves on b${prev.out}`,
      );
    }
  }
  // On the reel's clock no caption's first word rises before the last
  // caption starts to wipe, so a new line never lands on one still whole.
  for (const [, facts] of VARIANTS) {
    const timed = scenes
      .flatMap((s) => timeCaptions(sec(s.id), s.captions?.(facts) ?? []))
      .sort((a, b) => a.start - b.start);
    for (let i = 1; i < timed.length; i++) {
      const text = timed[i].lines.map((l) => plain(l.text)).join(" / ");
      assert.ok(
        timed[i].start >= timed[i - 1].out - 1e-9,
        `"${text}" starts before the caption above it leaves`,
      );
    }
  }
});

test("words land one per 1/32 note and leave over a sixteenth", () => {
  assert.equal(WORD, BEAT / 8);
  assert.equal(WIPE, BEAT / 4);
  assert.ok(Math.abs(WORD - 0.125) < 1e-12);
  assert.ok(Math.abs(WIPE - 0.25) < 1e-12);
  // Code is in backticks and each run of it is one or more words.
  assert.equal(entrance("No `jq`? `mise use jq` installs it", 1), 1 - 7 * WORD);
});

test("words are counted as captions.py counts them", () => {
  // Plan v2's captions.
  assert.equal(wordCount("One `mise.toml` per project:"), 4);
  assert.equal(wordCount("tools, env vars, and tasks."), 5);
  assert.equal(wordCount("No `jq`? `mise use jq` installs it"), 7);
  assert.equal(wordCount("`mise.lock` records `24.21.0`."), 3);
  assert.equal(wordCount("1,000+ tools by short name,"), 5);
  // Up max(words / 3 + 1 s, 3 s): 4 words, 3 s; 12 words, 5 s (5 beats at 60 BPM).
  assert.equal(readingTime(4), 3);
  assert.equal(readingTime(12), 5);
  assert.equal(readingTime(12) / BEAT, 5);
  assert.equal(settledTime(9), 3);
});

test("a caption's lines sit on the lower baselines and start when their first word rises", () => {
  const s = sec("pitch");
  const [one, two] = timeCaptions(s, [
    { out: 6, lines: [{ in: 2, text: "tools, env vars, and tasks." }] },
    {
      out: 14,
      lines: [
        { in: 9, text: "With `mise activate`, `cd` in," },
        { in: 9.5, text: "and its tools and env vars load." },
      ],
    },
  ]);
  assert.deepEqual(
    one.lines.map((l) => l.y),
    [CAPTION_Y[1]],
  );
  assert.deepEqual(
    two.lines.map((l) => l.y),
    [...CAPTION_Y],
  );
  assert.equal(one.start, entrance("tools, env vars, and tasks.", s.beat(2)));
  assert.equal(one.out, s.beat(6));
  assert.equal(one.end, s.beat(6) + WIPE);
  assert.equal(
    two.start,
    entrance("With `mise activate`, `cd` in,", s.beat(9)),
  );
});

test("the caption colours are the palette's", () => {
  assert.equal(PAPER, PALETTE.paper);
  assert.equal(CODE, PALETTE.paperDim);
});

test("captions hold on: a section's last until two beats before it ends (later only while it is read), and no gap under 4 s is left bare", () => {
  for (const { name, id, caps } of everyCaption()) {
    if (!caps.length) continue;
    const board = boardOf(id).captions;
    const beats = sec(id).beats;
    caps.forEach((c, i) => {
      const until = board[i].until;
      if (until !== undefined) return;
      if (i === caps.length - 1) {
        assert.ok(
          c.out >= beats - 2 && c.out <= beats - 1,
          `${name}: the last caption leaves on b${c.out}, not between b${beats - 2} and b${beats - 1}`,
        );
        return;
      }
      // The next caption's first word starts to rise on its anchor.
      const next = caps[i + 1].lines[0];
      const bare = entrance(next.text, next.in * BEAT) / BEAT - c.out;
      assert.ok(
        bare < 0.5 + 1e-9 || bare >= PACE.captionEvery / BEAT - 1e-9,
        `${name}: ${bare} beats bare before the next caption starts`,
      );
    });
  }
});
