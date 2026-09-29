// from jdx/hk@37937824 docs/.vitepress/theme/showreel/test/handoff.test.ts
// Every bar line's handoff (handoff.ts): one per boundary, in order, on its
// bar line, holds except the whip, and each scene starting from the one it
// inherits and ending on the one it owes. handoff-frames.test.ts draws the
// frames and holds every scene to them.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  type LitRect,
  type ReelFacts,
  SECTIONS,
  type SectionId,
  sec,
} from "../bible";
import { BOUNDARIES, HANDOFFS, handoffIn, handoffOut } from "../handoff";
import { scenes } from "../scenes";
import { WHIP_AT, WHIP_END, WHIP_START, whipIn, whipOut } from "../whip";

const VARIANTS: [string, ReelFacts | null][] = [
  ["no facts", null],
  ["empty facts", {}],
];

test("every bar line has a handoff, in order, on its bar line", () => {
  assert.equal(BOUNDARIES.length, SECTIONS.length - 1);
  assert.equal(Object.keys(HANDOFFS).length, SECTIONS.length - 1);
  assert.deepEqual(Object.keys(HANDOFFS), BOUNDARIES);
  BOUNDARIES.forEach((id, i) => {
    const h = HANDOFFS[id];
    assert.ok(h, `no handoff for ${id}`);
    assert.equal(h.id, id);
    assert.equal(h.from, SECTIONS[i].id);
    assert.equal(h.to, SECTIONS[i + 1].id);
    assert.equal(h.t, sec(h.to).start);
    assert.equal(h.t, sec(h.from).end);
    assert.ok(h.note.length > 20, `${id} says what is on its frame`);
  });
  for (const { id } of SECTIONS) {
    const i = SECTIONS.findIndex((s) => s.id === id);
    assert.equal(handoffIn(id)?.to ?? null, i === 0 ? null : id);
    assert.equal(
      handoffOut(id)?.from ?? null,
      i === SECTIONS.length - 1 ? null : id,
    );
  }
});

test("every bar line holds but the whip into the new machine", () => {
  for (const id of BOUNDARIES)
    assert.equal(HANDOFFS[id].meet, id === "lock|new" ? "motion" : "hold", id);
});

test("the whip crosses new's bar line: out of shot on it, in shot on either side", () => {
  assert.equal(WHIP_AT, sec("new").start);
  assert.equal(WHIP_AT, HANDOFFS["lock|new"].t);
  // Wind-up 1.5 beats before (lock's last beat and a half), cleared a beat after (new b1).
  assert.equal(WHIP_START, sec("lock").beat(sec("lock").beats - 1.5));
  assert.equal(WHIP_END, sec("new").beat(1));
  assert.ok(WHIP_END < sec("new").end, "the whip clears inside the ticket");
  assert.equal(whipOut(WHIP_START), 0);
  assert.ok(
    whipOut(WHIP_AT) <= -1920,
    "the outgoing content is gone on the bar line",
  );
  assert.ok(
    whipOut(WHIP_AT - 1 / 120) > -1920,
    "and still in shot on the frame before it",
  );
  assert.ok(
    whipIn(WHIP_AT) >= 1760,
    "the incoming content is off the right edge on the bar line",
  );
  assert.equal(whipIn(WHIP_END), 0);
});

test("each scene returns its handoffs' lit screen on its side of the bar line, whatever the facts", () => {
  // A screen at alpha 0 is no screen: the vignette covers it either way.
  const norm = (r: LitRect | null | undefined) =>
    r && r.alpha > 0
      ? { x: r.x, y: r.y, w: r.w, h: r.h, alpha: +r.alpha.toFixed(6) }
      : null;
  const scene = (id: SectionId) => scenes.find((s) => s.id === id)!;
  for (const [what, facts] of VARIANTS) {
    // Given as the compositor gives it, with nothing drawn first.
    const env = { facts };
    for (const id of BOUNDARIES) {
      const h = HANDOFFS[id];
      const want = norm(h.lit);
      const out = scene(h.from);
      assert.deepEqual(
        norm(out.lit?.(out.end - out.start - 1 / 120, env)),
        want,
        `${what}: ${h.from}'s last frame is lit as ${id} is`,
      );
      assert.deepEqual(
        norm(scene(h.to).lit?.(0, env)),
        want,
        `${what}: ${h.to}'s first frame is lit as ${id} is`,
      );
    }
  }
});
