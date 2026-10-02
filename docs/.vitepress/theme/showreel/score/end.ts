// end: the end card (kit/namecard.ts). The bed carries its own ending
// here, so the card's sounds are few, each on the card's own cue
// (END_CUES, an eighth of a beat after its bar line, as the picture has
// them): "mise-en-place" writes on as the open wrote it, the celesta's C D♭
// C a syllable (props.ts writeName); mise.jdx.dev lands with a soft click;
// and the glint crosses the chef's hat as tiny bells up from F, as the
// open's cells glinted flying to it. The tagline, the install strip and the
// platform line arrive under the bed alone.

import type { Part } from ".";
import { GLINT } from "../kit/chef";
import { END_CUES } from "../kit/namecard";
import { PAN, seat, writeName } from "./props";
import { shimmer } from "./sounds";

/** The card's stack stands at the left of the stage, as the open's name card does. */
const CARD = -0.3;

export const part: Part = {
  cues(m, s) {
    const c = END_CUES;
    [c.mise, c.en, c.place].forEach((t, i) =>
      writeName(m, s.start + t, i, CARD),
    );
    seat(m, s.start + c.url, 0.7, CARD);
    shimmer(
      m,
      s.start + c.glint,
      s.start + c.glint + GLINT.dur,
      0.02,
      PAN.card,
    );
  },
};
