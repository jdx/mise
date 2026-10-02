// from jdx/hk@37937824 docs/.vitepress/theme/showreel/test/timeline.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ACTS,
  BAR,
  BEAT,
  BEATS,
  BPM,
  beat,
  CHAPTERS,
  DURATION,
  SECTIONS,
  sec,
  TICKET_BEATS,
} from "../bible";
import { POSTER_TIME } from "../reel";
import { scenes } from "../scenes";

/** The frame rates the reel is rendered at: animatic drafts, the fallback, and the full render. */
const FPS = [30, 60, 120];

/** True when `t` is exactly a frame's time at `fps`, the same double `i / fps` gives. */
const onFrame = (t: number, fps: number): boolean =>
  Math.round(t * fps) / fps === t;

test("60 BPM: a beat is 1 s and a 4/4 bar 4 s", () => {
  assert.equal(BPM, 60);
  assert.equal(BEAT, 1);
  assert.equal(BAR, 4);
  assert.equal(TICKET_BEATS * BEAT, 2);
});

test("sections run end to end, from 0 to DURATION", () => {
  let t = 0;
  let beats = 0;
  for (const { id, beats: n } of SECTIONS) {
    const s = sec(id);
    assert.equal(s.start, t, `${id} starts where the previous section ends`);
    assert.equal(s.start, beat(beats));
    assert.equal(s.len, beat(n));
    assert.equal(s.end, beat(beats + n));
    t = s.end;
    beats += n;
  }
  assert.equal(t, DURATION);
  assert.equal(beats, BEATS);
});

test("every section is whole 4/4 bars, or whole bars and a closing 2/4 bar, but a ticket, which is one 2/4 bar", () => {
  for (const s of SECTIONS) {
    const ticket = "ticket" in s && s.ticket;
    if (ticket)
      assert.equal(
        s.beats,
        TICKET_BEATS,
        `${s.id} is a ticket, so it is one 2/4 bar`,
      );
    else
      assert.ok(
        Number.isInteger(s.beats / 2) && s.beats >= 4,
        `${s.id}: ${s.beats} beats is not whole 4/4 bars (and a 2/4 bar)`,
      );
    assert.equal(sec(s.id).ticket, ticket);
  }
});

test("a ticket opens its act, and only an act's opening can be one", () => {
  for (const s of SECTIONS)
    if (sec(s.id).ticket)
      assert.equal(sec(s.id).index, 0, `${s.id} is a ticket inside its act`);
});

test("every bar line, beat and eighth lands on a frame at 30, 60 and 120 fps, a sixteenth at 60 and 120, a 32nd at 120", () => {
  for (const fps of FPS) {
    for (let b = 0; b <= BEATS * 8; b++) {
      const t = beat(b / 8);
      // An eighth is every 4th 32nd, a sixteenth every 2nd.
      const on = b % 4 === 0 || (b % 2 === 0 && fps >= 60) || fps === 120;
      assert.equal(
        onFrame(t, fps),
        on,
        `the 32nd at beat ${b / 8} (${t} s) at ${fps} fps`,
      );
    }
    for (const { id } of SECTIONS) {
      assert.ok(
        onFrame(sec(id).start, fps),
        `${id} does not start on a frame at ${fps} fps`,
      );
      assert.ok(
        onFrame(sec(id).end, fps),
        `${id} does not end on a frame at ${fps} fps`,
      );
    }
    assert.ok(
      Number.isInteger(DURATION * fps),
      `${DURATION} s is not whole frames at ${fps} fps`,
    );
  }
});

test("sec() places beats, bars, and local times from the section's start", () => {
  for (const { id } of SECTIONS) {
    const s = sec(id);
    assert.equal(s.beat(0), s.start);
    assert.equal(s.bar(0), s.start);
    assert.equal(s.at(0), s.start);
    assert.equal(s.beat(s.beats), s.end);
    if (!s.ticket) assert.equal(s.bar(s.beats / 4), s.end);
    assert.ok(Math.abs(s.beat(1.5) - s.start - 1.5 * BEAT) < 1e-9);
  }
  assert.throws(() => sec("nope" as never));
});

test("acts hold whole runs of sections, in order, and each is one chapter", () => {
  const order = ACTS.map((a) => a.id);
  let last = -1;
  for (const s of SECTIONS) {
    const i = order.indexOf(s.act);
    assert.ok(
      i === last || i === last + 1,
      `${s.id} (${s.act}) is out of its act's run`,
    );
    last = i;
  }
  assert.equal(last, ACTS.length - 1, "every act has a section");
  assert.deepEqual(
    CHAPTERS.map((c) => c.id),
    order,
  );
  for (const c of CHAPTERS) {
    const mine = SECTIONS.filter((s) => s.act === c.id).map((s) => s.id);
    assert.deepEqual(c.sections, mine);
    assert.equal(c.start, sec(mine[0]).start);
    assert.equal(c.end, sec(mine[mine.length - 1]).end);
  }
  assert.equal(CHAPTERS[0].start, 0);
  assert.equal(CHAPTERS[CHAPTERS.length - 1].end, DURATION);
});

test("one scene per section, in order, on its section's span", () => {
  assert.deepEqual(
    scenes.map((s) => s.id),
    SECTIONS.map((s) => s.id),
  );
  for (const scene of scenes) {
    const s = sec(scene.id);
    assert.equal(scene.start, s.start, scene.id);
    assert.equal(scene.end, s.end, scene.id);
  }
});

test("the timeline is the retime's act table: plan v3's acts at 60 BPM, each section as long as its event plan", () => {
  assert.deepEqual(
    ACTS.map((a) => a.label),
    [
      "mise",
      "Dev tools",
      "Versions",
      "Environments",
      "Tasks",
      "Dotfiles",
      "Everywhere",
      "A new machine",
      "Install mise",
    ],
  );
  assert.equal(SECTIONS.length, 27);
  assert.equal(SECTIONS.filter((s) => "ticket" in s && s.ticket).length, 7);
  // Plan v3 was 258 beats at 75 BPM (206.4 s). The retime (STORYBOARD.md
  // "Pacing") plays at 60 BPM and gives each section the beats its event
  // plan needs: 436 beats, 7:16, 52,320 frames at 120 fps. The morph keeps
  // its 8 beats and the end card takes 14: the card's moments, then the
  // bed's ending, its ring-out clear of the last beat (decision 5).
  assert.equal(BEATS, 436);
  assert.equal(DURATION, 436);
  assert.equal(DURATION * 120, 52320);
  assert.equal(sec("morph").beats, 8);
  assert.equal(sec("end").beats, 14);
  assert.equal(sec("end").start, 422);
  // The act boundaries, seconds.
  assert.deepEqual(
    CHAPTERS.map((c) => c.start),
    [0, 42, 110, 170, 210, 288, 328, 352, 414],
  );
});

test("no chapter or section label carries a number, so the chapters track never goes stale", () => {
  for (const { id, label } of ACTS) assert.doesNotMatch(label, /\d/, id);
  for (const { id, label } of SECTIONS) assert.doesNotMatch(label, /\d/, id);
});

test("the poster is switch at beat 18, both versions on their folder cards: 2:10", () => {
  assert.equal(POSTER_TIME, sec("switch").beat(18));
  assert.ok(Math.abs(POSTER_TIME - 130) < 1e-9);
});
