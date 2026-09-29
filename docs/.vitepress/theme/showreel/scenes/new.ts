// new: the ticket that opens A new machine (STORYBOARD.md Act VII). The
// whip (whip.ts; greyScene wraps the layers in it) lands on an empty
// terminal at `~ $`, its chrome badged "fresh machine · mise installed",
// and the act's ticket, terracotta, `mise bootstrap` on its band, comes
// down on the narrow rail beside it in place, over the whip (the terminal
// whips in under it; the ticket is not part of the whipping stage), and
// prints, tears, swings and lifts off. There
// was nothing to clear (the whip cleared it), and the terminal is already
// up, so the ticket brings nothing in: the terminal and its badge hold
// into bootstrap (kit/rest.ts new|bootstrap).

import { BEAT, sec } from "../bible";
import { drawSectionTicket, greyScene, rest, ticket } from "../kit/grey";
import { restLit } from "../kit/rest";
import { progress } from "../math";
import type { SceneCues } from "../score/cues";
import { WHIP_AT, WHIP_END, whipIn } from "../whip";
import { TICKET_CUES } from "./g1-brand/tickets";

const START = sec("new").start;

export const scene = greyScene(
  "new",
  (g) => {
    rest(g, "new|bootstrap");
    ticket(g, { bring: false, ticket: false });
  },
  {
    over: drawSectionTicket,
    // The terminal as the whip brings it in from the right: nothing on the
    // bar line (lock|new keeps nothing), new|bootstrap's once it lands.
    lit: (b) => {
      const z = restLit("new|bootstrap");
      const t = START + b * BEAT;
      const k = progress(WHIP_AT, WHIP_END, t);
      if (!z || k <= 0) return null;
      if (k >= 1) return z;
      return { ...z, x: z.x + whipIn(t), alpha: Math.min(1, 4 * k) };
    },
  },
);

/** The score's cues: the printer's steps, the tear, the lift-off. */
export const cues: SceneCues<"new"> = TICKET_CUES;
