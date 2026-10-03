// depends: `ci` depends on lint, test and build, which can run in
// parallel. Act IV is a voice per lane (plan v3 §5, lanes.ts), each struck
// on the bed's chord of the moment (harmony.ts): F, its third left out,
// under the command and the lanes, C minor from beat 6 as the lanes are
// read, and B♭ from beat 14 as they settle into [tasks].
//
// The picture's cues: each lane's voice on its entrance, all three and the
// bell on ci's, and a click as the lanes settle into [tasks], which lights.

import type { Part } from ".";
import { CH, type Changes, chordAt } from "./harmony";
import { ciIn, type Lane, laneIn } from "./lanes";
import { beatIn, listen } from "./listen";
import { seat } from "./props";

const LANES: Lane[] = ["lint", "test", "build"];

const CHANGES: Changes = [
  [0, CH.F5],
  [6, CH.Cm],
  [14, CH.Bb],
];

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("depends", s, facts);
    const at = (t: number) => chordAt(CHANGES, beatIn(s, t) + 1e-3, t);
    for (const lane of LANES) {
      const t = c.at(lane);
      if (t !== null) laneIn(m, t, lane, at(t));
    }
    const ci = c.at("ci");
    if (ci !== null) ciIn(m, ci, at(ci));
    for (const t of c.all("settle")) seat(m, t + 0.3, 0.8);
  },
};
