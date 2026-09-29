// redact: `redact = true` masks a secret in task output. Act III's organ
// and brushes carry on over C | D♭ | F minor | B♭ minor | C: the dominant
// as the line lifts and is rewritten, D♭ under the file cards, the tonic
// as `[redacted]` prints and is stamped, and iv, V under the caption and
// the fold, turning again for the Tasks ticket.
//
// The picture's cues: the `_.file` line lifts out of the card on air and is
// rewritten with the click; the two file cards slide in; `[redacted]`
// prints on a tick and the stamp lands round it with the thunk; and the
// file cards fold back into the card.

import type { Part } from ".";
import {
  ACT3_DUCK,
  ACT3_TAPS,
  brushes,
  LV,
  offbeatsNear,
  oom,
  organBars,
} from "./band";
import { CH, type Changes, perBar } from "./harmony";
import { listen } from "./listen";
import { air, PAN, seat, thunk } from "./props";
import { tick } from "./sounds";

const CHANGES: Changes = perBar(CH.C, CH.Db, CH.Fm, CH.Bbm, CH.C);

export const part: Part = {
  level: 0.85,
  cues(m, s, facts) {
    const c = listen("redact", s, facts);
    for (const t of c.all("zoom")) air(m, t, t + 0.6, 0.09, PAN.card, 0);
    for (const t of c.all("rewrite")) {
      m.duck(t, ACT3_DUCK, 0.1);
      seat(m, t, 1.75, 0);
    }
    for (const t of c.all("cards"))
      air(m, t, t + 0.4, 0.07, 0.7, PAN.card, false);
    for (const t of c.all("redacted")) {
      m.duck(t, ACT3_DUCK, 0.1);
      tick(m, t, 2400, 0.95, PAN.term, 0.1);
    }
    // The stamp's thunk on its landing (score/cues.ts LISTEN.redact.stamp),
    // its knock and slap struck harder than skip's: Act III's organ sits in
    // their band.
    for (const t of c.all("stamp")) thunk(m, t, 1, PAN.term, 1.8);
    for (const t of c.all("fold")) {
      air(m, t, t + 0.6, 0.07, PAN.card, PAN.card, false);
      seat(m, t + 0.6, 0.8);
    }
  },
  drums(m, s, facts) {
    const c = listen("redact", s, facts);
    const clicks = [
      ...c.all("rewrite"),
      ...c.all("redacted"),
      ...c.all("stamp"),
      ...c.all("fold").map((t) => t + 0.6),
    ];
    brushes(m, s, {
      vel: LV.swirl,
      taps: ACT3_TAPS,
      rests: offbeatsNear(s, clicks, 0.2, 0.15),
    });
  },
  bass: (m, s) =>
    oom(m, s, CHANGES, { level: 0.75 * LV.bass, every: 2, len: 3 }),
  pads: (m, s) => organBars(m, s, CHANGES, { vel: LV.organ, speed: "slow" }),
};
