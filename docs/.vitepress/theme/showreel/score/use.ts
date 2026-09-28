// use: no `jq`, then `mise use jq`. Over F minor | D♭ | B♭ minor | F
// minor | D♭, the band plays bar 1 thin, the bass and the pizzicato alone
// under the missing command, and comes in whole with the slam: the brass on
// the and of 2 and of 4, and the marimba's patter. `"ok"` lands on the
// tonic's bar, and the marimba rests after it, so its answer is the
// marimba's last word; D♭ turns up to registry's E♭.
//
// The picture's cues: `command not found` is the sigh left hanging, A♭ to
// G with no F; `mise use jq` slams in as big type on a low hit and a brass
// stab of the bar's chord; `jq = "latest"` seats with the click; the ⌃L
// keycap is a soft click; and `"ok"` is M1's head on the marimba, C to F,
// up high.

import type { Part } from ".";
import { cello, LV, mallets, oom, pah } from "./band";
import { CH, chordAt, perBar } from "./harmony";
import { marimba, pizz } from "./instruments";
import { beatIn, listen } from "./listen";
import { hz } from "./mix";
import { PAN, keyClick, seat, slam } from "./props";
import { PATTER } from "./pitch";

const CHANGES = perBar(CH.Fm, CH.Db, CH.Bbm, CH.Fm, CH.Db);

/** The bar after `"ok"`, the card's leave and the rest: the marimba sits it out. */
const LAST = 16;

export const part: Part = {
  level: 0.8,
  cues(m, s, facts) {
    const c = listen("use", s, facts);
    for (const t of c.all("notFound")) {
      pizz(m, t, hz(68), 0.06, PAN.term, 0.25, { bus: "sfx", len: 0.35 });
      pizz(m, t + 0.2, hz(67), 0.05, PAN.term, 0.25, { bus: "sfx", len: 0.6 });
    }
    for (const t of c.all("slam"))
      slam(m, t, chordAt(CHANGES, beatIn(s, t)).mid, 1, PAN.term);
    for (const t of c.all("seat")) seat(m, t);
    for (const t of c.all("clear")) keyClick(m, t);
    for (const t of c.all("ok")) {
      marimba(m, t, hz(84), 0.07, PAN.term, { bus: "sfx" });
      marimba(m, t + 0.1, hz(89), 0.08, PAN.term, { bus: "sfx", len: 0.7 });
    }
  },
  bass: (m, s) => oom(m, s, CHANGES, { level: LV.bass }),
  pads(m, s, facts) {
    const slamAt = listen("use", s, facts).at("slam");
    const full = slamAt === null ? 4 : Math.max(0, beatIn(s, slamAt));
    pah(m, s, CHANGES, { vel: 0.85 * LV.pah, to: full - 0.25 });
    pah(m, s, CHANGES, { vel: LV.pah, brass: LV.brass, from: full + 0.25 });
    mallets(m, s, CHANGES, PATTER, {
      from: full + 0.5,
      to: LAST,
      vel: 0.8 * LV.mallet,
    });
    cello(m, s, CHANGES, { from: full });
  },
};
