// packslip: hk's signed releases carry completions and agent skills. F
// minor | D♭ | B♭ minor while the old version's list is read; the upgrade
// brightens to the relative major on the card's flip, A♭ | E♭ under the new
// list, and B♭ minor turns it home: the skills link lands on F minor,
// where the chorus has it. D♭ | B♭ minor, C carry the last caption's read
// to the Environments ticket. The marimba rests from just before the link
// to the end, so the celesta speaks and the band thins toward Act III; the
// brass drops out for the closing bars.
//
// The picture's cues: `hk = "…"` lights on a pizzicato F; the slip slides
// up on air; each Tab keycap is a soft pop, and the new list's
// `--junit-xml` a brighter, higher pluck; the card flips to the new
// version with the click and the slip's chip re-stamps lightly; the
// `linked` rows tick; and the drawn link lights on the celesta's G G G F,
// the song's "the C-I both" (plan v3 §5).

import type { Part } from ".";
import type { ReelFacts } from "../bible";
import type { Section } from "../timeline";
import { cello, LV, mallets, oom, pah } from "./band";
import { CH, type Changes } from "./harmony";
import { celesta, pizz } from "./instruments";
import { beatIn, listen } from "./listen";
import { hz } from "./mix";
import { BOTH, landing, onReel } from "./motif";
import { air, PAN, seat, thunk } from "./props";
import { PATTER } from "./pitch";
import { pop, tick } from "./sounds";

/** The changes: the upgrade's A♭ on the card's flip (or bar 4's downbeat). */
function changes(s: Section, facts: ReelFacts | null): Changes {
  const flip = listen("packslip", s, facts).at("flip");
  const up = flip === null ? 12 : Math.min(15, Math.max(9, beatIn(s, flip)));
  return [
    [0, CH.Fm],
    [4, CH.Db],
    [8, CH.Bbm],
    [up, CH.Ab],
    [16, CH.Eb],
    [20, CH.Bbm],
    [24, CH.Fm],
    [28, CH.Db],
    [32, CH.Bbm],
    [34, CH.C],
  ];
}

/** The closing bars: the brass drops out. */
const CLOSE = 32;

/** The slip sits under the terminal, the Tab list in it. */
const SLIP = PAN.term;

export const part: Part = {
  level: 0.82,
  cues(m, s, facts) {
    const c = listen("packslip", s, facts);
    for (const t of c.all("light"))
      pizz(m, t, hz(77), 0.07, PAN.card, 0.3, { bus: "sfx", len: 0.6 });
    for (const t of c.all("slip")) air(m, t, t + 0.4, 0.07, SLIP, SLIP, true);
    for (const t of c.all("tab")) pop(m, t, hz(72), 0.18, SLIP, 0.15);
    for (const t of c.all("flip")) {
      seat(m, t);
      thunk(m, t + 0.05, 0.35, SLIP);
    }
    for (const t of c.all("junit")) {
      pop(m, t, hz(72), 0.18, SLIP, 0.15);
      pizz(m, t + 0.03, hz(87), 0.08, SLIP + 0.1, 0.35, {
        bus: "sfx",
        bright: 11,
        wobble: 0.006,
        len: 0.8,
      });
    }
    for (const t of c.all("linked")) tick(m, t, 3400, 0.2, SLIP, 0.1);
    for (const t of c.all("link")) {
      const e0 = landing(BOTH[0][0], t);
      for (const [a, , n, v] of onReel(BOTH, e0, 1, 12))
        celesta(m, a, hz(n), 0.08 * v, SLIP + 0.1, 1.5, 0.42, "sfx");
    }
  },
  bass: (m, s, facts) => oom(m, s, changes(s, facts), { level: LV.bass }),
  pads(m, s, facts) {
    const ch = changes(s, facts);
    const link = listen("packslip", s, facts).at("link");
    pah(m, s, ch, { vel: LV.pah, brass: LV.brass, to: CLOSE });
    pah(m, s, ch, { vel: 0.9 * LV.pah, from: CLOSE });
    mallets(m, s, ch, PATTER, {
      vel: 0.75 * LV.mallet,
      to: link === null ? CLOSE : beatIn(s, link) - 1,
    });
    cello(m, s, ch);
  },
};
