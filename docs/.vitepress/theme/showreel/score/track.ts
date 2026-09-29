// track: `mise dot` keeps a history of a dotfile and rolls it back. Acts V
// and VI are the song's reggae verses (plan v3 §5): off-beat chops at full
// level, a thin bass, and the one-drop, over F minor | D♭ | E♭, twice, a
// verse for the baseline and one for the watcher's save, then B♭ minor | C
// as the rollback is planned and typed. The rollback rolls the harmony
// back too: the band stops for the tape's rewind and comes back on the
// chord it started from, F minor, before the dominant turns toward the
// Everywhere ticket in the closing 2/4 bar.
//
// The picture's cues: each checkpoint joining the rail is a tape deck's
// click, and the rollback is the tape rewinding into it; the alias's copy
// seats in the ~/.zshrc card with the click. A chop that would fall in the
// quarter beat before a click rests, so the click speaks alone: node 2's
// landed a sixth of a second after one and was lost under it.

import type { Part } from ".";
import type { ReelFacts } from "../bible";
import { BEAT, type Section } from "../timeline";
import { LV, offbeatsNear, type Rests, skank } from "./band";
import { CH, type Changes, type Chord } from "./harmony";
import { beatIn, listen } from "./listen";
import { PAN, rewind, seat, tapeClick } from "./props";

/** The rewind takes this many beats, landing on the rollback. */
const REWIND = 0.75;

function rollback(s: Section, facts: ReelFacts | null): number | null {
  const t = listen("track", s, facts).at("rollback");
  return t === null ? null : beatIn(s, t);
}

/** A bar each, up to the rollback: two verses, then iv, V into it. */
const VERSES: readonly Chord[] = [
  CH.Fm,
  CH.Db,
  CH.Eb,
  CH.Fm,
  CH.Db,
  CH.Eb,
  CH.Bbm,
  CH.C,
];

function changes(s: Section, facts: ReelFacts | null): Changes {
  const rb = rollback(s, facts);
  // The closing 2/4 bar turns to the dominant.
  const turn = s.beats - 2;
  const upTo = Math.min(rb ?? turn, turn);
  const out: [number, Chord][] = [];
  for (let i = 0; 4 * i < upTo - 1e-6; i++)
    out.push([4 * i, VERSES[i % VERSES.length]]);
  if (rb !== null && rb < turn) out.push([rb, CH.Fm]);
  if (rb === null || rb < turn - 1) out.push([turn, CH.C]);
  return out.sort((a, b) => a[0] - b[0]);
}

/** The rail sits under the ~/.zshrc card, in the card column. */
const RAIL = PAN.card;

/** How far before a click a chop is too close to it, beats. */
const CLEAR = 0.3;

/** The off-beat chops that would crowd a checkpoint's or the seat's click. */
function crowded(s: Section, facts: ReelFacts | null): Rests {
  const c = listen("track", s, facts);
  return offbeatsNear(s, [...c.all("checkpoints"), ...c.all("seat")], CLEAR, 0);
}

export const part: Part = {
  level: 1.45,
  cues(m, s, facts) {
    const c = listen("track", s, facts);
    const rb = c.at("rollback");
    for (const t of c.all("checkpoints"))
      if (rb === null || Math.abs(t - rb) > 0.05) tapeClick(m, t, 1, RAIL);
    if (rb !== null) rewind(m, rb - REWIND * BEAT, rb, 0.1, RAIL);
    for (const t of c.all("seat")) seat(m, t, 1.2);
  },
  pads(m, s, facts) {
    const rb = rollback(s, facts);
    skank(m, s, changes(s, facts), {
      chop: LV.chop,
      bass: 0.045,
      rests: rb === null ? [] : [[rb - REWIND, rb]],
      chopRests: crowded(s, facts),
    });
  },
};
