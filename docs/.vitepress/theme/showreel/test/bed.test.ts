// The bed (score/bed.ts): the reel's music, one committed Ogg Opus track on
// reel time, and how the score plays it. The file is the one BED pins (its
// sha256), stereo, and as long as the reel, read from its own Ogg pages:
// the last page's granule position less the OpusHead's pre-skip, at Opus's
// 48 kHz. On the fake AudioContext (mock-audio.ts), with a stand-in for the
// decoded buffer: one source, on the music bus and not the reverb, started
// on an exact frame and reading from a whole frame, so a render from any
// start plays bed frame k on the frame a voice at reel time k / 48000 plays
// on; stopped by the reel's end; and none at all without a buffer. The
// master takes the effects, the music and the reverb return, and nothing
// else: the band's drums bus is gone. The music ducks under the effects
// BED.duck as deep as each accent asks.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { playScore } from "../audio";
import { compose } from "../score";
import { BED } from "../score/bed";
import { LATENCY, Mix, shared } from "../score/mix";
import { DURATION } from "../timeline";
import { type Call, MockContext } from "./mock-audio";
import { REPO } from "./repo";

const SR = 48000;
/** The context time reel time 0 plays at, as the MP4 renderer schedules it. */
const WHEN = 0.2;

// The file.

const bytes = (): Buffer => readFileSync(join(REPO, BED.file));

test("the bed is the file BED pins, beside the score, where the render cache hashes it", () => {
  assert.ok(existsSync(join(REPO, BED.file)), `${BED.file} is missing`);
  // xtasks/docs/showreel keys the rendered reel on every file under the
  // showreel's directory but its .md files and its tests.
  assert.match(BED.file, /^docs\/\.vitepress\/theme\/showreel\/score\//);
  assert.doesNotMatch(BED.file, /\.md$/);
  assert.equal(
    createHash("sha256").update(bytes()).digest("hex"),
    BED.sha256,
    `${BED.file} changed: pin its sha256 in score/bed.ts`,
  );
});

interface OggPage {
  flags: number;
  granule: bigint;
  serial: number;
  body: Buffer;
}

/** Every page of an Ogg stream, in order; throws unless they tile the file. */
function oggPages(b: Buffer): OggPage[] {
  const pages: OggPage[] = [];
  let at = 0;
  while (at < b.length) {
    assert.equal(
      b.toString("latin1", at, at + 4),
      "OggS",
      `no Ogg page at byte ${at}`,
    );
    assert.equal(b[at + 4], 0, "Ogg version 0");
    const segments = b[at + 26];
    let len = 0;
    for (let i = 0; i < segments; i++) len += b[at + 27 + i];
    const start = at + 27 + segments;
    assert.ok(start + len <= b.length, `the page at byte ${at} is cut short`);
    pages.push({
      flags: b[at + 5],
      granule: b.readBigInt64LE(at + 6),
      serial: b.readUInt32LE(at + 14),
      body: b.subarray(start, start + len),
    });
    at = start + len;
  }
  return pages;
}

test("the bed is Ogg Opus, stereo, and runs the whole reel, end card included", () => {
  const pages = oggPages(bytes());
  // One logical stream, opened and closed.
  assert.ok(pages[0].flags & 0x02, "the first page begins the stream");
  assert.ok(pages[pages.length - 1].flags & 0x04, "the last page ends it");
  assert.deepEqual(
    [...new Set(pages.map((p) => p.serial))],
    [pages[0].serial],
    "one logical stream",
  );
  // The OpusHead packet (RFC 7845 §5.1).
  const head = pages[0].body;
  assert.equal(head.toString("latin1", 0, 8), "OpusHead");
  assert.equal(head[8] & 0xf0, 0, "OpusHead version 0.x");
  assert.equal(head[9], 2, "stereo");
  const preSkip = head.readUInt16LE(10);
  assert.equal(pages[1].body.toString("latin1", 0, 8), "OpusTags");
  // Granule positions count 48 kHz samples from the start of the decode,
  // pre-skip included; the last page's is the stream's end.
  // The renderer refuses a bed even one frame short of the reel, and the
  // fit tool's verify wants it frame for frame, so this does too.
  const samples = Number(pages[pages.length - 1].granule) - preSkip;
  assert.equal(
    samples,
    Math.round(DURATION * SR),
    `the bed runs ${(samples / SR).toFixed(6)} s, not the reel's ${DURATION} s`,
  );
});

// Playing it.

/** A decoded bed as the score reads it: its rate and length (and an id the mock logs). */
function mockBed(seconds = DURATION, sampleRate = SR): AudioBuffer {
  return {
    id: "bed",
    sampleRate,
    length: Math.round(seconds * sampleRate),
    numberOfChannels: 2,
    duration: seconds,
  } as unknown as AudioBuffer;
}

function render(
  from: number,
  when = WHEN,
  bed: AudioBuffer | null = mockBed(),
): MockContext {
  const ac = new MockContext(SR);
  playScore(ac.context, ac.destination as AudioNode, from, when, null, bed);
  return ac;
}

/** The mock's audio graph: who connects into each node. */
function inputs(ac: MockContext): Map<string, string[]> {
  const into = new Map<string, string[]>();
  for (const c of ac.calls) {
    if (c.method !== "connect") continue;
    const to = c.args[0] as string;
    into.set(to, [...(into.get(to) ?? []), c.target]);
  }
  return into;
}

/** Every node `id` feeds, however far down. */
function downstream(ac: MockContext, id: string): Set<string> {
  const seen = new Set<string>();
  const todo = [id];
  while (todo.length) {
    const n = todo.pop()!;
    for (const c of ac.calls)
      if (c.method === "connect" && c.target === n) {
        const to = c.args[0] as string;
        if (!seen.has(to)) (seen.add(to), todo.push(to));
      }
  }
  return seen;
}

const only = <T>(list: T[], what: string): T => {
  assert.equal(list.length, 1, `${list.length} ${what}`);
  return list[0];
};

/** The master chain's input, the reverb's input and the music bus, found by their wiring. */
function master(ac: MockContext) {
  const into = inputs(ac);
  // The master opens on an 18 Hz DC blocker; what feeds it is its input.
  const dc = only(
    ac.calls.filter(
      (c) =>
        c.method === "value=" &&
        c.target.endsWith(".frequency") &&
        c.args[0] === 18,
    ),
    "18 Hz filters",
  ).target.replace(/\.frequency$/, "");
  const pre = only(into.get(dc) ?? [], "inputs to the DC blocker");
  // The reverb's input feeds the room's convolver.
  const verb = only(
    ac.calls.filter(
      (c) =>
        c.method === "connect" && String(c.args[0]).startsWith("convolver#"),
    ),
    "inputs to a convolver",
  ).target;
  // The music bus feeds the duck, the gain on the master's input whose
  // level is one curve over the whole reel.
  const duck = only(
    (into.get(pre) ?? []).filter((n) =>
      ac.calls.some(
        (c) => c.target === `${n}.gain` && c.method === "setValueCurveAtTime",
      ),
    ),
    "ducked inputs to the master",
  );
  const music = only(into.get(duck) ?? [], "inputs to the music's duck");
  return { into, pre, verb, duck, music };
}

/** The bed's sources: those given the bed's buffer. */
const bedSources = (ac: MockContext): string[] =>
  ac.calls
    .filter((c) => c.method === "buffer=" && c.args[0] === "bed")
    .map((c) => c.target);

const callOf = (ac: MockContext, target: string, method: string): Call =>
  only(
    ac.calls.filter((c) => c.target === target && c.method === method),
    `${method} calls on ${target}`,
  );

/** True when `x` seconds is a whole number of frames. */
const onFrame = (x: number) => Math.abs(x * SR - Math.round(x * SR)) < 1e-6;

test("the bed plays once, dry, on the music bus, at its pinned gain", () => {
  const ac = render(0);
  const src = only(bedSources(ac), "bed sources");
  const { into, verb, music } = master(ac);
  // source → its gain → the music bus, and that is all that is on it.
  const gain = only(into.get(music) ?? [], "inputs to the music bus");
  assert.deepEqual(into.get(gain), [src]);
  assert.deepEqual(
    ac.calls
      .filter((c) => c.target === `${gain}.gain` && c.method === "value=")
      .map((c) => c.args[0]),
    [10 ** (BED.gainDb / 20)],
  );
  // It brings its own room: nothing of it reaches the reverb.
  const reach = downstream(ac, src);
  assert.ok(reach.has(music));
  assert.ok(!reach.has(verb), "the bed is sent to the reverb");
  assert.ok(reach.has("destination#0"), "the bed does not reach the output");
});

test("the master takes the effects, the music and the reverb, and nothing else: no drums bus", () => {
  for (const bed of [mockBed(), null]) {
    const ac = render(0, WHEN, bed);
    const { into, pre, verb, duck } = master(ac);
    const feeds = into.get(pre) ?? [];
    assert.equal(feeds.length, 3, `the master's inputs: ${feeds.join(", ")}`);
    assert.ok(feeds.includes(duck));
    assert.ok(
      downstream(ac, verb).has(pre),
      "the reverb returns to the master",
    );
  }
});

test("without a decoded bed, no bed plays, and nothing at all is on the music bus", () => {
  const ac = render(0, WHEN, null);
  assert.deepEqual(bedSources(ac), []);
  const { into, music } = master(ac);
  assert.deepEqual(into.get(music) ?? [], []);
});

test("from any start, the bed starts on an exact frame, reads from a whole frame and lines up with the voices", () => {
  // The voices' frame for reel time t (Mix.at, less its half frame).
  const frameOf = (t: number, from: number, when: number) =>
    Math.round((when + (t - from) - LATENCY) * SR);
  const cases: [from: number, when: number][] = [
    [0, WHEN],
    [0.013, WHEN],
    [4.2, WHEN],
    [57.3, WHEN],
    [128.1, WHEN],
    [129.5, WHEN],
    [300, WHEN],
    [422, WHEN],
    [435.5, WHEN],
    // A context started on reel time `from` has no frames before it, so
    // the first LATENCY of bed is skipped, as the voices skip it (Mix.floor).
    [0, 0],
    [100, 0],
  ];
  for (const [from, when] of cases) {
    const what = `from ${from} at ${when}`;
    const ac = render(from, when);
    const src = only(bedSources(ac), `bed sources ${what}`);
    const [start, offset] = callOf(ac, src, "start").args as [number, number];
    assert.ok(onFrame(start), `${what}: starts between frames, at ${start}`);
    assert.ok(
      onFrame(offset),
      `${what}: reads from between frames, at ${offset}`,
    );
    assert.ok(start >= 0, `${what}: starts before the context does`);
    const floor = Math.max(from, from + LATENCY - when);
    assert.equal(
      Math.round(offset * SR),
      Math.round(floor * SR),
      `${what}: reads from ${offset}`,
    );
    // Bed frame k plays on the frame a voice at reel time k / SR plays on.
    assert.equal(
      Math.round(start * SR),
      frameOf(Math.round(offset * SR) / SR, from, when),
      `${what}: the bed is off the voices' frames`,
    );
    // It stops on the reel's last frame, even with a bed that runs over.
    for (const bed of [mockBed(), mockBed(DURATION + 1)]) {
      const r = render(from, when, bed);
      const s = only(bedSources(r), `bed sources ${what}`);
      const [stop] = callOf(r, s, "stop").args as [number];
      assert.ok(onFrame(stop), `${what}: stops between frames`);
      assert.equal(
        Math.round(stop * SR),
        frameOf(DURATION, from, when),
        `${what}: stops at ${stop}, not on the reel's end`,
      );
    }
  }
});

test("a mid-reel render plays the same bed frames, on the same output frames, as a render from the top", () => {
  // A render from `from` writes output frame 0 for reel time `from`, so
  // context frame c of it is output frame c + from × SR of a render from the
  // top (both less the same pre-roll). Bed frame k's output frame, measured
  // that way, less k: the same for every start, or the bed slides against
  // the reel between renders.
  const lineUp = (from: number) => {
    const ac = render(from);
    const src = only(bedSources(ac), `bed sources from ${from}`);
    const [start, offset] = callOf(ac, src, "start").args as [number, number];
    return (
      Math.round(start * SR) - Math.round(offset * SR) + Math.round(from * SR)
    );
  };
  const top = lineUp(0);
  for (const from of [4.2, 57.3, 129.5, 190.3, 300])
    assert.equal(lineUp(from), top, `from ${from}`);
});

test("the bed must be decoded at the score's rate", () => {
  assert.throws(() => render(0, WHEN, mockBed(DURATION, 44100)), /44100 Hz/);
});

test("the music ducks under the effects BED.duck as deep as each accent asks", () => {
  // The accents the score records as it builds (Mix.duck), alone.
  const ctx = new MockContext(SR).context;
  const g = () => ctx.createGain();
  const m = new Mix(ctx, shared(ctx), 0, WHEN, g(), g());
  compose(m, null);
  const deepest = Math.max(...m.ducks.map(([, d]) => d));
  // The duck's curve, whole-reel, logged with its lowest value.
  const ac = render(0);
  const { duck } = master(ac);
  const curve = callOf(ac, `${duck}.gain`, "setValueCurveAtTime");
  const min = Number(/min ([\d.e-]+)/.exec(String(curve.args[0]))?.[1]);
  // The deepest accent stands alone: the curve bottoms out at its scaled
  // depth, on the grid point nearest it.
  assert.ok(
    Math.abs(min - (1 - BED.duck * deepest)) < 0.01,
    `the music dips to ${min}, not 1 - ${BED.duck} × ${deepest}`,
  );
});
