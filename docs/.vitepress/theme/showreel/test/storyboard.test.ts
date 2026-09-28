// The timeline and the captions against the storyboard (sections.json, from
// plan v3's timing checker): the same sections, acts, beats and times; every
// caption drawn verbatim from its anchor; version tokens filled from the
// facts, never written; and every line short enough with any versions the
// capture run could resolve (plan v3 §2's four sets).

import assert from "node:assert/strict";
import { test } from "node:test";
import { ACTS, BEAT, sec, SECTIONS } from "../bible";
import type { ReelData } from "../captures";
import { fill } from "../captures";
import { scenes } from "../scenes";
import { captionsFor, STORYBOARD } from "../storyboard";
import { entrance, plain } from "../type";

test("timeline.ts is sections.json: the same sections, acts, beats and times", () => {
  assert.deepEqual(
    SECTIONS.map((s) => s.id),
    STORYBOARD.map((s) => s.id),
  );
  for (const b of STORYBOARD) {
    const s = sec(b.id as (typeof SECTIONS)[number]["id"]);
    const act = ACTS.find((a) => a.id === s.act)!;
    assert.equal(act.numeral, b.act, `${b.id}'s act`);
    assert.equal(act.label, b.actLabel, `${b.id}'s act label`);
    assert.equal(s.beats, b.beats, `${b.id}'s beats`);
    assert.ok(
      Math.abs(s.start - b.start) < 1e-9,
      `${b.id} starts at ${b.start}`,
    );
    assert.ok(Math.abs(s.end - b.end) < 1e-9, `${b.id} ends at ${b.end}`);
  }
});

/** A capture set's versions, as plan v3 §2's variant sets. */
const versions = (
  lts: string,
  ltsV: string,
  other: string,
  otherV: string,
  variant = "systemd",
): ReelData =>
  ({
    schema: 1,
    variant,
    versions: {
      node: {
        lts_major: Number(lts),
        lts_version: ltsV,
        other_major: Number(other),
        other_version: otherV,
      },
    },
  }) as unknown as ReelData;

const SETS: [string, ReelData][] = [
  ["today", versions("24", "24.21.0", "26", "26.10.0")],
  ["after the alias bump", versions("26", "26.10.0", "24", "24.21.0")],
  ["after 27.0.0", versions("26", "26.10.0", "27", "27.0.0")],
  ["three digits", versions("100", "100.100.100", "101", "101.100.100")],
];

test("every caption is the storyboard's, verbatim, starting on its anchor", () => {
  const today = SETS[0][1];
  for (const scene of scenes) {
    const board = STORYBOARD.find((b) => b.id === scene.id)!;
    const caps = scene.captions?.(today as never) ?? [];
    assert.equal(
      caps.length,
      board.captions.length,
      `${scene.id}: caption count`,
    );
    caps.forEach((c, i) => {
      const want = board.captions[i];
      assert.deepEqual(
        c.lines.map((l) => l.text),
        want.lines.map((l) =>
          fill(l, {
            "node.lts_major": "24",
            "node.lts_version": "24.21.0",
            "node.other_major": "26",
            "node.other_version": "26.10.0",
          }),
        ),
        `${scene.id} caption ${i + 1}`,
      );
      // The first word starts to rise on the anchor, or within a beat of it
      // (plan v3 §2), where the lines' landings snap to the quarter beat.
      const s = sec(scene.id);
      const start = s.start + entrance(c.lines[0].text, c.lines[0].in * BEAT);
      assert.ok(
        start >= want.at - 1e-6 && start <= want.at + BEAT + 1e-6,
        `${scene.id} caption ${i + 1} starts at ${start}, anchor ${want.at}`,
      );
    });
  }
  assert.deepEqual(
    captionsFor("morph", today).length + captionsFor("end", today).length,
    0,
    "the closing act has no captions",
  );
});

test("captions write no version numbers: digits come from tokens or 1,000+", () => {
  for (const b of STORYBOARD)
    for (const c of b.captions)
      for (const l of c.lines) {
        const rest = l.replace(/\{[a-z_.]+\}/g, "").replace("1,000+", "");
        assert.doesNotMatch(rest, /\d/, `${b.id}: "${l}"`);
      }
});

test("with any versions the capture could resolve, every caption line fits in 36 characters", () => {
  for (const [what, d] of SETS)
    for (const scene of scenes)
      for (const c of scene.captions?.(d as never) ?? [])
        for (const l of c.lines)
          assert.ok(
            plain(l.text).length <= 36,
            `${what}: "${plain(l.text)}" (${plain(l.text).length})`,
          );
  // A later set shows none of today's numbers.
  for (const [what, d] of SETS.slice(1)) {
    const text = scenes
      .flatMap((s) => s.captions?.(d as never) ?? [])
      .flatMap((c) => c.lines.map((l) => l.text))
      .join("\n");
    if (what !== "after the alias bump")
      assert.doesNotMatch(text, /24\.21\.0/, what);
  }
});

test("without systemd on machine 2, the bootstrap caption drops the watcher", () => {
  const plainSet = versions("24", "24.21.0", "26", "26.10.0", "plain");
  const [, second] = captionsFor("bootstrap", plainSet);
  assert.deepEqual(
    second.lines.map((l) => l.text),
    ["Then it installs your global tools."],
  );
});
