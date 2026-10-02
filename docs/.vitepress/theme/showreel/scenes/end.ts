// end (STORYBOARD.md Act VIII, ART.md §11 End): the end card, with no
// captions, each move on an eighth of a beat after its bar line
// (kit/namecard.ts END_CUES). The chef holds from the morph at its place
// under the end lamp. On 0, 3/8 and 5/8 of the first beat the name writes on
// a syllable each, with a bloom behind the chef; from beat 1.5 the tagline
// rises word by word; the install strip rises in and `curl
// https://mise.run | sh` types into it; the platform line fades in;
// mise.jdx.dev lands, and a glint crosses the hat into a sparkle on its
// right lobe. The chef's eyes never move. From 9 s only the motes move,
// and the card holds under the bed's ending. The stack sits
// high (the URL's baseline at 790) so mise.jdx.dev stays clear of a
// player's controls.
//
// The end card is drawn whole by the brand kit (namecard.ts drawEndCard):
// its first frame is the chef at rest (kit/rest.ts chefRest), so it is
// kept, out from under the section's edge fade.

import { chefLit } from "../kit/chef";
import { greyScene, keep } from "../kit/grey";
import { drawEndCard } from "../kit/namecard";
import { CHEF_ART } from "../kit/style";

export const scene = greyScene(
  "end",
  (g) => keep(g, () => drawEndCard(g.ctx, g.lt, g.t)),
  // The chef stays the lit screen the vignette spares, as on morph|end.
  { lit: () => chefLit(CHEF_ART.place) },
);
