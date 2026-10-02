// The hook the score quotes (score/motif.ts), pinned to the transcription
// of jdx's chorus (plan v3 §5), and the "C-I both" cell on the celesta at
// the packslip's skills link, the one whole phrase of it the sound design
// still plays. Its other quotes are fragments: the name's C D♭ C as it
// writes on, on the open and the end card (props.ts writeName), and M1's
// head on use's "ok" and switch's versions. The synthesized band's quotes
// of it (M1 at the open and the climax, the tickets' fragments, the
// morph's bars 7 to 12) went with the band, and the sung recording with
// them.

import assert from "node:assert/strict";
import { test } from "node:test";
import type { ReelFacts } from "../bible";
import { PARTS } from "../score";
import { listen } from "../score/listen";
import { hz, LATENCY, Mix, shared, X } from "../score/mix";
import {
  BOTH,
  CHORUS,
  HEAD,
  type HookNote,
  M1,
  M2,
  SIGH,
} from "../score/motif";
import { type SectionId, sec } from "../timeline";
import { MockContext } from "./mock-audio";

const WHEN = 0.2;

type Layer = "cues";

/** One layer of one section's part, alone, on a mix of its own. */
function renderLayer(
  id: SectionId,
  layer: Layer,
  facts: ReelFacts | null = null,
): MockContext {
  const ac = new MockContext();
  const ctx = ac.context;
  const bus = () => ctx.createGain();
  const m = new Mix(ctx, shared(ctx), 0, WHEN, bus(), bus());
  PARTS[id][layer]?.(m, sec(id), facts);
  return ac;
}

/** Pitched oscillators started (above 100 Hz: no LFOs), reel time and first pitch. */
function tones(ac: MockContext): { t: number; f: number }[] {
  const pitch = new Map<string, number>();
  for (const c of ac.calls) {
    if (!c.target.endsWith(".frequency")) continue;
    const id = c.target.slice(0, -".frequency".length);
    if (pitch.has(id)) continue;
    if (c.method === "value=" || c.method === "setValueAtTime")
      pitch.set(id, c.args[0] as number);
  }
  return ac
    .starts()
    .filter(({ target }) => target.startsWith("osc#"))
    .map(({ target, t }) => ({
      t: t - WHEN + LATENCY,
      f: pitch.get(target) ?? 0,
    }))
    .filter(({ f }) => f > 100);
}

/** True when `heard` has a tone of MIDI note `n` starting at `t` (a pluck starts a touch sharp). */
const sounds = (heard: { t: number; f: number }[], t: number, n: number) =>
  heard.some(
    (h) => Math.abs(h.t - t) < 0.002 && Math.abs(h.f / hz(n) - 1) < 0.012,
  );

const pitches = (p: readonly HookNote[]) => p.map(([, , n]) => n);
const steps = (p: readonly HookNote[]) =>
  p.slice(1).map(([, , n], i) => n - p[i][2]);

test("M1 and M2 are the transcription's: their pitches and intervals", () => {
  // "In SHORT, it's Nix for PEO-ple WHO have": 5 → 1 5 5 5 ♭3 2 1 5.
  assert.deepEqual(pitches(M1), [60, 65, 60, 60, 60, 68, 67, 65, 60]);
  assert.deepEqual(steps(M1), [5, -5, 0, 0, 8, -1, -2, -5]);
  assert.equal(M1[1][0], 0, "M1's F is on the chorus's downbeat");
  // "pre-CISE and O-pe-RA-tion-al": 4 5 … 5 2 2 1 1, the octave drop.
  assert.deepEqual(pitches(M2), [58, 60, 48, 55, 55, 53, 53]);
  assert.deepEqual(steps(M2), [2, -12, 7, 0, -2, 0]);
  assert.deepEqual(pitches(HEAD), [60, 65]);
  assert.deepEqual(pitches(SIGH), [68, 67, 65]);
  assert.deepEqual(pitches(BOTH), [67, 67, 67, 65]);
  // The chorus runs forward, notes never overlapping.
  for (let i = 1; i < CHORUS.length; i++)
    assert.ok(
      CHORUS[i][0] >= CHORUS[i - 1][0] + CHORUS[i - 1][1],
      `chorus note ${i} overlaps`,
    );
});

test("M2 is never played: no part's cues run its intervals", () => {
  // M2's interval sequence, in any octave, in the tones each section's
  // cues start, in order (plan v3 §5: the song's own ending is not quoted).
  const want = steps(M2);
  const midi = (f: number) => Math.round(69 + 12 * Math.log2(f / 440));
  for (const id of Object.keys(PARTS) as SectionId[]) {
    const ns = tones(renderLayer(id, "cues"))
      .sort((a, b) => a.t - b.t)
      .map(({ f }) => midi(f));
    const run = ns.slice(1).map((n, i) => n - ns[i]);
    for (let i = 0; i + want.length <= run.length; i++)
      assert.notDeepEqual(
        run.slice(i, i + want.length),
        want,
        `${id} plays M2 from its tone ${i}`,
      );
  }
});

test("the packslip's skills link rings the C-I both cell, G G G F, on the celesta as the link lights", () => {
  const link = listen("packslip", sec("packslip"), null).at("link")!;
  const heard = tones(renderLayer("packslip", "cues"));
  [79, 79, 79, 77].forEach((n, i) =>
    assert.ok(sounds(heard, link + X * i, n), `no ${n} at ${link + X * i}`),
  );
});
