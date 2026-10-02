// morph: the card's lit headers become the toque's lobes and the toque
// settles on the chef, into the end card.
//
// The picture's cues: the three headers lift off, glinting as the open's
// cells did; each settles over its lobe on the celesta, F, A♭, C, the notes
// its table lit on in the pitch; the toque's drop starts on a soft kick, on
// bar 2's downbeat; and it touches the head with a puff and the kitchen's
// bell, as the chef blooms. The bed carries the rest into the end card.

import type { Part } from ".";
import { celesta, kick } from "./instruments";
import { listen } from "./listen";
import { hz } from "./mix";
import { serviceBell } from "./props";
import { puff, shimmer, tick } from "./sounds";

/** The lobes sit over the chef's head, left to right: tools, env, tasks. */
const LOBES = [0.27, 0.36, 0.45];
/** Each lobe's note: its table's in the pitch, F A♭ C up the tonic chord. */
const LOBE_NOTES = [77, 80, 84];

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("morph", s, facts);
    const lands = c.all("lands");
    for (const t of c.all("lift"))
      shimmer(m, t, lands[0] ?? t + 1.5, 0.03, LOBES[1]);
    lands.forEach((t, i) => {
      celesta(m, t, hz(LOBE_NOTES[i % 3]), 0.07, LOBES[i % 3], 1.6, 0.45);
      tick(m, t, 3000, 0.12, LOBES[i % 3], 0.12);
    });
    // At the level it had on the band's drums bus (0.8 there, under the
    // bus's 0.6 and the morph's fader of 0.75).
    for (const t of c.all("drop")) kick(m, t, 0.36);
    for (const t of c.all("land")) {
      puff(m, t, 0.05, LOBES[1], false);
      serviceBell(m, t + 0.005, 0.045, LOBES[1]);
    }
  },
};
