// Pitch: one mise.toml per project. The band of Acts 0 to II settles in
// over F minor | D♭ | B♭ minor | C, and goes round again, the dominant
// answered deceptively by D♭ under the second caption and the tonic kept
// for `mise run ci`: D♭ | F minor | B♭ minor, C. The oom-pah of bass and
// pizzicato, the brass leaning on the and of 2 and of 4 from bar 2, and the
// marimba's patter in eighths under the captions, which sits out the last
// bar so the Dev tools ticket prints out of a thinner band.
//
// The picture's cues: the card's three tables light in turn, each on its
// act's voice, F A♭ C up the tonic chord (a pizzicato for [tools], an organ
// for [env], a woodblock and a marimba for [tasks]); the cwd dot hops on
// `cd api` with a tick (only dashboard changes key); `mise run ci` prints
// its tasks' lines as a ripple, lint, test, build and a ding for ci; and
// the version and env var click home on the card rows that asked for them.

import type { Part } from ".";
import { cello, type Figure, LV, mallets, oom, pah } from "./band";
import { CH, type Changes, perBar } from "./harmony";
import { marimba, organ, pizz, woodblock } from "./instruments";
import { listen } from "./listen";
import { hz } from "./mix";
import { PAN, seat } from "./props";
import { ding, tick } from "./sounds";

const CHANGES: Changes = [
  ...perBar(CH.Fm, CH.Db, CH.Bbm, CH.C, CH.Db, CH.Fm, CH.Bbm),
  [26, CH.C],
];

/** The last bar, where the marimba rests into the ticket. */
const LAST = 24;

/** The patter: chord tones in eighths, the top one saved for the bar's end. */
export const PATTER: Figure = [
  0,
  null,
  2,
  null,
  1,
  null,
  2,
  null,
  0,
  null,
  2,
  null,
  1,
  null,
  3,
  null,
];

export const part: Part = {
  level: 0.8,
  cues(m, s, facts) {
    const c = listen("pitch", s, facts);
    const [tools, env, tasks] = c.all("tables");
    if (tools !== undefined)
      pizz(m, tools, hz(77), 0.08, PAN.card, 0.3, { bus: "sfx", len: 0.6 });
    if (env !== undefined)
      organ(m, env, env + 0.22, [80], 0.05, {
        bus: "sfx",
        speed: "fast",
        release: 0.2,
        sub: 0,
      });
    if (tasks !== undefined) {
      woodblock(m, tasks, 1250, 0.12, PAN.card, 0.12, "sfx");
      marimba(m, tasks + 0.004, hz(84), 0.09, PAN.card, { bus: "sfx" });
    }
    for (const t of c.all("cd")) tick(m, t, 2600, 0.32, PAN.term, 0.1);
    for (const t of c.all("ci")) {
      woodblock(m, t, 1250, 0.07, PAN.term - 0.1, 0.1, "sfx");
      pizz(m, t + 0.05, hz(72), 0.05, PAN.term, 0.2, { bus: "sfx" });
      marimba(m, t + 0.1, hz(77), 0.06, PAN.term + 0.1, { bus: "sfx" });
      ding(m, t + 0.15, hz(84), 0.025, PAN.term, 1, 0.35);
    }
    for (const t of c.all("lands")) seat(m, t, 1.2);
  },
  bass: (m, s) => oom(m, s, CHANGES, { level: LV.bass }),
  pads(m, s) {
    pah(m, s, CHANGES, { vel: LV.pah, brass: 0, to: 4 });
    pah(m, s, CHANGES, { vel: LV.pah, brass: LV.brass, from: 4 });
    mallets(m, s, CHANGES, PATTER, {
      from: 4,
      to: LAST,
      vel: 0.8 * LV.mallet,
    });
    cello(m, s, CHANGES);
  },
};
