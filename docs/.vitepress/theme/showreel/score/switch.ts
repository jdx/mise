// switch: `cd` between two projects, and each project's own `node`. Each
// project prints in a key of its own: the bed's chord as it prints
// (harmony.ts), F minor for the first six beats, C minor from beat 6 and
// B♭ from beat 14, so dashboard's `node` lands in C minor and api's in B♭.
//
// The picture's cues: each `cd` is a marimba run up the chord of the moment
// as the cwd dot hops, from its root to the octave; and each project's
// `node --version` prints on M1's head in the chord it prints in, its fifth
// rising a fourth to its root (G → C for dashboard, F → B♭ for api).

import type { Part } from ".";
import { CH, type Changes, type Chord, chordAt, rootOf } from "./harmony";
import { marimba } from "./instruments";
import { beatIn, listen } from "./listen";
import { hz, type Mix } from "./mix";
import { PAN } from "./props";

const CHANGES: Changes = [
  [0, CH.Fm],
  [6, CH.Cm],
  [14, CH.Bb],
];

/** Note `n` moved by octaves to the lowest at or over `floor`. */
const over = (n: number, floor: number): number =>
  floor + ((((n - floor) % 12) + 12) % 12);

/** A chord's root between B♭4 and A5: the runs start on it, the heads land an octave up. */
const root = (chord: Chord): number => over(rootOf(chord), 70);

/** A run up a chord, from its root to the octave, a 32nd a note: the dot hopping to its folder. */
function run(m: Mix, t: number, notes: readonly number[], pan: number): void {
  notes.forEach((n, i) =>
    marimba(m, t + 0.1 * i, hz(n), 0.06 + 0.008 * i, pan),
  );
}

/** M1's head on a root: the fifth a sixteenth before the root, which lands on `t`. */
function head(m: Mix, t: number, r: number, pan: number): void {
  marimba(m, t - 0.2, hz(r - 5), 0.07, pan);
  marimba(m, t, hz(r), 0.09, pan, { len: 0.9 });
}

/** The folders sit in the card column, dashboard below api. */
const FOLDERS = PAN.card;

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("switch", s, facts);
    const at = (t: number) => chordAt(CHANGES, beatIn(s, t) + 1e-3, t);
    for (const t of [...c.all("cdDashboard"), ...c.all("cdApi")]) {
      const r = root(at(t));
      const up = at(t)
        .high.map((n) => over(n, r))
        .sort((a, b) => a - b);
      run(m, t, [...up, r + 12], FOLDERS);
    }
    for (const t of [...c.all("dashboard"), ...c.all("api")])
      head(m, t, root(at(t)) + 12, FOLDERS);
  },
};
