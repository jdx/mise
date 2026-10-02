// bootstrap: a fresh machine restores its dotfiles and mise config. The
// valley's first half (plan v3 §5): the bed plays its breakdown again (no
// drums, no hats; bed.toml), its fader at the flat 1, for work being done
// rather than silence; the breath after it is the valley's floor, where the
// fader takes the bed 23 dB down.
//
// The picture's cues: the `~/.zshrc` card flies in off its create row on
// air; each tile ticks as the take reaches it, Dotfiles and Config
// together; and the Tools tile's ring, a step started and not finished, is
// a soft ping on C that does not resolve.

import type { Part } from ".";
import { listen } from "./listen";
import { hz } from "./mix";
import { air, PAN } from "./props";
import { ping, tick } from "./sounds";

/** The tiles stand in the card column. */
const TILES = PAN.card;

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("bootstrap", s, facts);
    for (const t of c.all("card"))
      air(m, t, t + 0.6, 0.09, PAN.term, TILES, true);
    for (const t of c.all("wrote")) {
      tick(m, t, 2400, 0.2, TILES, 0.14);
      tick(m, t + 0.05, 2700, 0.18, TILES, 0.14);
    }
    for (const t of c.all("watcher")) tick(m, t, 2550, 0.18, TILES, 0.14);
    for (const t of c.all("tools"))
      ping(m, t + 0.1, hz(84), 0.03, 0.9, { pan: TILES, send: 0.45 });
  },
};
