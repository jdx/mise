// breath: the lights go down over bootstrap's last frame and a Ctrl-L
// clears the same shell. Near silence (plan v3 §5): the bed's fader at its
// lowest, and the ⌃L a soft click in the quiet. The synthesized band went
// near silent here on its own (two faint pads, some 14 dB under
// bootstrap's), so its fader dipped only 5 dB; a recorded bed plays on at
// whatever its arrangement puts here (bed.toml runs one breakdown across
// bootstrap and the breath), so the fader takes the whole dip, 23 dB under
// bootstrap's, down with the lights at the breath's downbeat and up into
// the clone's.

import type { Part } from ".";
import { listen } from "./listen";
import { keyClick } from "./props";

export const part: Part = {
  level: 0.07,
  cues(m, s, facts) {
    for (const t of listen("breath", s, facts).all("clear"))
      keyClick(m, t, 0.8);
  },
};
