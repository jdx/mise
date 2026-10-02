// vars: leave the project and its env goes; `.env` files load too.
//
// The picture's cues: each `cd` ticks; `APP_ENV=` printing empty folds the
// env strip's values off, two soft blips falling, C to B♭; `_.file` seats
// with the click; and `PORT=3000` brings the values back, the blips rising,
// F to B♭, on the bed's B♭. Act III
// is the quiet act (plan v3 §5), so its clicks pull the music down further
// than the other acts' (props.ts ACT3_DUCK), to speak over it softly.

import type { Part } from ".";
import { listen } from "./listen";
import { hz } from "./mix";
import { ACT3_DUCK, PAN, seat } from "./props";
import { blip, tick } from "./sounds";

/** The shell-env strip runs across the top of the terminal column. */
const STRIP = PAN.term;

export const part: Part = {
  // Act III's bed is the track's breakdown, no drums and no hats, some
  // 7 LU under its grooves; lifted 3 dB, it sits quieter than the acts
  // either side and leaves the top clear for the clicks.
  level: 1.4,
  cues(m, s, facts) {
    const c = listen("vars", s, facts);
    for (const t of [...c.all("cdOut"), ...c.all("cdIn")]) {
      m.duck(t, ACT3_DUCK, 0.1);
      tick(m, t, 2600, 0.6, PAN.term, 0.1);
    }
    for (const t of c.all("empty")) {
      blip(m, t, hz(84), 0.05);
      blip(m, t + 0.05, hz(82), 0.04);
    }
    for (const t of c.all("seat")) {
      m.duck(t, ACT3_DUCK, 0.1);
      seat(m, t, 2.7);
    }
    for (const t of c.all("port")) {
      m.duck(t, ACT3_DUCK, 0.1);
      blip(m, t, hz(77), 0.04);
      blip(m, t + 0.05, hz(82), 0.05);
      tick(m, t + 0.05, 3000, 0.12, STRIP, 0.1);
    }
  },
};
