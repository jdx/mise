// use: no `jq`, then `mise use jq`. The bed plays F minor, C minor from
// beat 6 and B♭ from beat 14 (harmony.ts), the chords the slam's stab is
// struck on.
//
// The picture's cues: `command not found` is the sigh left hanging, A♭ to
// G with no F; `mise use jq` slams in as big type on a low hit and a brass
// stab of the chord; `jq = "latest"` seats with the click; the ⌃L
// keycap is a soft click; and `"ok"` is M1's head on the marimba, C to F,
// up high.

import type { Part } from ".";
import { CH, type Changes, chordAt } from "./harmony";
import { marimba, pizz } from "./instruments";
import { beatIn, listen } from "./listen";
import { hz } from "./mix";
import { PAN, keyClick, seat, slam } from "./props";

const CHANGES: Changes = [
  [0, CH.Fm],
  [6, CH.Cm],
  [14, CH.Bb],
];

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("use", s, facts);
    for (const t of c.all("notFound")) {
      pizz(m, t, hz(68), 0.06, PAN.term, 0.25, { len: 0.35 });
      pizz(m, t + 0.2, hz(67), 0.05, PAN.term, 0.25, { len: 0.6 });
    }
    for (const t of c.all("slam"))
      slam(m, t, chordAt(CHANGES, beatIn(s, t), t).mid, 1, PAN.term);
    for (const t of c.all("seat")) seat(m, t);
    for (const t of c.all("clear")) keyClick(m, t);
    for (const t of c.all("ok")) {
      marimba(m, t, hz(84), 0.07, PAN.term);
      marimba(m, t + 0.1, hz(89), 0.08, PAN.term, { len: 0.7 });
    }
  },
};
