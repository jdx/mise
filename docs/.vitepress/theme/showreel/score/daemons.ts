// daemons: a task declares the daemons it needs, and `mise run db` starts
// Postgres and waits until it is ready. The lane voices over F minor, then
// the dominant from bar 2 as the task's card is read and `mise run db` is
// typed and waits on `pg_isready`: only lint's woodblock ticks, in eighths,
// like the spinner, over a held C. When Postgres starts the pilot lights,
// and an organ drone on F and C comes in under everything (plan v3 §5) and
// holds through the section, the voices back over F minor for a bar, then
// B♭ minor and D♭ under the last caption and the long rest into the
// Dotfiles ticket, in eighths from there.
//
// The picture's cues: `cd ../shop` ticks; `✔ … started` is the pilot
// light's whump; and the thread tying the server's version to
// `postgres = "18"` clicks.

import type { Part } from ".";
import type { ReelFacts } from "../bible";
import type { Section } from "../timeline";
import { LV, oom, type Rests, trio } from "./band";
import { CH, type Changes } from "./harmony";
import { organ } from "./instruments";
import { beatIn, listen } from "./listen";
import { PAN, seat, whump } from "./props";
import { bassRun, tick } from "./sounds";

/** When the run starts waiting, and when Postgres is up, in section beats. */
function waits(s: Section, facts: ReelFacts | null) {
  const c = listen("daemons", s, facts);
  const run = c.at("run");
  const started = c.at("started");
  return {
    run: run === null ? 4 : beatIn(s, run),
    started: started === null ? null : beatIn(s, started),
  };
}

function changes(s: Section, facts: ReelFacts | null): Changes {
  const { run, started } = waits(s, facts);
  const out: [number, (typeof CH)[keyof typeof CH]][] = [
    [0, CH.Fm],
    [Math.min(run, 4), CH.C],
  ];
  if (started !== null) out.push([started, CH.Fm]);
  // B♭ minor a bar after the flame at the earliest, D♭ in the last bar.
  const iv = Math.max(12, Math.ceil(((started ?? run) + 3) / 4) * 4);
  if (iv < s.beats - 2) out.push([iv, CH.Bbm]);
  out.push([Math.max(iv + 2, 16), CH.Db]);
  return out.filter(([b]) => b < s.beats).sort((a, b) => a[0] - b[0]);
}

/** Where the figures move in eighths: the wait, and from the last caption on. */
const calm = (s: Section, wait: number, up: number): Rests => [
  [wait, up],
  [Math.max(up, 12), s.beats],
];

export const part: Part = {
  level: 1.05,
  cues(m, s, facts) {
    const c = listen("daemons", s, facts);
    for (const t of c.all("cd")) tick(m, t, 2600, 0.32, PAN.term, 0.1);
    for (const t of c.all("started")) whump(m, t, 1, PAN.card);
    for (const t of c.all("tie")) seat(m, t, 0.7);
  },
  bass(m, s, facts) {
    const { run, started } = waits(s, facts);
    const wait = Math.min(run, 4);
    const up = started ?? s.beats;
    oom(m, s, changes(s, facts), { level: LV.bass, rests: [[wait, up]] });
    // The held C while it waits.
    if (up > wait)
      bassRun(
        m,
        [[s.beat(wait), s.beat(up) - 0.03, CH.C.root, 0.8]],
        0.05,
        320,
        0.45,
      );
  },
  pads(m, s, facts) {
    const ch = changes(s, facts);
    const { run, started } = waits(s, facts);
    const wait = Math.min(run, 4);
    const up = started ?? s.beats;
    trio(m, s, ch, {
      test: LV.test,
      build: LV.build,
      rests: [[wait, up]],
      calm: calm(s, wait, up),
    });
    trio(m, s, ch, { lint: LV.lint, calm: calm(s, wait, up) });
    // The drone swells in under the flame, into the ticket: slowly, so the
    // whump is heard alone first rather than on the drone's attack.
    if (started !== null)
      organ(m, s.beat(started), s.end + 0.5, [41, 48, 53], 0.05, {
        speed: "slow",
        sub: 0.6,
        bright: 1800,
        attack: 0.8,
        release: 0.6,
      });
  },
};
