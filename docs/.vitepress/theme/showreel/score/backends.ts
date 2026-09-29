// backends: `npm:` types into its slot, prettier installs and checks, then
// the github backend and the others. F minor | D♭ | B♭ minor, held a
// second bar under the check | C for the npm half, prettier's verdict on
// the dominant; F minor | B♭ minor, C for the github half, the backend
// chips on it; and D♭, C to close, ending on the dominant for the Versions
// ticket. Bar 1 is thin under the kinetic
// type, then the whole band of Act I; the marimba sits out the closing
// bars as the terminal narrows, so the ticket prints out of a thinner band.
//
// The picture's cues: `npm:` types in on four pizzicato plucks up F minor
// in tools pink; `"npm:prettier"` seats with the click; ⌃L; prettier's
// verdict is a pass, the kitchen's bell, and its file chips tick; `mise use
// github:cli/cli` prints its tools line on a lighter click; and the
// `pypi:`, `cargo:` and `go:` chips land on three plucks up the C chord.

import type { Part } from ".";
import { cello, LV, mallets, oom, pah } from "./band";
import { CH, type Changes, perBar } from "./harmony";
import { pizz } from "./instruments";
import { listen } from "./listen";
import { hz } from "./mix";
import { PAN, keyClick, seat, serviceBell } from "./props";
import { PATTER } from "./pitch";
import { tick } from "./sounds";

const CHANGES: Changes = [
  ...perBar(CH.Fm, CH.Db, CH.Bbm, CH.Bbm, CH.C, CH.Fm),
  [24, CH.Bbm],
  [26, CH.C],
  [28, CH.Db],
  [32, CH.C],
];

/** The closing bars, the terminal narrowing and the rest: the marimba sits them out. */
const LAST = 30;

/** n, p, m, : climb F minor: C5 F5 A♭5 C6. */
const NPM = [72, 77, 80, 84];
/** The chips climb the C chord: G5 C6 E6. */
const CHIPS = [79, 84, 88];

export const part: Part = {
  level: 0.8,
  cues(m, s, facts) {
    const c = listen("backends", s, facts);
    c.all("npm").forEach((t, i) =>
      pizz(m, t, hz(NPM[i % 4]), 0.075, -0.1 + 0.05 * i, 0.25, {
        bus: "sfx",
        bright: 9,
      }),
    );
    for (const t of c.all("seat")) seat(m, t);
    for (const t of c.all("clear")) keyClick(m, t);
    for (const t of c.all("verdict")) serviceBell(m, t, 0.035, PAN.term);
    for (const t of c.all("chips")) tick(m, t, 3000, 0.22, PAN.card, 0.12);
    for (const t of c.all("gh")) seat(m, t, 0.7, PAN.term);
    c.all("backends").forEach((t, i) =>
      pizz(m, t, hz(CHIPS[i % 3]), 0.07, -0.45 + 0.15 * i, 0.3, {
        bus: "sfx",
        len: 0.6,
      }),
    );
  },
  bass: (m, s) => oom(m, s, CHANGES, { level: LV.bass }),
  pads(m, s) {
    pah(m, s, CHANGES, { vel: 0.85 * LV.pah, to: 4 });
    pah(m, s, CHANGES, { vel: LV.pah, brass: LV.brass, from: 4 });
    mallets(m, s, CHANGES, PATTER, {
      from: 4,
      to: LAST,
      vel: 0.8 * LV.mallet,
    });
    cello(m, s, CHANGES, { from: 4 });
  },
};
