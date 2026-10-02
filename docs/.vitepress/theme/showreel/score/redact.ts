// redact: `redact = true` masks a secret in task output.
//
// The picture's cues: the `_.file` line lifts out of the card on air and is
// rewritten with the click; the two file cards slide in; `[redacted]`
// prints on a tick and the stamp lands round it with the thunk; and the
// file cards fold back into the card. Act III's clicks duck the music
// deeper (props.ts ACT3_DUCK).

import type { Part } from ".";
import { listen } from "./listen";
import { ACT3_DUCK, air, PAN, seat, thunk } from "./props";
import { tick } from "./sounds";

export const part: Part = {
  // The breakdown, lifted as in vars.
  level: 1.4,
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
    // its knock and slap struck harder than skip's: it was voiced over Act
    // III's organ, which sat in their band.
    for (const t of c.all("stamp")) thunk(m, t, 1, PAN.term, 1.8);
    for (const t of c.all("fold")) {
      air(m, t, t + 0.6, 0.07, PAN.card, PAN.card, false);
      seat(m, t + 0.6, 0.8);
    }
  },
};
