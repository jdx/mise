// bootstrap: a fresh machine restores its dotfiles and mise config. The
// valley's first half (plan v3 §5): the band has gone, and a filtered pulse
// on F ticks in eighths, its filter opening across the section as the
// machine comes back, under a thin pad that barely moves, two bars a
// chord: F minor | D♭ | E♭ and F minor again, the chord the breath lets
// ring out. It is scored as work being done, about 5 dB under the acts, not
// as silence: the breath after it is the valley's floor. Kept darker and
// quieter, bootstrap sat 10 dB under the acts and read as dead air before
// the breath had begun. At 60 BPM its sixteenths ran four a second under
// the prompts and captions being read, so it ticks half as often, a
// little louder.
//
// The picture's cues: the `~/.zshrc` card flies in off its create row on
// air; each tile ticks as the take reaches it, Dotfiles and Config
// together; and the Tools tile's ring, a step started and not finished, is
// a soft ping on C that does not resolve.

import type { Part } from ".";
import { pulse } from "./band";
import { listen } from "./listen";
import { hz } from "./mix";
import { air, PAN } from "./props";
import { pad, ping, tick } from "./sounds";

/** The tiles stand in the card column. */
const TILES = PAN.card;

/**
 * The thin pad, two bars a chord, then F minor with its ninth again: from
 * and to (section beats, null for the end), notes, level, cutoff, attack
 * and release.
 */
const PADS = [
  [0, 8, [53, 56, 60, 67], 0.065, 1700, 0.9, 0.3],
  [8, 16, [49, 56, 60, 65], 0.065, 1700, 0.8, 0.3],
  [16, 20, [51, 55, 58, 67], 0.06, 1600, 0.8, 0.5],
  [20, null, [53, 56, 60, 67], 0.06, 1600, 0.8, 0.5],
] as const;

export const part: Part = {
  level: 0.7,
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
  pads(m, s) {
    pulse(m, s, {
      note: 41,
      vel: 0.14,
      cutoff: (b) => 420 * 3.5 ** (b / s.beats),
      calm: [[0, s.beats]],
    });
    const thin = 240;
    for (const [from, to, notes, vel, cutoff, attack, release] of PADS)
      pad(
        m,
        s.beat(from),
        (to === null ? s.end : s.beat(to)) - 0.05,
        [...notes],
        vel,
        cutoff,
        attack,
        release,
        thin,
      );
  },
};
