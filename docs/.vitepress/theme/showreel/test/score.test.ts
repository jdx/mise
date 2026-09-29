// from jdx/hk@37937824 docs/.vitepress/theme/showreel/test/score.test.ts
// The score, run on a recording fake AudioContext (mock-audio.ts): what it
// schedules, and when.
//
// Adapted for mise: hk's generic checks; the cue API (score/cues.ts),
// which a scene's `cues` export drives and every sync point answers on its
// time; the one key change; the valley and the breath; the handoff, where
// the morph's held B♭ minor sounds whole until the recording enters and is
// gone by SCORE_END; and no click track anywhere.

import assert from "node:assert/strict";
import { test } from "node:test";
import { handoff, playScore } from "../audio";
import type { ReelFacts } from "../bible";
import { arc, PARTS } from "../score";
import { CHORUS_AT } from "../score/clone";
import { CH } from "../score/harmony";
import {
  type CuedId,
  LISTEN,
  resolveCues,
  type SceneCues,
} from "../score/cues";
import { withSceneModules } from "../score/listen";
import { hz, LATENCY, Mix, shared } from "../score/mix";
import { DRUMS_OUT, HELD, HELD_CHORD } from "../score/morph";
import { SCENE_MODULES } from "../score/scenes";
import { SCORE_END, SONG } from "../score/song";
import { BEAT, DURATION, SECTIONS, type SectionId, sec } from "../timeline";
import { MockContext } from "./mock-audio";

/** The context time reel time 0 plays at, as the MP4 renderer schedules it. */
const WHEN = 0.2;

/** Facts the picture could be drawn with. */
const VARIANTS: [string, ReelFacts | null][] = [
  ["no facts", null],
  ["empty facts", {}],
];

function render(from = 0, facts: ReelFacts | null = null): MockContext {
  const ac = new MockContext();
  playScore(ac.context, ac.destination as AudioNode, from, WHEN, facts);
  return ac;
}

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

/** Each source's start and stop, in reel seconds, for a score started at `from`. */
function spans(
  ac: MockContext,
  from = 0,
): { target: string; start: number; stop: number }[] {
  const stops = new Map<string, number>();
  for (const c of ac.calls)
    if (c.method === "stop") stops.set(c.target, c.args[0] as number);
  return ac.starts().map(({ target, t }) => ({
    target,
    start: t - WHEN + from,
    stop: (stops.get(target) ?? Infinity) - WHEN + from,
  }));
}

/** Reel times sources start at: LATENCY early, half a sample before their frame. */
const starts = (ac: MockContext): number[] =>
  spans(ac).map((s) => s.start + LATENCY);

/** Oscillators started, with the pitch each was first given. */
function oscillators(ac: MockContext): { t: number; f: number }[] {
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
      f: pitch.get(target) ?? NaN,
    }));
}

test("the score starts every source before the recording takes over, and sounds in every other section", () => {
  for (const [what, facts] of VARIANTS) {
    const all = spans(render(0, facts));
    for (const { target, start } of all) {
      // Sources start a few milliseconds early, ahead of the master's compressor lookahead.
      assert.ok(
        start >= -LATENCY - 0.001 && start < SONG.segments[0].at,
        `${what}: ${target} starts at ${start}`,
      );
    }
    for (const { id } of SECTIONS) {
      if (id === "end") continue;
      const s = sec(id);
      assert.ok(
        all.some(
          ({ start }) => start >= s.start - 0.01 && start < s.end - 0.01,
        ),
        `${what}: nothing sounds in ${id}`,
      );
    }
  }
});

test("there is no click track: the band does not strike a tick on every beat", () => {
  // A tick on every beat of every section would be a metronome, not a
  // score: the grooves leave the valley and the held bar alone.
  const at = starts(render(0));
  const quiet = [sec("breath").beat(1), sec("breath").beat(3), HELD + BEAT];
  for (const t of quiet)
    assert.ok(
      !at.some((s) => Math.abs(s - t) < 0.002),
      `something strikes on the beat at ${t}`,
    );
});

/**
 * The sync points of plan v3 §5 and ART.md §13: cues whose sound starts on
 * the cue itself (or, for sounds that wind up, within 60 ms before it).
 */
const ON_CUE: { [S in CuedId]?: readonly string[] } = {
  open: ["resolve", "name"],
  pitch: ["tables", "cd", "ci", "lands"],
  tools: ["print", "tear", "lift"],
  use: ["notFound", "slam", "seat", "clear", "ok"],
  registry: ["lit"],
  backends: ["npm", "seat", "clear", "verdict", "chips", "gh", "backends"],
  versions: ["print", "tear", "lift"],
  switch: ["cdDashboard", "dashboard", "cdApi", "api"],
  packslip: ["light", "slip", "tab", "flip", "junit", "linked", "link"],
  env: ["print", "tear", "lift"],
  vars: ["cdOut", "empty", "seat", "cdIn", "port"],
  redact: ["zoom", "rewrite", "cards", "redacted", "stamp", "fold"],
  tasks: ["print", "tear", "lift"],
  depends: ["build", "lint", "test", "ci"],
  skip: ["seat", "skipped", "build", "test", "lint", "ci", "stamp"],
  args: ["usage", "help", "error", "tie"],
  daemons: ["cd", "started", "tie"],
  dotfiles: ["print", "tear", "lift"],
  track: ["checkpoints", "rollback", "seat"],
  machines: ["print", "tear", "lift"],
  lock: ["ledger", "thread", "dock", "run"],
  new: ["print", "tear", "lift"],
  bootstrap: ["card", "wrote", "watcher"],
  breath: ["clear"],
  clone: ["slam", "test", "lint", "build", "ci", "ready", "fold", "home"],
  morph: ["lift", "lands", "drop", "land"],
};

test("every sync point sounds on its cue", () => {
  const at = starts(render(0));
  const missing: string[] = [];
  for (const [id, names] of Object.entries(ON_CUE) as [CuedId, string[]][]) {
    const c = resolveCues(sec(id) as never, null, SCENE_MODULES) as {
      all(n: string): number[];
    };
    for (const name of names) {
      for (const t of c.all(name))
        if (!at.some((s) => s >= t - 0.061 && s <= t + 0.002))
          missing.push(`${id}.${name} at ${t.toFixed(3)}`);
    }
  }
  assert.deepEqual(missing, []);
});

test("a scene's cues export moves its sound, and the fallback fills in the rest", () => {
  const use = sec("use");
  const heard = (mods: object | null) =>
    withSceneModules(mods ? { use: mods } : {}, () =>
      starts(renderLayer("use", "cues")),
    );
  const before = heard(null);
  const clear = use.beat(LISTEN.use.clear);
  assert.ok(before.some((t) => Math.abs(t - clear) < 0.002));
  // The scene says ⌃L comes a beat later; nothing else moves.
  const after = heard({ cues: { clear: LISTEN.use.clear + 1 } });
  assert.ok(!after.some((t) => Math.abs(t - clear) < 0.002));
  assert.ok(
    after.some((t) => Math.abs(t - use.beat(LISTEN.use.clear + 1)) < 0.002),
  );
  assert.ok(after.some((t) => Math.abs(t - use.beat(LISTEN.use.seat)) < 0.002));
  // A function export is given the facts; null drops the event.
  const seen: unknown[] = [];
  const dropped = heard({
    cues: (facts: unknown) => {
      seen.push(facts);
      return { ok: null };
    },
  });
  assert.equal(seen[0], null);
  const ok = use.beat(LISTEN.use.ok);
  assert.ok(!dropped.some((t) => Math.abs(t - ok) < 0.002));
  const c = resolveCues(use as never, null, {
    use: { cues: { ok: [3, 4] } },
  }) as {
    all(n: string): number[];
    source: Record<string, string>;
  };
  assert.deepEqual(c.all("ok"), [use.beat(3), use.beat(4)]);
  assert.equal(c.source.ok, "scene");
  assert.equal(c.source.clear, "fallback");
});

test("the cue API's types let a scene export only the cues its section listens for", () => {
  const typed: SceneCues<"use"> = { clear: 8, seat: [6], ok: null };
  const fromFacts: SceneCues<"switch"> = (facts) =>
    facts ? { cdDashboard: 1.5 } : {};
  // @ts-expect-error: use listens for "clear", not "clr".
  const misspelt: SceneCues<"use"> = { clr: 8 };
  // @ts-expect-error: the end card listens for nothing; the recording keys it.
  const unheard: SceneCues<"end"> = {};
  assert.deepEqual([typed, misspelt, unheard].length, 3);
  assert.equal(typeof fromFacts, "function");
});

test("every cue a scene exports is one the score listens for, inside its section", () => {
  const bad: string[] = [];
  for (const { id } of SECTIONS) {
    const mod = SCENE_MODULES[id] as { cues?: unknown };
    if (!("cues" in mod)) continue;
    const listened = (LISTEN as Record<string, Record<string, unknown>>)[id];
    for (const facts of [null, {}] as (ReelFacts | null)[]) {
      const table = (
        typeof mod.cues === "function" ? mod.cues(facts) : mod.cues
      ) as Record<string, unknown>;
      for (const [name, v] of Object.entries(table)) {
        if (!listened || !(name in listened)) {
          bad.push(
            `${id} exports "${name}", which the score does not listen for`,
          );
          continue;
        }
        const list =
          v === null ? [] : typeof v === "number" ? [v] : (v as number[]);
        for (const b of list)
          if (!(b >= -1 && b <= sec(id).beats + 1))
            bad.push(`${id}.${name} = ${b} is outside the section`);
      }
    }
  }
  assert.deepEqual(bad, []);
});

test("dashboard's cd moves the band to D♭, and api's brings it home", () => {
  const sw = sec("switch");
  const c = resolveCues(sw as never, null, SCENE_MODULES) as {
    at(n: string): number;
  };
  const dash = c.at("cdDashboard");
  const api = c.at("cdApi");
  const near = (f: number, g: number) => Math.abs(f / g - 1) < 0.004;
  // The bass run is one voice: its pitch moves by setValueAtTime.
  const pitches = renderLayer("switch", "bass")
    .calls.filter(
      (x) => x.target.endsWith(".frequency") && x.method === "setValueAtTime",
    )
    .map((x) => ({
      f: x.args[0] as number,
      t: (x.args[1] as number) - WHEN + LATENCY,
    }));
  const between = pitches.filter((p) => p.t > dash + 0.01 && p.t < api - 0.01);
  assert.ok(
    between.some((p) => near(p.f, hz(CH.Db.root))),
    "no D♭ in the bass",
  );
  assert.ok(
    !between.some((p) => near(p.f, hz(CH.Fm.root))),
    "F in the bass in D♭",
  );
  const after = pitches.filter((p) => p.t > api + 0.01 && p.t < sw.end);
  assert.ok(
    after.some((p) => near(p.f, hz(CH.Fm.root))),
    "the bass is not home",
  );
});

test("the valley and the breath: no drums, and the breath near silent", () => {
  for (const id of ["bootstrap", "breath"] as const) {
    assert.equal(spans(renderLayer(id, "drums")).length, 0, `${id} has drums`);
    assert.ok((PARTS[id].level ?? 1) <= 0.7, `${id} is not a valley`);
  }
  assert.equal(spans(renderLayer("breath", "bass")).length, 0);
  assert.ok((PARTS.breath.level ?? 1) < (PARTS.bootstrap.level ?? 1));
  // The valley (the new machine's ticket, bootstrap and the breath) is the
  // reel's lowest: every other section's fader is above it.
  const valley = ["new", "bootstrap", "breath", "end"];
  for (const { id } of SECTIONS)
    if (!valley.includes(id))
      assert.ok(
        (PARTS[id].level ?? 1) > (PARTS.bootstrap.level ?? 1),
        `${id} sits in the valley`,
      );
});

test("the morph's drums stop on its second bar, and from its beat 6 only the held B♭ minor and its air sound", () => {
  const morph = sec("morph");
  assert.equal(morph.beat(DRUMS_OUT), morph.bar(1));
  assert.equal(HELD, morph.beat(6));
  for (const t of starts(renderLayer("morph", "drums")))
    assert.ok(t < morph.beat(DRUMS_OUT), `a drum at ${t}`);
  // Every section's every layer: nothing starts in the held bar but its
  // chord and the air over it (the morph's pads).
  const late: string[] = [];
  const pitches = new Set<number>();
  for (const { id } of SECTIONS)
    for (const layer of ["cues", "drums", "bass", "lead", "pads"] as const) {
      const ac = renderLayer(id, layer);
      for (const o of oscillators(ac))
        if (o.t >= HELD - 0.001) {
          if (id !== "morph" || layer !== "pads")
            late.push(`${id}.${layer} at ${o.t}`);
          else pitches.add(Math.round(12 * Math.log2(o.f / 440) + 69));
        }
      if (!(id === "morph" && layer === "pads"))
        for (const t of starts(ac))
          if (t >= HELD - 0.001) late.push(`${id}.${layer} at ${t}`);
    }
  assert.deepEqual(late, []);
  // The chord's notes, each on the drawbar wave and an octave down.
  for (const n of HELD_CHORD)
    assert.ok(pitches.has(n), `no ${n} in the held chord`);
});

test("the handoff holds the chord whole until the recording enters, then falls to silence by SCORE_END", () => {
  const [line] = SONG.segments;
  const pts = handoff();
  assert.deepEqual(pts[0], [line.at, 1]);
  assert.equal(pts[pts.length - 1][0], SCORE_END);
  assert.equal(pts[pts.length - 1][1], 0);
  assert.ok(Math.abs(SCORE_END - line.at - 0.05) < 1e-9);
  // Equal power with the recording's linear fade-in: the two together keep
  // the chord's power within 1 dB (the old tail's 8 dB dip at 3:13.05).
  for (let t = line.at; t <= line.at + line.fadeIn; t += 0.001) {
    let g = 1;
    for (let i = 1; i < pts.length; i++)
      if (t <= pts[i][0]) {
        const [t0, v0] = pts[i - 1];
        const [t1, v1] = pts[i];
        g = v0 + ((v1 - v0) * (t - t0)) / (t1 - t0);
        break;
      }
    const r = Math.min(1, (t - line.at) / line.fadeIn);
    const p = g * g + r * r;
    assert.ok(p > 0.79 && p < 1.26, `the handoff's power is ${p} at ${t}`);
  }
});

test("the score is silent from SCORE_END: the end card is the recording's", () => {
  assert.ok(SCORE_END > SONG.segments[0].at && SCORE_END < sec("end").start);
  // The recording's "mise" is the end card's downbeat, and its button follows the line.
  const [line, button] = SONG.segments;
  assert.ok(Math.abs(line.at + (72.3 - line.from) - sec("end").start) < 1e-9);
  assert.ok(button.at > line.at + (line.to - line.from));
  assert.ok(button.at + (button.to - button.from) < DURATION);
  for (const { target, stop } of spans(render(0)))
    assert.ok(stop <= SCORE_END + 0.5, `${target} rings until ${stop}`);
});

test("every automation curve runs forward in time, and every exponential ramp between positive values", () => {
  // Web Audio sorts a param's events by time, so a curve whose points run
  // backwards (an envelope's peak written after its end) plays as a
  // different curve, often silence, and an exponential ramp to or from 0
  // jumps instead of ramping. A value curve owns its whole span: Chromium
  // rejects any event that starts inside it.
  const TIMED = [
    "setValueAtTime",
    "linearRampToValueAtTime",
    "exponentialRampToValueAtTime",
    "setTargetAtTime",
    "setValueCurveAtTime",
  ];
  const reel = (t: number) => (t - WHEN + LATENCY).toFixed(4);
  const bad: string[] = [];
  for (const [what, facts] of VARIANTS) {
    // Per param: when its last event ends (a curve's end, any other event's
    // time), and the value it leaves (NaN after a curve: the mock keeps only
    // a curve's length and sum).
    const last = new Map<string, number>();
    const value = new Map<string, number>();
    for (const c of render(0, facts).calls) {
      if (c.method === "value=") value.set(c.target, c.args[0] as number);
      if (c.method === "cancelScheduledValues") last.delete(c.target);
      if (!TIMED.includes(c.method)) continue;
      const [v, t, span] = c.args as [number, number, number];
      if (!Number.isFinite(v) && c.method !== "setValueCurveAtTime")
        bad.push(`${what}: ${c.target} ${c.method}(${v})`);
      const curve = c.method === "setValueCurveAtTime";
      const before = last.get(c.target) ?? -Infinity;
      if (t < before - 1e-9)
        bad.push(
          `${what}: ${c.target} ${c.method}(${curve ? "curve" : v}) at ${reel(t)} starts before ${reel(before)}, where its last event ends`,
        );
      const from = value.get(c.target) ?? 0;
      if (c.method === "exponentialRampToValueAtTime" && !(v > 0 && from > 0))
        bad.push(
          `${what}: ${c.target} ramps exponentially from ${from} to ${v} at ${reel(t)}`,
        );
      last.set(c.target, curve ? t + span : t);
      value.set(c.target, curve ? NaN : v);
    }
  }
  assert.deepEqual(bad, []);
});

test("the score schedules the same calls every time, from any start", () => {
  for (const from of [0, 4.2, 11, 57.3, 128.1, 190.3]) {
    assert.deepEqual(render(from).calls, render(from).calls, `from ${from}`);
  }
});

test("a start mid-reel plays the same sounds, on the same samples, as playback from the top", () => {
  const onSample = (t: number) => Math.round(t * 48000 + 0.5);
  const whole = spans(render(0)).map((s) => s.start);
  for (const from of [4.2, 23.9, 50.3, 142.5, 176.1, 193.1]) {
    // Sustained sounds already under way enter mid-sound at `from`; everything after it matches exactly.
    const after = (list: number[]) =>
      list
        .filter((t) => t >= from + 0.1)
        .map(onSample)
        .sort((a, b) => a - b);
    assert.deepEqual(
      after(spans(render(from), from).map((s) => s.start)),
      after(whole),
      `from ${from}`,
    );
  }
});

test("the climax's last bar is C whole, under the reed's E, as the song has it", () => {
  const clone = sec("clone");
  // The bass run is one voice: its pitch moves by setValueAtTime.
  const notes = renderLayer("clone", "bass")
    .calls.filter(
      (x) => x.target.endsWith(".frequency") && x.method === "setValueAtTime",
    )
    .map((x) => ({
      f: x.args[0] as number,
      t: (x.args[1] as number) - WHEN + LATENCY,
    }))
    // Chorus bar 8: its last two beats (score/clone.ts CHORUS_AT).
    .filter(
      (p) => p.t > clone.beat(CHORUS_AT + 14) - 0.01 && p.t < clone.end - 0.01,
    );
  assert.ok(notes.length > 0, "no bass in bar 8");
  // Each note's oscillators: the root, and a triangle an octave up.
  const octaves = (f: number, g: number) => {
    const k = Math.log2(f / g);
    return Math.abs(k - Math.round(k)) < 0.006;
  };
  const cTones = [CH.C.root, CH.C.fifth].map(hz);
  for (const { f, t } of notes)
    assert.ok(
      cTones.some((g) => octaves(f, g)),
      `the bass plays ${f.toFixed(1)} Hz at ${t.toFixed(3)}, not C's root or fifth`,
    );
});

test("in Acts V and VI no chop falls in the quarter beat before a click, so the click speaks alone", () => {
  const crowded: string[] = [];
  const clicks: [SectionId, string[]][] = [
    ["track", ["checkpoints", "seat"]],
    ["lock", ["thread", "dock", "run"]],
  ];
  for (const [id, names] of clicks) {
    const s = sec(id);
    const c = resolveCues(s as never, null, SCENE_MODULES) as {
      all(n: string): number[];
    };
    // A chop is the band's only voice that starts pitched above 380 Hz on an "and".
    const chops = oscillators(renderLayer(id, "pads"))
      .filter(({ t, f }) => {
        const b = (t - s.start) / BEAT;
        return f > 380 && Math.abs(b - Math.round(b - 0.5) - 0.5) < 0.01;
      })
      .map(({ t }) => t);
    for (const name of names)
      for (const t of c.all(name))
        for (const k of chops)
          if (k > t - 0.3 * BEAT + 0.002 && k <= t + 0.002)
            crowded.push(
              `${id}.${name} at ${t.toFixed(3)}: a chop at ${k.toFixed(3)}`,
            );
  }
  assert.deepEqual(crowded, []);
});

test("the groove's fader holds each section's level and moves only over the half beat before a bar line", () => {
  const pts = arc();
  // The fader's value at reel time t: it only ever moves in straight lines.
  const at = (t: number): number => {
    let v = pts[0][1];
    for (let i = 1; i < pts.length; i++) {
      const [t0, v0] = pts[i - 1];
      const [t1, v1] = pts[i];
      if (t >= t1) v = v1;
      else if (t > t0) return v0 + ((v1 - v0) * (t - t0)) / (t1 - t0);
    }
    return v;
  };
  for (const { id } of SECTIONS) {
    const s = sec(id);
    const level = PARTS[id].level ?? 1;
    for (const t of [
      s.start,
      (s.start + s.end) / 2,
      s.end - BEAT / 2 - 0.001,
    ]) {
      assert.ok(
        Math.abs(at(t) - level) < 1e-9,
        `${id}: the fader is at ${at(t)} at ${t}, not its level ${level}`,
      );
    }
  }
});
