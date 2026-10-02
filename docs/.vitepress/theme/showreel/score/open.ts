// Open: `mise` is typed and its help screen prints beside the block chef.
// The chef's cells lift off the help screen and fly to it as tiny bells
// glinting up an octave from F (sounds.ts shimmer); it resolves on beat 4,
// where the service bell rings for the name card; and as "mise-en-place"
// writes on, a syllable a quarter beat, the celesta plays the name, C D♭ C
// (props.ts writeName), as the end card writes it on again. The card's leave has no sound of its own.

import type { Part } from ".";
import { listen } from "./listen";
import { PAN, serviceBell, writeName } from "./props";
import { shimmer } from "./sounds";

/** The name card stands at the left of the stage, the chef at the right. */
const CARD = -0.3;

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("open", s, facts);
    const lift = c.or("lift", 2.5);
    const resolve = c.or("resolve", 4);
    // The cells fly off the help screen to the chef: tiny bells up from F.
    shimmer(m, lift + 0.1, resolve - 0.06, 0.028, PAN.card);
    // The name card lands with the chef: the kitchen's bell.
    serviceBell(m, resolve, 0.045, PAN.card);
    c.all("name").forEach((t, i) => writeName(m, t, i, CARD));
  },
};
