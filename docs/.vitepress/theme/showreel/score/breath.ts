// breath: the lights go down over bootstrap's last frame and a Ctrl-L
// clears the same shell. Near silence (plan v3 §5): the pulse has stopped,
// the valley's last chord fades in the room as the lights go down, and the
// ⌃L is a soft click in the quiet after it. As the lights come back up
// (the scene's `up`, from beat 3.75), the dominant the clone's intro
// swells on begins to rise out of nothing, a beat ahead of them.

import type { Part } from ".";
import { listen } from "./listen";
import { keyClick } from "./props";
import { pad } from "./sounds";

/** Where the dominant starts to rise: a beat before the lights come up. */
const RISE = 2.75;

export const part: Part = {
  level: 0.4,
  cues(m, s, facts) {
    for (const t of listen("breath", s, facts).all("clear"))
      keyClick(m, t, 0.8);
  },
  pads(m, s) {
    pad(m, s.start, s.beat(1.5), [53, 56, 60, 67], 0.012, 900, 0.05, 1.2, 240);
    pad(
      m,
      s.beat(RISE),
      s.end + 0.3,
      [48, 55, 58, 64],
      0.02,
      900,
      s.end - s.beat(RISE),
      0.4,
      200,
    );
  },
};
