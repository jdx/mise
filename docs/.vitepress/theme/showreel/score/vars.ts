// vars: leave the project and its env goes; `.env` files load too. Act
// III is quieter (plan v3 §5): the organ holds the chords on its slow
// rotor, brushes swirl and tap the off beats, and the bass plays only on 1
// and 3. The chords walk down F minor | E♭ | D♭ as the project is left,
// come home to F minor on the bar after `cd api` brings its env back, and
// turn to D♭ in the closing 2/4 bar, which falls to redact's C.
//
// The picture's cues: each `cd` ticks; `APP_ENV=` printing empty folds the
// env strip's values off, two soft blips falling; `_.file` seats with the
// click; and `PORT=3000` brings the values back, the blips rising. The
// cues are set firmer than Act I's: Act III is the quiet act, but its
// brushes sit where the clicks do. Each click here and in redact stands
// at least 6 dB over the music in its loudest third-octave (measured on
// the mix less a render without the clicks, its first 80 ms).

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
import { hz } from "./mix";
import { PAN, seat } from "./props";
import { blip, tick } from "./sounds";

const CHANGES: Changes = [...perBar(CH.Fm, CH.Eb, CH.Db, CH.Fm), [16, CH.Db]];

/** The shell-env strip runs across the top of the terminal column. */
const STRIP = PAN.term;

export const part: Part = {
  level: 0.85,
  cues(m, s, facts) {
    const c = listen("vars", s, facts);
    for (const t of [...c.all("cdOut"), ...c.all("cdIn")]) {
      m.duck(t, ACT3_DUCK, 0.1);
      tick(m, t, 2600, 0.6, PAN.term, 0.1);
    }
    for (const t of c.all("empty")) {
      blip(m, t, hz(84), 0.05);
      blip(m, t + 0.05, hz(80), 0.04);
    }
    for (const t of c.all("seat")) {
      m.duck(t, ACT3_DUCK, 0.1);
      seat(m, t, 2.7);
    }
    for (const t of c.all("port")) {
      m.duck(t, ACT3_DUCK, 0.1);
      blip(m, t, hz(79), 0.04);
      blip(m, t + 0.05, hz(82), 0.05);
      tick(m, t + 0.05, 3000, 0.12, STRIP, 0.1);
    }
  },
  drums(m, s, facts) {
    const c = listen("vars", s, facts);
    const clicks = [
      ...c.all("cdOut"),
      ...c.all("empty"),
      ...c.all("seat"),
      ...c.all("cdIn"),
      ...c.all("port"),
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
