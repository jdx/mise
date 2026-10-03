// skip: with `sources` and `outputs`, up-to-date work is skipped. The lane
// voices are struck on the bed's chord (harmony.ts): B♭ for the first
// bar, then F minor, and F from beat 10.
//
// The picture's cues: `sources` and `outputs` seat with two clicks; the
// hero line `[build] sources up-to-date, skipping` is build's marimba
// struck dead; the lanes refill, build's dead again, the others in their
// voices and ci's on the chord; and the "skipped" stamp lands with a
// lighter thunk.

import type { Part } from ".";
import { CH, type Changes, chordAt } from "./harmony";
import { ciIn, laneIn, laneSkipped } from "./lanes";
import { beatIn, listen } from "./listen";
import { PAN, seat, thunk } from "./props";

const CHANGES: Changes = [
  [0, CH.Bb],
  [2, CH.Fm],
  [10, CH.F5],
];

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("skip", s, facts);
    const at = (t: number) => chordAt(CHANGES, beatIn(s, t), t);
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
};
