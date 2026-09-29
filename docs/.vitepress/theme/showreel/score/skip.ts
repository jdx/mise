// skip: with `sources` and `outputs`, up-to-date work is skipped. The lane
// voices carry on over D♭ | C | B♭ minor | C, but build's marimba is gone:
// its work was skipped, and so is its figure. The figures chop sixteenths
// while the run prints and the lanes refill, and move in eighths under the
// command before it and the caption's read after it.
//
// The picture's cues: `sources` and `outputs` seat with two clicks; the
// hero line `[build] sources up-to-date, skipping` is build's marimba
// struck dead; the lanes refill, build's dead again, the others in their
// voices and ci's on the chord; and the "skipped" stamp lands with a
// lighter thunk.

import type { Part } from ".";
import { LV, oom, type Rests, trio } from "./band";
import { CH, chordAt, perBar } from "./harmony";
import { ciIn, laneIn, laneSkipped } from "./lanes";
import { beatIn, listen } from "./listen";
import { PAN, seat, thunk } from "./props";

const CHANGES = perBar(CH.Db, CH.C, CH.Bbm, CH.C);

/** Where the figures move in eighths: under the command, and the caption's read. */
const CALM: Rests = [
  [0, 3],
  [8, 16],
];

export const part: Part = {
  level: 1.1,
  cues(m, s, facts) {
    const c = listen("skip", s, facts);
    const at = (t: number) => chordAt(CHANGES, beatIn(s, t));
    for (const t of c.all("seat")) {
      seat(m, t);
      seat(m, t + 0.1, 0.7);
    }
    for (const t of [...c.all("skipped"), ...c.all("build")])
      laneSkipped(m, t, at(t));
    for (const t of c.all("test")) laneIn(m, t, "test", at(t), 0.8);
    for (const t of c.all("lint")) laneIn(m, t, "lint", at(t), 0.8);
    for (const t of c.all("ci")) ciIn(m, t, at(t), 0);
    // The thunk on the stamp's landing (score/cues.ts LISTEN.skip.stamp).
    for (const t of c.all("stamp")) thunk(m, t, 0.7, PAN.term);
  },
  bass: (m, s) => oom(m, s, CHANGES, { level: LV.bass }),
  pads: (m, s) =>
    trio(m, s, CHANGES, {
      lint: 0.9 * LV.lint,
      test: 0.9 * LV.test,
      calm: CALM,
    }),
};
