// The hook the score quotes (score/motif.ts), pinned to the transcription
// of jdx's chorus (plan v3 §5), and where the score plays it: M1 whole at
// the open and the climax, a fragment on each act's ticket landing F on the
// tear, the "C-I both" cell at the packslip's skills link, chorus bars 7 to
// 12 into and through the morph, and M2 never: the recording sings it.

import assert from "node:assert/strict";
import { test } from "node:test";
import type { ReelFacts } from "../bible";
import { TICKET } from "../kit/style";
import { PARTS } from "../score";
import { CHORUS_AT } from "../score/clone";
import { LISTEN } from "../score/cues";
import { listen } from "../score/listen";
import { boardOf } from "../storyboard";
import { hz, LATENCY, Mix, shared, X } from "../score/mix";
import {
  BOTH,
  CHORUS,
  HEAD,
  type HookNote,
  LAPTOP,
  M1,
  M2,
  onReel,
  SIGH,
  STARTS,
} from "../score/motif";
import { SECTIONS, type SectionId, sec } from "../timeline";
import { MockContext } from "./mock-audio";

const WHEN = 0.2;

type Layer = "cues" | "drums" | "bass" | "lead" | "pads";

/** One layer of one section's part, alone, on a mix of its own. */
function renderLayer(
  id: SectionId,
  layer: Layer,
  facts: ReelFacts | null = null,
): MockContext {
  const ac = new MockContext();
  const ctx = ac.context;
  const bus = () => ctx.createGain();
  const m = new Mix(
    ctx,
    shared(ctx),
    0,
    WHEN,
    { sfx: bus(), drums: bus(), music: bus() },
    bus(),
  );
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

test("the open plays M1 whole on the reed, its F on the chef's resolve", () => {
  // The resolve as the score hears it: the scene's cue, else the fallback,
  // which is the event plan's (the end of the chef's lift, sections.json).
  const resolve = listen("open", sec("open"), null).at("resolve")!;
  const lift = boardOf("open").plan?.find((e) => e.name === "lift");
  assert.ok(lift && Math.abs(lift.end - LISTEN.open.resolve) <= 1 / 16);
  const heard = tones(renderLayer("open", "lead"));
  for (const [a, , n] of onReel(M1, resolve))
    assert.ok(sounds(heard, a, n), `no ${n} at ${a}`);
});

test("the climax sings M1 over chorus bars 1–2, rests through 3–6, and returns for 7–8 into the morph", () => {
  const clone = sec("clone");
  // Chorus bar 1 falls 16 beats before the morph (score/clone.ts CHORUS_AT).
  const e0 = clone.beat(CHORUS_AT);
  assert.equal(CHORUS_AT, clone.beats - 16);
  const heard = tones(renderLayer("clone", "lead"));
  for (const [a, , n] of onReel(M1, e0))
    assert.ok(sounds(heard, a, n), `no ${n} at ${a}`);
  // Bars 3 to 6: the lead rests.
  const bar = (k: number) => e0 + 8 * (k - 1) * X;
  assert.ok(!heard.some((h) => h.t >= bar(3) - 0.001 && h.t < bar(7) - 0.001));
  for (const [a, , n] of onReel(STARTS, e0))
    assert.ok(sounds(heard, a, n), `no ${n} at ${a}`);
  // Its pickup C4 two sixteenths before the morph lands the next note on its downbeat.
  assert.ok(sounds(heard, sec("morph").start - 2 * X, 60));
  assert.ok(Math.abs(e0 + 64 * X - sec("morph").start) < 1e-9);
});

test("the morph sings chorus bars 9–12: F on its downbeat, the G G G F sigh from beat 1.5, D♭ recited from beat 4", () => {
  const heard = tones(renderLayer("morph", "lead"));
  const morph = sec("morph");
  const at = (t: number, n: number) =>
    assert.ok(sounds(heard, t, n), `no ${n} at ${t}`);
  at(morph.start, 65);
  at(morph.beat(1.5), 67);
  at(morph.beat(1.75), 67);
  at(morph.beat(2), 67);
  at(morph.beat(2.25), 65);
  at(morph.beat(4), 61);
  for (const [a, , n] of onReel(LAPTOP, sec("morph").start - 64 * X))
    assert.ok(sounds(heard, a, n), `no ${n} at ${a}`);
  // The recitation is done by beat 6.5, before the held bar's handoff.
  const last = onReel(LAPTOP, sec("morph").start - 64 * X).at(-1);
  assert.ok(last && last[1] <= morph.beat(6.5) + 1e-9);
});

test("the packslip's skills link rings the C-I both cell, G G G F, on the celesta as the link lights", () => {
  const link = listen("packslip", sec("packslip"), null).at("link")!;
  const heard = tones(renderLayer("packslip", "cues"));
  [79, 79, 79, 77].forEach((n, i) =>
    assert.ok(sounds(heard, link + X * i, n), `no ${n} at ${link + X * i}`),
  );
});

test("each ticket plays a fragment of M1 on its act's voice, alternating the head and the sigh, its F on the tear", () => {
  const tickets = SECTIONS.filter((s) => "ticket" in s && s.ticket);
  assert.equal(tickets.length, 7);
  tickets.forEach(({ id }, i) => {
    const s = sec(id);
    const tear = s.beat(TICKET.cue.tear);
    const heard = tones(renderLayer(id, "lead"));
    const f = heard.filter(
      (h) =>
        Math.abs(h.t - tear) < 0.002 &&
        Math.abs(Math.log2(h.f / hz(65)) % 1) < 0.025,
    );
    assert.ok(f.length > 0, `${id}: no F on the tear at ${tear}`);
    // Its first note: C before the head's F, A♭ before the sigh's.
    const first = Math.min(...heard.map((h) => h.t));
    const head = i % 2 === 0;
    const lead = heard.filter((h) => Math.abs(h.t - first) < 0.002);
    const pc = (h: { f: number }) =>
      ((Math.round(12 * Math.log2(h.f / 440)) % 12) + 12) % 12;
    assert.ok(
      lead.some((h) => pc(h) === (head ? 3 : 11)),
      `${id}: it does not open on ${head ? "C" : "A♭"}`,
    );
    assert.ok(
      Math.abs(tear - first - (head ? 2 : 3) * X) < 0.002,
      `${id}: the fragment is not the ${head ? "head" : "sigh"}`,
    );
  });
});

test("M2 is never synthesized: no quote reaches the cadence, and no lead sings below C4", () => {
  const quoted = [M1, HEAD, SIGH, BOTH, STARTS, LAPTOP].flat();
  const lastQuoted = Math.max(...quoted.map(([e, len]) => e + len));
  assert.ok(lastQuoted <= M2[0][0], "a quote runs into M2");
  for (const { id } of SECTIONS) {
    const low = tones(renderLayer(id, "lead")).filter(
      (h) => h.f < hz(60) * 0.99 && h.f > 150,
    );
    assert.deepEqual(low, [], `${id}'s lead sings below C4`);
  }
});
