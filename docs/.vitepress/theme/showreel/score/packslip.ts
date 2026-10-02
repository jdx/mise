// packslip: hk's signed releases carry completions and agent skills.
//
// The picture's cues: `hk = "…"` lights on a pizzicato F; the slip slides
// up on air; each Tab keycap is a soft pop, and the new list's
// `--junit-xml` a brighter, higher pluck; the card flips to the new
// version with the click and the slip's chip re-stamps lightly; the
// `linked` rows tick; and the drawn link lights on the celesta's G G G F,
// the song's "the C-I both" (plan v3 §5, motif.ts BOTH).

import type { Part } from ".";
import { celesta, pizz } from "./instruments";
import { listen } from "./listen";
import { hz } from "./mix";
import { BOTH, landing, onReel } from "./motif";
import { air, PAN, seat, thunk } from "./props";
import { pop, tick } from "./sounds";

/** The slip sits under the terminal, the Tab list in it. */
const SLIP = PAN.term;

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("packslip", s, facts);
    for (const t of c.all("light"))
      pizz(m, t, hz(77), 0.07, PAN.card, 0.3, { len: 0.6 });
    for (const t of c.all("slip")) air(m, t, t + 0.4, 0.07, SLIP, SLIP, true);
    for (const t of c.all("tab")) pop(m, t, hz(72), 0.18, SLIP, 0.15);
    for (const t of c.all("flip")) {
      seat(m, t);
      thunk(m, t + 0.05, 0.35, SLIP);
    }
    for (const t of c.all("junit")) {
      pop(m, t, hz(72), 0.18, SLIP, 0.15);
      pizz(m, t + 0.03, hz(87), 0.08, SLIP + 0.1, 0.35, {
        bright: 11,
        wobble: 0.006,
        len: 0.8,
      });
    }
    for (const t of c.all("linked")) tick(m, t, 3400, 0.2, SLIP, 0.1);
    for (const t of c.all("link")) {
      const e0 = landing(BOTH[0][0], t);
      for (const [a, , n, v] of onReel(BOTH, e0, 1, 12))
        celesta(m, a, hz(n), 0.08 * v, SLIP + 0.1, 1.5, 0.42);
    }
  },
};
