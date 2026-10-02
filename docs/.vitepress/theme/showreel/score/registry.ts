// registry: three rivers of the registry's names drift across the stage.
//
// The picture's cues: `node` and `terraform` light on the celesta, high, as
// the caption names them, B♭ and F, a fifth up the bed's B♭.

import type { Part } from ".";
import { celesta } from "./instruments";
import { listen } from "./listen";
import { hz } from "./mix";

/** The rivers run across the left of the stage, x 100 to 1100: the names that light sit there. */
const RIVERS = -0.25;

export const part: Part = {
  cues(m, s, facts) {
    const lit = listen("registry", s, facts).all("lit");
    const notes = [82, 89];
    lit.forEach((t, i) =>
      celesta(m, t, hz(notes[i % 2]), 0.07, RIVERS + 0.1 * i, 1.6, 0.45),
    );
  },
};
