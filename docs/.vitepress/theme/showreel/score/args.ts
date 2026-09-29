// args: file tasks take `#USAGE` args, and bad input stops the task before
// it runs. The lane voices over F minor | B♭ minor | D♭ | C, all three
// again, chopping sixteenths as the card slides in and the bad input is
// typed, and in eighths while `--help` is read. The `mise ERROR` lines stop
// them too: a dissonant brass cluster on the impact, the band cut dead
// under it, and nothing but its ring for about two beats, to the next half
// bar, where the groove picks back up in eighths on the dominant; B♭ minor
// carries the caption's read, and the dominant turns it to the daemons.
//
// The picture's cues: the `#USAGE` lines light with a pluck; `--help`
// prints with a light tick, as the reel's printed lines do; the error is
// the stab; and the thread ties `prod` to the card's `choices` with a soft
// click.

import type { Part } from ".";
import type { ReelFacts } from "../bible";
import type { Section } from "../timeline";
import { LV, oom, type Rests, trio } from "./band";
import { CH, perBar } from "./harmony";
import { pizz } from "./instruments";
import { beatIn, listen } from "./listen";
import { hz } from "./mix";
import { errorStab, PAN, seat } from "./props";
import { tick } from "./sounds";

const CHANGES = perBar(CH.Fm, CH.Bbm, CH.Db, CH.C, CH.Bbm, CH.C);

/**
 * From the error the band rests while the stab rings, to the first half
 * bar at least a beat and a half on: about two beats, never a bar and more
 * of silence at 60 BPM.
 */
function rests(s: Section, facts: ReelFacts | null): Rests {
  const e = listen("args", s, facts).at("error");
  if (e === null) return [];
  const b = beatIn(s, e);
  return [[b - 0.05, Math.ceil((b + 1.5) / 2) * 2]];
}

/**
 * Where the figures move in eighths: from `--help`'s print until the bad
 * input is typed (bar 3's second half), and after the error's rest.
 */
function calm(s: Section, facts: ReelFacts | null): Rests {
  const help = listen("args", s, facts).at("help");
  const [rest] = rests(s, facts);
  const from = help === null ? 4 : beatIn(s, help);
  return [
    [from, Math.max(from, 10)],
    [rest ? rest[1] : 16, s.beats],
  ];
}

export const part: Part = {
  level: 1.05,
  cues(m, s, facts) {
    const c = listen("args", s, facts);
    for (const t of c.all("usage"))
      pizz(m, t, hz(72), 0.06, PAN.card, 0.3, { bus: "sfx" });
    for (const t of c.all("help")) tick(m, t, 2400, 0.22, PAN.term, 0.1);
    for (const t of c.all("error")) errorStab(m, t, 1, PAN.term);
    for (const t of c.all("tie")) seat(m, t, 0.6, 0.1);
  },
  bass: (m, s, facts) =>
    oom(m, s, CHANGES, { level: LV.bass, rests: rests(s, facts) }),
  pads: (m, s, facts) =>
    trio(m, s, CHANGES, {
      lint: LV.lint,
      test: LV.test,
      build: LV.build,
      rests: rests(s, facts),
      calm: calm(s, facts),
    }),
};
