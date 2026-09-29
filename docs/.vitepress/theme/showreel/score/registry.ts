// registry: three rivers of the registry's names drift across the stage.
// The harmony lifts to the relative major for them, E♭ | A♭, and comes back
// through B♭ minor and C to backends' F minor. The marimba flows in
// sixteenths while the rivers pour in, then, as they slow to a drift and
// the caption is read, in eighths; `node` and `terraform` light on the
// celesta, high, as the caption names them.

import type { Part } from ".";
import { cello, type Figure, LV, mallets, oom, pah } from "./band";
import { CH, type Changes, perBar } from "./harmony";
import { celesta } from "./instruments";
import { listen } from "./listen";
import { hz } from "./mix";

const CHANGES: Changes = [...perBar(CH.Eb, CH.Ab, CH.Bbm), [10, CH.C]];

/** The river: up and down the chord in sixteenths, never still. */
const FLOW: Figure = [0, 1, 2, 3, 2, 1, 0, 1, 2, 3, 2, 1, 2, 1, 0, 1];
/** The drift, once the rivers slow: the same shape in eighths. */
const DRIFT: Figure = [
  0,
  null,
  1,
  null,
  2,
  null,
  3,
  null,
  2,
  null,
  1,
  null,
  2,
  null,
  1,
  null,
];
/** The rivers are in by here (the plan's `rivers` event), and drift from then on. */
const IN = 2;

/** The rivers run across the left of the stage, x 100 to 1100: the names that light sit there. */
const RIVERS = -0.25;
/**
 * The marimba's flow sits right of centre, across from the pizzicato: at
 * the rivers' side it leaned the section 3.6 dB left.
 */
const FLOW_PAN = 0.2;

export const part: Part = {
  level: 0.78,
  cues(m, s, facts) {
    const lit = listen("registry", s, facts).all("lit");
    const notes = [82, 87];
    lit.forEach((t, i) =>
      celesta(m, t, hz(notes[i % 2]), 0.07, RIVERS + 0.1 * i, 1.6, 0.45, "sfx"),
    );
  },
  bass: (m, s) => oom(m, s, CHANGES, { level: 0.9 * LV.bass }),
  pads(m, s) {
    pah(m, s, CHANGES, { vel: 0.9 * LV.pah, brass: 0.8 * LV.brass });
    mallets(m, s, CHANGES, FLOW, {
      to: IN,
      vel: 0.6 * LV.mallet,
      pan: FLOW_PAN,
    });
    mallets(m, s, CHANGES, DRIFT, {
      from: IN,
      vel: 0.7 * LV.mallet,
      pan: FLOW_PAN,
    });
    cello(m, s, CHANGES);
  },
};
