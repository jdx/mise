// The task lanes (depends, skip, clone): each lane's voice (plan v3 §5),
// lint on the woodblocks, test on the pizzicato, build on the marimba,
// sounds as its lane appears, on the chord of the moment, and ci's lane
// resolves them: all three at once and the kitchen's bell.

import type { Chord } from "./harmony";
import { marimba, pizzChord, woodblock } from "./instruments";
import { hz, type Mix } from "./mix";
import { serviceBell } from "./props";

export type Lane = "lint" | "test" | "build";

/** Lanes sit in the terminal column, x 160 to 1100. */
const LANES = -0.2;

/** A lane appearing: its voice, once, on the chord sounding then. */
export function laneIn(
  m: Mix,
  t: number,
  lane: Lane,
  chord: Chord,
  vel = 1,
): void {
  if (lane === "lint") {
    // 3 dB over the other lanes: a woodblock's knock is short, and its
    // 1.25 kHz sits in the pizzicato's and the marimba's band.
    woodblock(m, t, 1250, 0.34 * vel, LANES - 0.15, 0.14, "sfx");
    woodblock(m, t + 0.1, 880, 0.25 * vel, LANES - 0.15, 0.14, "sfx");
  } else if (lane === "test") {
    pizzChord(m, t, chord.high, 0.17 * vel, LANES, { bus: "sfx", send: 0.25 });
  } else {
    marimba(m, t, hz(chord.high[0]), 0.15 * vel, LANES + 0.15, { bus: "sfx" });
    marimba(m, t + 0.1, hz(chord.high[2]), 0.17 * vel, LANES + 0.15, {
      bus: "sfx",
    });
  }
}

/** A lane that did not run: the build's marimba struck dead, muted in the hand. */
export function laneSkipped(m: Mix, t: number, chord: Chord): void {
  marimba(m, t, hz(chord.high[0]), 0.08, LANES + 0.15, {
    bus: "sfx",
    len: 0.07,
  });
  woodblock(m, t + 0.002, 520, 0.08, LANES + 0.15, 0.08, "sfx");
}

/** ci's lane: every voice on the tonic, and the bell. */
export function ciIn(m: Mix, t: number, chord: Chord, bell = 0.045): void {
  woodblock(m, t, 1250, 0.2, LANES - 0.15, 0.14, "sfx");
  pizzChord(m, t + 0.003, chord.high, 0.16, LANES, { bus: "sfx", send: 0.25 });
  marimba(m, t + 0.006, hz(chord.high[2]), 0.18, LANES + 0.15, { bus: "sfx" });
  if (bell) serviceBell(m, t + 0.01, bell, LANES);
}
