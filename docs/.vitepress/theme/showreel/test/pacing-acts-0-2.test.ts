// Acts 0 to II's scenes against the pacing rules (STORYBOARD.md "Pacing"),
// as they schedule themselves from the capture set (each scene's
// `pacing`): one focal motion at a time with the section's rest after the
// last (kit/pace.ts Pace, rules 4 and 6), typing no faster than 12 keys a
// second (rule 2), and every caption arriving only once what it explains
// is on screen and still (rule 5).

import assert from "node:assert/strict";
import { test } from "node:test";
import { BEAT, type ReelFacts, type SectionId, sec } from "../bible";
import type { Capture, CaptureId, ReelData } from "../captures";
import { beatOf, type Play } from "../kit/grey";
import type { Pace } from "../kit/pace";
import { GAP } from "../kit/pace";
import { loadFacts } from "../load";
import * as backends from "../scenes/backends";
import * as open from "../scenes/open";
import * as packslip from "../scenes/packslip";
import * as pitch from "../scenes/pitch";
import * as registry from "../scenes/registry";
import * as switchScene from "../scenes/switch";
import * as use from "../scenes/use";
import { boardOf } from "../storyboard";
import { entrance } from "../type";
import { REPO } from "./repo";

type Pacing = (facts: ReelFacts | null) => {
  pace: Pace;
  takes: Partial<Record<CaptureId, readonly Play[]>>;
};

const SCENES: [SectionId, { pacing: Pacing; scene: typeof use.scene }][] = [
  ["open", open],
  ["pitch", pitch],
  ["use", use],
  ["registry", registry],
  ["backends", backends],
  ["switch", switchScene],
  ["packslip", packslip],
];

const { facts, report } = loadFacts(REPO);
const d = facts as ReelData | null;

function needFacts(t: { skip: (m: string) => void }): ReelData | null {
  if (d) return d;
  if (process.env.SHOWREEL_REQUIRE_CAPTURES) assert.fail(report.join("; "));
  t.skip(`no capture set: ${report.join("; ")}`);
  return null;
}

test("acts 0-II: one focal motion at a time, and each section long enough for its rest", () => {
  for (const [id, m] of SCENES)
    for (const f of [d, null]) {
      const { pace } = m.pacing(f as ReelFacts | null);
      assert.deepEqual(pace.issues, [], `${id}`);
      assert.ok(
        pace.need({ half: true }) <= sec(id).beats,
        `${id} needs ${pace.need({ half: true })} beats of ${sec(id).beats}`,
      );
    }
});

test("acts 0-II: no take types more than 12 keys in any second on screen", (t) => {
  const f = needFacts(t);
  if (!f) return;
  let checked = 0;
  for (const [id, m] of SCENES) {
    const { takes } = m.pacing(f as unknown as ReelFacts);
    for (const [cid, plays] of Object.entries(takes)) {
      const c = f.captures[cid as CaptureId] as Capture;
      // The keys a moving play shows (a cut shows none typing), on their beats.
      const shown = c.keys
        .filter(([, k]) => k.length === 1 && k >= " ")
        .map(([at]) => at)
        .filter((at) =>
          plays!.some((p) => p.to > p.from + 1e-6 && at > p.from && at <= p.to),
        )
        .map((at) => beatOf(plays!, at))
        .filter((b) => Number.isFinite(b))
        .sort((a, z) => a - z);
      checked += shown.length;
      let j = 0;
      for (let i = 0; i < shown.length; i++) {
        while (shown[i] - shown[j] >= 1 / BEAT) j++;
        assert.ok(
          i - j + 1 <= 12,
          `${id} ${cid}: ${i - j + 1} keys in the second before b${shown[i]}`,
        );
      }
    }
  }
  // Every command the seven sections type is on screen.
  assert.ok(checked > 150, `only ${checked} keys shown`);
});

test("acts 0-II: each caption starts half a beat after the event it explains is still", () => {
  for (const [id, m] of SCENES)
    for (const f of [d, null]) {
      const events = m.pacing(f as ReelFacts | null).pace.events();
      const caps = m.scene.captions?.(f as never) ?? [];
      boardOf(id).captions.forEach((want, k) => {
        const c = caps[k];
        const start = entrance(c.lines[0].text, c.lines[0].in * BEAT) / BEAT;
        const after = events[want.after!];
        assert.ok(after !== undefined, `${id}: no "${want.after}" event`);
        assert.ok(
          start >= after + GAP - 1e-6,
          `${id} caption ${k + 1} starts at b${start}, "${want.after}" is still at b${after}`,
        );
      });
    }
});
