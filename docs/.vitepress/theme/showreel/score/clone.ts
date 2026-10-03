// clone: on a fresh clone, `mise run ci` installs what mise.lock pins, then
// its tasks run with the project's env vars. The climax (plan v3 §5): the
// bed's fader at its peak. The stingers are struck on the bed's chords
// (harmony.ts): B♭ for the first six beats, then F, its third left out,
// over the bed's ringing C, through to the morph.
//
// The picture's cues: `mise run ci` slams in as big type on the chord;
// each of the card's headers lights, as its table is used, on that table's
// own voice from the pitch (the pizzicato for [tools], the marimba for
// [tasks], the organ for [env]), on the chord of the moment; each lane
// appears on its voice, ci's on all three; the card folds to its headers
// on air and closes with a click; the `[ci] api ready` line lands with the
// kitchen's bell, rung twice; and its pieces land home in their tables
// with a click each and their table's voice again, an octave higher,
// rising up the chord they land on.

import type { Part } from ".";
import { DUR } from "../kit/style";
import { BEAT } from "../timeline";
import { CH, type Changes, chordAt } from "./harmony";
import { marimba, organ, pizz, woodblock } from "./instruments";
import { ciIn, type Lane, laneIn } from "./lanes";
import { beatIn, listen } from "./listen";
import { hz, type Mix } from "./mix";
import { air, PAN, seat, serviceBell, slam } from "./props";

const CHANGES: Changes = [
  [0, CH.Bb],
  [6, CH.F5],
];

/** The card's tables, in the order the climax line's pieces land home (LISTEN.clone.home). */
const TABLES = ["tasks", "tools", "env"] as const;

/**
 * Each table's voice from the pitch (pitch.ts), on MIDI note `n`, at `vel`
 * of a landing's level.
 */
const VOICES: Record<
  (typeof TABLES)[number],
  (m: Mix, t: number, n: number, vel: number) => void
> = {
  tasks(m, t, n, vel) {
    woodblock(m, t, 1250, 0.14 * vel, PAN.card, 0.12);
    marimba(m, t + 0.004, hz(n), 0.11 * vel, PAN.card);
  },
  tools: (m, t, n, vel) =>
    pizz(m, t, hz(n), 0.1 * vel, PAN.card, 0.3, { len: 0.6 }),
  env: (m, t, n, vel) =>
    organ(m, t, t + 0.22, [n], 0.06 * vel, {
      speed: "fast",
      release: 0.2,
      sub: 0,
    }),
};

const LANES: Lane[] = ["test", "lint", "build"];

export const part: Part = {
  // The climax: the track's loudest groove, pushed a little more.
  level: 1.1,
  cues(m, s, facts) {
    const c = listen("clone", s, facts);
    for (const t of c.all("slam"))
      slam(m, t, chordAt(CHANGES, beatIn(s, t), t).mid, 1.1, PAN.term);
    // Each header lights on its table's voice and its own tone of the
    // chord (the one its piece rises to when it lands home), quieter than
    // the landing.
    TABLES.forEach((table, i) => {
      for (const t of c.all(table))
        VOICES[table](
          m,
          t,
          chordAt(CHANGES, beatIn(s, t) + 1e-3, t).high[i],
          0.7,
        );
    });
    for (const lane of LANES)
      for (const t of c.all(lane))
        laneIn(m, t, lane, chordAt(CHANGES, beatIn(s, t) + 1e-3, t), 0.9);
    for (const t of c.all("ci"))
      ciIn(m, t, chordAt(CHANGES, beatIn(s, t), t), 0);
    for (const t of c.all("ready")) {
      serviceBell(m, t, 0.06, 0);
      serviceBell(m, t + 0.1, 0.04, 0.1, hz(96));
    }
    // The card folds to its three headers clear of the big type, and shuts.
    for (const t of c.all("fold")) {
      air(m, t, t + DUR.fold * BEAT, 0.07, PAN.card, PAN.card, false);
      seat(m, t + DUR.fold * BEAT, 0.7);
    }
    c.all("home").forEach((t, i) => {
      seat(m, t, 0.8);
      const tones = chordAt(CHANGES, beatIn(s, t) + 1e-3, t).high;
      VOICES[TABLES[i % 3]](m, t, tones[i % 3] + 12, 1);
    });
  },
};
