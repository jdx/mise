// switch: `cd` between two projects, and each project's own `node`. The
// one key change in the reel (plan v3 §5): `cd ../dashboard` moves the band
// to D♭, the song's ♭VI, and `cd ../api` brings it home to F minor. The
// two keys share F and A♭, so only D♭ and C trade places.
//
// In D♭ the band plays I | IV | I, D♭ | G♭ | D♭, a bar each where the
// dashboard's lines are read, and comes home by the step both keys share,
// D♭ to F minor, on `cd ../api`.
//
// The picture's cues: each `cd` is a marimba run up its new key's chord as
// the cwd dot hops; and each project's `node --version` prints on M1's
// head in that project's key, A♭ → D♭ for dashboard, C → F for api. Home
// holds through the poster frame and the threads, and B♭ minor and C turn
// in the closing 2/4 bar toward the packslip.

import type { Part } from ".";
import { cello, LV, oom, pah } from "./band";
import { CH, type Changes, type Chord } from "./harmony";
import { marimba } from "./instruments";
import { beatIn, listen } from "./listen";
import { hz, type Mix } from "./mix";
import { PAN } from "./props";
import type { Section } from "../timeline";
import type { ReelFacts } from "../bible";

/** In D♭, its IV from this bar line, and I again from the next. */
const IV = 8;
const I = 10;

/** The changes, the key following the two `cd`s. */
function changes(s: Section, facts: ReelFacts | null): Changes {
  const c = listen("switch", s, facts);
  const turn = s.beats - 2;
  const at = (t: number | null, or: number) => (t === null ? or : beatIn(s, t));
  const dash = at(c.at("cdDashboard"), turn);
  const api = Math.max(dash, at(c.at("cdApi"), turn));
  const out: [number, Chord][] = [[0, CH.Fm]];
  if (dash < api) {
    out.push([dash, CH.Db]);
    // G♭ and D♭ again only where each has a beat or more before `cd ../api`.
    if (dash <= IV - 1 && api >= IV + 1) out.push([IV, CH.Gb]);
    if (dash <= IV - 1 && api >= I + 1) out.push([I, CH.Db]);
    out.push([api, CH.Fm]);
  }
  out.push([turn, CH.Bbm], [turn + 1, CH.C]);
  return out.filter(([b]) => b <= turn + 1).sort((a, b) => a[0] - b[0]);
}

/** A run up a chord, a 32nd a note: the dot hopping to its folder. */
function run(m: Mix, t: number, notes: readonly number[], pan: number): void {
  notes.forEach((n, i) =>
    marimba(m, t + 0.1 * i, hz(n), 0.06 + 0.008 * i, pan, { bus: "sfx" }),
  );
}

/** M1's head in a key: the dominant a sixteenth before the tonic, which lands on `t`. */
function head(m: Mix, t: number, dominant: number, pan: number): void {
  marimba(m, t - 0.2, hz(dominant), 0.07, pan, { bus: "sfx" });
  marimba(m, t, hz(dominant + 5), 0.09, pan, { bus: "sfx", len: 0.9 });
}

/** The folders sit in the card column, dashboard below api. */
const FOLDERS = PAN.card;

export const part: Part = {
  level: 0.8,
  cues(m, s, facts) {
    const c = listen("switch", s, facts);
    for (const t of c.all("cdDashboard")) run(m, t, [73, 77, 80, 85], FOLDERS);
    for (const t of c.all("dashboard")) head(m, t, 80, FOLDERS);
    for (const t of c.all("cdApi")) run(m, t, [77, 80, 84, 89], FOLDERS);
    for (const t of c.all("api")) head(m, t, 84, FOLDERS);
  },
  bass: (m, s, facts) => oom(m, s, changes(s, facts), { level: LV.bass }),
  pads(m, s, facts) {
    pah(m, s, changes(s, facts), { vel: LV.pah, brass: LV.brass });
    cello(m, s, changes(s, facts));
  },
};
