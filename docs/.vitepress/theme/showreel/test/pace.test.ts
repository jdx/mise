// The pacing rules (STORYBOARD.md "Pacing", kit/style.ts PACE) as the kit
// applies them: typing at TYPE_RATE with a pause before Enter, the read
// holds, one focal motion at a time (kit/pace.ts Pace), the focus dim, and
// captions anchored on the events they explain. And the storyboard's event
// plans against the rules: every caption's `after` is a planned event, its
// anchor is where captionTimes puts it from the plan, and each section is
// long enough for its plan, its rest and its last caption.

import assert from "node:assert/strict";
import { test } from "node:test";
import { BEAT, SECTIONS, type SectionId, sec } from "../bible";
import { stepOf } from "../captures";
import {
  beatOf,
  clipTime,
  ENTER_PAUSE,
  play,
  playsEnd,
  step,
  TYPE_RATE,
  typed,
} from "../kit/grey";
import {
  FOCUS_DIM,
  focusOf,
  GAP,
  Pace,
  readHold,
  restOut,
  slamHold,
  unfocus,
} from "../kit/pace";
import { DUR, PACE } from "../kit/style";
import { boardOf, captionsFor, captionTimes, STORYBOARD } from "../storyboard";
import { entrance } from "../type";

test("60 BPM: the pacing numbers in beats", () => {
  assert.equal(BEAT, 1);
  assert.equal(readHold(0), 2);
  assert.equal(readHold(3), 2);
  assert.equal(readHold(7), 4);
  assert.equal(restOut(), 1.5);
  assert.ok(DUR.bigHold >= slamHold(), "a slam holds 1 s at full size");
  assert.equal(GAP, 0.5);
  assert.equal(FOCUS_DIM, 0.55);
});

test("typing plays at TYPE_RATE, holds ENTER_PAUSE on the typed command, then runs at 1x", () => {
  // Take time 1 to 2 typed, Enter at 2, output to 3.
  const p = typed(4, 1, 2, 3);
  const typing = 1 / (BEAT * TYPE_RATE);
  assert.equal(clipTime([p], 4).ct, 1);
  assert.ok(Math.abs(clipTime([p], 4 + typing / 2).ct - 1.5) < 1e-9);
  // The pause shows the frame just before Enter's.
  const paused = clipTime([p], 4 + typing + ENTER_PAUSE / 2).ct;
  assert.ok(paused < 2 && paused > 2 - 1e-3, `${paused}`);
  // From Enter, real time.
  const enterAt = 4 + typing + ENTER_PAUSE;
  assert.ok(Math.abs(clipTime([p], enterAt).ct - 2) < 1e-9);
  assert.ok(Math.abs(clipTime([p], enterAt + 0.5 / BEAT).ct - 2.5) < 1e-9);
  assert.ok(Math.abs(playsEnd([p]) - (enterAt + 1 / BEAT)) < 1e-9);
  // beatOf inverts it.
  assert.ok(Math.abs(beatOf([p], 1.5) - (4 + typing / 2)) < 1e-9);
  assert.ok(Math.abs(beatOf([p], 2) - enterAt) < 1e-9);
  assert.ok(Math.abs(beatOf([p], 2.5) - (enterAt + 0.5)) < 1e-9);
  // Never a time-lapse while typing or printing at 1x.
  assert.equal(clipTime([p], 4.1).lapse, false);
  // A plain play is unchanged.
  const q = play(0, 0, 2, 2);
  assert.equal(playsEnd([q]), 1 / BEAT);
});

test("at TYPE_RATE no take types more than 12 characters a second", () => {
  // The rig's keys come 45 ms apart: at 0.5 they show 90 ms apart.
  const shown = 0.045 / TYPE_RATE;
  assert.ok(1 / shown <= 12, `${1 / shown} a second`);
  assert.ok(TYPE_RATE <= 0.5);
});

test("Pace: one focal motion at a time, half a beat apart; Enter waits out the last output's read hold", () => {
  const p = new Pace("use");
  const a = p.move("a", 1);
  assert.equal(a.at, GAP);
  const b = p.move("b", 1);
  assert.equal(b.at, a.end + GAP);
  // A terminal act: its Enter never lands inside the last output's hold.
  const fake = (len: number) => (at: number) => [typed(at, 0, len, len + 0.5)];
  const t1 = p.term("t1", fake(1), { lines: 1 });
  assert.equal(t1.at, b.end + GAP);
  const held = t1.held;
  assert.equal(held, t1.end + readHold(1));
  const c = p.move("c", 0.5);
  assert.equal(c.at, t1.end + GAP);
  const t2 = p.term("t2", fake(0.25), { lines: 1 });
  assert.ok(t2.enter >= held - 1e-9, `Enter at ${t2.enter}, hold to ${held}`);
  assert.ok(t2.at >= c.end + GAP - 1e-9);
  assert.deepEqual(p.issues, []);
  assert.equal(p.events().t1, t1.end);
  // The section needs its rest after the last motion, and the read hold.
  assert.ok(p.need({ half: true }) >= t2.held);
  assert.ok(p.need({ half: true }) >= t2.end + restOut());
  assert.equal(p.need({ half: true }) % 2, 0);
});

test("Pace.caption anchors a caption half a beat after its `after` event, as captionsFor does", () => {
  const id: SectionId = "use";
  const after = boardOf(id).captions[0].after!;
  const p = new Pace(id);
  const seat = p.move(after, 3);
  const cap = p.caption(0);
  const times = captionTimes(id, null, p.events());
  assert.equal(times[0].at, Math.ceil((seat.end + GAP) * 8 - 1e-6) / 8);
  assert.equal(cap.at, times[0].at);
  assert.equal(cap.end, times[0].landed);
  // The scene's captions follow its events, not the storyboard's time.
  const [c] = captionsFor(id, null, { [after]: seat.end + 5 });
  const start = entrance(c.lines[0].text, c.lines[0].in * BEAT) / BEAT;
  assert.ok(start >= seat.end + 5 + GAP - 1e-9);
  assert.ok(start <= seat.end + 5 + GAP + 0.375);
  assert.deepEqual(p.issues, []);
});

test("focusOf dims everything but the focus to FOCUS_DIM, easing over DUR.dim", () => {
  const plan = [
    [2, "term"],
    [6, ["card", "term"]],
    [8, "*"],
  ] as const;
  assert.equal(focusOf(plan, "card", 0), 1);
  assert.equal(focusOf(plan, "card", 2), 1);
  assert.equal(focusOf(plan, "card", 2 + DUR.dim), FOCUS_DIM);
  assert.equal(focusOf(plan, "term", 3), 1);
  assert.equal(focusOf(plan, "card", 6 + DUR.dim), 1);
  assert.equal(focusOf(plan, "rail", 7), FOCUS_DIM);
  assert.equal(focusOf(plan, "rail", 8 + DUR.dim), 1);
  const mid = focusOf(plan, "card", 2 + DUR.dim / 2);
  assert.ok(mid < 1 && mid > FOCUS_DIM);
  assert.equal(unfocus(plan, "card", 3), 1);
  assert.equal(unfocus(plan, "term", 3), 0);
});

test("step() types at TYPE_RATE by default and at the take's pace with typeRate 1 and no pause", () => {
  const c = {
    id: "C3",
    width: 80,
    height: 30,
    styles: [],
    rows: [],
    frames: [{ t: 0, rows: [], cursor: [0, 0], hidden: false }],
    marks: [
      { name: "a", t: 1 },
      { name: "b", t: 4 },
    ],
    keys: [
      [2, "l"],
      [2.5, "\r"],
    ],
  } as const;
  const s = stepOf(c as never, "b");
  const slow = step(c as never, "b", 0);
  const fast = step(c as never, "b", 0, { typeRate: 1, pause: 0 });
  const typing = s.enter - s.type;
  assert.ok(
    Math.abs(
      playsEnd([slow]) -
        playsEnd([fast]) -
        (typing / (BEAT * TYPE_RATE) - typing / BEAT + ENTER_PAUSE),
    ) < 1e-9,
  );
});

test("every caption names the planned event it follows, and starts where the plan puts it", () => {
  for (const b of STORYBOARD) {
    const plan = b.plan ?? [];
    const events = Object.fromEntries(plan.map((e) => [e.name, e.end]));
    const times = captionTimes(b.id as SectionId, null, events);
    b.captions.forEach((c, k) => {
      assert.ok(c.after, `${b.id} caption ${k + 1} names no event`);
      assert.ok(
        c.after in events,
        `${b.id} caption ${k + 1}: no "${c.after}" in the plan`,
      );
      assert.ok(
        Math.abs(sec(b.id as SectionId).beat(times[k].at) - c.at) < 1e-6,
        `${b.id} caption ${k + 1}: at ${c.at}, the plan puts it at ${sec(b.id as SectionId).beat(times[k].at)}`,
      );
      const plannedCap = plan.find((e) => e.name === `caption${k + 1}`);
      assert.ok(plannedCap, `${b.id}: caption ${k + 1} is not in the plan`);
    });
  }
});

test("every event plan fits its section: in order, one focal motion at a time, its rest and its last caption inside", () => {
  for (const s of SECTIONS) {
    const b = boardOf(s.id);
    const plan = b.plan ?? [];
    if (s.id === "morph" || s.id === "end" || ("ticket" in s && s.ticket))
      continue;
    assert.ok(plan.length > 0, `${s.id} has no event plan`);
    let last = -Infinity;
    for (const e of plan) {
      assert.ok(e.end >= e.at, `${s.id} ${e.name} ends before it starts`);
      assert.ok(e.at >= 0 && e.end <= s.beats, `${s.id} ${e.name} is outside`);
      assert.ok(
        e.at >= last + GAP - 1e-6,
        `${s.id}: ${e.name} starts at b${e.at}, ${e.at - last} after the last motion`,
      );
      last = Math.max(last, e.end);
    }
    const extra = s.id === "lock" ? 1.5 : 0; // the whip's wind-up
    assert.ok(
      last + restOut() + extra <= s.beats + 1e-6,
      `${s.id}: its last motion ends at b${last}, no rest before b${s.beats}`,
    );
    const caps = captionsFor(
      s.id,
      null,
      Object.fromEntries(plan.map((e) => [e.name, e.end])),
    );
    for (const c of caps)
      assert.ok(
        c.out <= s.beats - 1 + 1e-9,
        `${s.id}: a caption holds too long`,
      );
  }
});

test("the pacing rules' numbers are the brief's", () => {
  assert.equal(PACE.typeRate, 0.5);
  assert.equal(PACE.read, 2);
  assert.equal(PACE.readPerLine, 0.5);
  assert.equal(PACE.lapseUp, 1.5);
  assert.equal(PACE.restOut, 1.5);
  assert.equal(PACE.slamHold, 1);
  assert.equal(PACE.captionEvery, 4);
});
