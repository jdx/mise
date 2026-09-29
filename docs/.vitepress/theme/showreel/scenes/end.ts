// end (STORYBOARD.md Act VIII, ART.md §11 End): the end card, with no
// captions, keyed to the recording's measured onsets (score/song.ts CUE,
// through kit/namecard.ts END_CUES), not the grid. The chef holds from the
// morph at its place under the end lamp. On the sung "mise", "en" and
// "place" the name writes on a syllable each, with a bloom behind the
// chef; on "dev" the tagline rises word by word; across "precise and
// operational" the install strip rises in and `curl https://mise.run | sh`
// types into it; once the voice has gone the platform line fades in; on
// the button's first hit mise.jdx.dev lands, and on its second a glint
// crosses the hat into a sparkle on its right lobe. The chef's eyes never
// move. From 9 s only the motes and the grain move, and the card holds,
// silent, after the ring-out. The stack sits high (the URL's baseline at
// 790) so mise.jdx.dev stays clear of a player's controls.
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
