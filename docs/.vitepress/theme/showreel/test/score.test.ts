// from jdx/hk@37937824 docs/.vitepress/theme/showreel/test/score.test.ts
// The score, run on a recording fake AudioContext (mock-audio.ts): what it
// schedules, and when.
//
// Adapted for mise: hk's generic checks; the cue API (score/cues.ts),
// which a scene's `cues` export drives and every sync point answers on its
// time; the end card's few sounds on its own cues (kit/namecard.ts
// END_CUES); the valley and the breath, and the sections' fader the bed
// rides; and no click track anywhere. The score is sound design only: the
// synthesized band is gone (jdx's call), and the music is the bed
// (score/bed.ts, test/bed.test.ts), the one thing on the music bus.

import assert from "node:assert/strict";
import { test } from "node:test";
import { playScore } from "../audio";
import type { ReelFacts } from "../bible";
import { GLINT } from "../kit/chef";
import { END_CUES } from "../kit/namecard";
import { arc, PARTS } from "../score";
import {
  type CuedId,
  type CueName,
  LISTEN,
  resolveCues,
  type SceneCues,
} from "../score/cues";
import { listen, withSceneModules } from "../score/listen";
import { hz, LATENCY, Mix, shared } from "../score/mix";
import { SCENE_MODULES } from "../score/scenes";
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

test("the score starts every source inside the reel, stops it by the reel's end, and sounds in every section", () => {
  for (const [what, facts] of VARIANTS) {
    const all = spans(render(0, facts));
    for (const { target, start, stop } of all) {
      // Sources start a few milliseconds early, ahead of the master's compressor lookahead.
      assert.ok(
        start >= -LATENCY - 0.001 && start < DURATION,
        `${what}: ${target} starts at ${start}`,
      );
      // An offline render ends with the reel, so a source left running
      // would only be cut off there; every one stops on its own.
      assert.ok(
        stop + LATENCY <= DURATION + 1e-6,
        `${what}: ${target} stops at ${stop}`,
      );
    }
    for (const { id } of SECTIONS) {
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

test("there is no click track: nothing strikes the breath's quiet beats", () => {
  // A tick on every beat of every section would be a metronome, not a
  // score: the effects answer the picture's events, and the breath's
  // offbeats have none.
  const at = starts(render(0));
  const quiet = [sec("breath").beat(1), sec("breath").beat(3)];
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
  clone: [
    "slam",
    "tools",
    "test",
    "lint",
    "build",
    "ci",
    "tasks",
    "ready",
    "env",
    "fold",
    "home",
  ],
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

test("the end card's sounds land on its cues, and only there: the bed carries the rest", () => {
  // "mise", "en" and "place" write on, mise.jdx.dev lands with a click and
  // the glint crosses the hat as bells (score/end.ts); the tagline, the
  // install strip and the platform line arrive under the bed alone.
  const end = sec("end");
  const c = END_CUES;
  const at = starts(renderLayer("end", "cues"));
  const near = (t: number) => at.some((s) => s >= t - 0.061 && s <= t + 0.002);
  for (const name of ["mise", "en", "place", "url", "glint"] as const)
    assert.ok(
      near(end.start + c[name]),
      `nothing sounds on the end card's ${name}`,
    );
  const windows: [number, number][] = [
    ...[c.mise, c.en, c.place, c.url].map((t): [number, number] => [t, t]),
    [c.glint, c.glint + GLINT.dur],
  ];
  for (const s of at) {
    const t = s - end.start;
    assert.ok(
      windows.some(([a, b]) => t >= a - 0.061 && t <= b + 0.01),
      `a sound at ${t.toFixed(3)} on the end card is on none of its cues`,
    );
  }
});

test("the score is sound design only: each part is a fader level and a cues layer", () => {
  // The band's drums, bass, lead and pads layers are gone; the bed is the
  // music (score/bed.ts).
  for (const { id } of SECTIONS) {
    const extra = Object.keys(PARTS[id]).filter(
      (k) => k !== "level" && k !== "cues",
    );
    assert.deepEqual(extra, [], `${id}'s part has ${extra.join(", ")}`);
  }
  // Nothing the score composes plays on the music bus: Mix has only the
  // effects bus to mix into, and bed.test checks that, with no bed, the
  // music bus is empty.
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
  // @ts-expect-error: the end card listens for nothing; its cues are its picture's (END_CUES).
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

test("the valley and the breath: the bed's fader is lowest at the breath, near silent", () => {
  // The bed carries its own dynamics (bed.toml plays its breakdown across
  // bootstrap and the breath), so the fader is flat through the valley but
  // for the breath, which takes the whole dip: at least 18 dB under
  // bootstrap's, the bed playing on through it.
  assert.ok((PARTS.breath.level ?? 1) <= 0.7, "the breath is not a valley");
  assert.ok(
    (PARTS.breath.level ?? 1) <=
      (PARTS.bootstrap.level ?? 1) * 10 ** (-18 / 20),
    "the breath is not near silent",
  );
  // The breath is the reel's floor: every other section's fader is above it.
  for (const { id } of SECTIONS)
    if (id !== "breath")
      assert.ok(
        (PARTS[id].level ?? 1) > (PARTS.breath.level ?? 1),
        `${id} sits under the breath`,
      );
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

test("the bed's fader holds each section's level and moves only over the half beat before a bar line", () => {
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

/** Pitched oscillators a layer starts (above 100 Hz: no LFOs), reel time and first pitch. */
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

test("switch: each `cd` runs up the bed's chord, and each project's `node` lands on its root", () => {
  // The bed plays F minor under `cd ../dashboard`, C minor as dashboard's
  // `node --version` prints, C minor again under `cd ../api` and B♭ as
  // api's prints (score/harmony.ts, the bed's chord chart): each project
  // prints in a key of its own, on M1's head, its fifth rising a fourth.
  const c = listen("switch", sec("switch"), null);
  const heard = tones(renderLayer("switch", "cues"));
  const struck = (t: number, n: number) =>
    heard.some(
      (h) => Math.abs(h.t - t) < 0.002 && Math.abs(h.f / hz(n) - 1) < 0.004,
    );
  const missing: string[] = [];
  const want: [CueName<"switch">, number, number[]][] = [
    ["cdDashboard", 0, [77, 80, 84, 89]],
    ["dashboard", -0.2, [79, 84]],
    ["cdApi", 0, [72, 75, 79, 84]],
    ["api", -0.2, [77, 82]],
  ];
  for (const [name, from, notes] of want) {
    const t = c.at(name)!;
    notes.forEach((n, i) => {
      const at = t + from + (name.startsWith("cd") ? 0.1 : 0.2) * i;
      if (!struck(at, n)) missing.push(`${name}: ${n} at ${at.toFixed(3)}`);
    });
  }
  assert.deepEqual(missing, []);
});
