// track: `mise dot` keeps a history of a dotfile and rolls it back.
//
// The picture's cues: each checkpoint joining the rail is a tape deck's
// click, and the rollback is the tape rewinding into it; the alias's copy
// seats in the ~/.zshrc card with the click.

import type { Part } from ".";
import { BEAT } from "../timeline";
import { listen } from "./listen";
import { PAN, rewind, seat, tapeClick } from "./props";

/** The rewind takes this many beats, landing on the rollback. */
const REWIND = 0.75;

/** The rail sits under the ~/.zshrc card, in the card column. */
const RAIL = PAN.card;

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("track", s, facts);
    const rb = c.at("rollback");
    for (const t of c.all("checkpoints"))
      if (rb === null || Math.abs(t - rb) > 0.05) tapeClick(m, t, 1, RAIL);
    if (rb !== null) rewind(m, rb - REWIND * BEAT, rb, 0.1, RAIL);
    for (const t of c.all("seat")) seat(m, t, 1.2);
  },
};
