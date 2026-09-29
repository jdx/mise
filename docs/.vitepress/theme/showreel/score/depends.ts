// depends: `ci` depends on lint, test and build, which can run in
// parallel. Act IV is a voice per lane (plan v3 §5). Under the command only
// the bass and the brushes on F minor; the three lanes appear close
// together on the dominant, and ci's lane resolves them to i.
// Then the song's verse turns once round, iv, V, i, as the lanes are read
// and settle into [tasks]: F minor, C, F minor | F minor | B♭ minor, C |
// F minor, the settle landing home.
// From each lane's entrance its voice keeps its figure (lint's woodblocks
// chopping sixteenths, test's pizzicato off-beats, build's marimba run)
// while the lanes fly in and run, and in eighths from bar 3, where they
// hold still to be read.
//
// The picture's cues: each lane's voice on its entrance, all three and the
// bell on ci's, and a click as the lanes settle into [tasks], which lights.

import type { Part } from ".";
import type { ReelFacts } from "../bible";
import type { Section } from "../timeline";
import { brushes, LV, oom, trio } from "./band";
import { CH, type Changes, type Chord, chordAt } from "./harmony";
import { ciIn, type Lane, laneIn } from "./lanes";
import { beatIn, listen } from "./listen";
import { seat } from "./props";

const LANES: Lane[] = ["lint", "test", "build"];

/** Each lane's entrance (section beats), in the order they appear, and ci's. */
function entrances(s: Section, facts: ReelFacts | null) {
  const c = listen("depends", s, facts);
  const lanes = LANES.map((lane) => ({ lane, t: c.at(lane) }))
    .filter((l): l is { lane: Lane; t: number } => l.t !== null)
    .sort((a, b) => a.t - b.t);
  return { lanes, ci: c.at("ci") };
}

/** Where the lanes hold still to be read, and where the verse turns: iv, then V, then home (the settle). */
const READ = 8;
const IV = 12;
const V = 14;
const HOME = 16;

/** A cue's beat, to the nearest sixteenth: a change a frame after a beat still lands on it. */
const near = (s: Section, t: number): number =>
  Math.round(beatIn(s, t) * 4) / 4;

/**
 * i until the first lane, V until ci, then i; then iv, V, i on the grid.
 * ci's lane lands a frame after bar 2's downbeat in the take, and the
 * bass's root has to be there with it.
 */
function changes(s: Section, facts: ReelFacts | null): Changes {
  const { lanes, ci } = entrances(s, facts);
  const out: [number, Chord][] = [[0, CH.Fm]];
  const first = lanes.length ? near(s, lanes[0].t) : null;
  const home = ci === null ? null : near(s, ci);
  if (first !== null && first < READ) out.push([first, CH.C]);
  if (home !== null && home < READ) out.push([home, CH.Fm]);
  out.push([IV, CH.Bbm], [V, CH.C], [HOME, CH.Fm]);
  return out.sort((a, b) => a[0] - b[0]);
}

export const part: Part = {
  level: 1.05,
  cues(m, s, facts) {
    const ch = changes(s, facts);
    const { lanes, ci } = entrances(s, facts);
    for (const { lane, t } of lanes)
      laneIn(m, t, lane, chordAt(ch, beatIn(s, t) + 1e-3));
    if (ci !== null) ciIn(m, ci, CH.Fm);
    for (const t of listen("depends", s, facts).all("settle"))
      seat(m, t + 0.3, 0.8);
  },
  drums: (m, s) => brushes(m, s, { vel: 0.7 * LV.swirl, taps: 0.8 * LV.taps }),
  bass: (m, s, facts) => oom(m, s, changes(s, facts), { level: LV.bass }),
  pads(m, s, facts) {
    const ch = changes(s, facts);
    const { lanes } = entrances(s, facts);
    for (const { lane, t } of lanes) {
      // A lane's figure starts on the next sixteenth after it appears.
      const from = Math.ceil(beatIn(s, t) * 4 + 1e-3) / 4;
      trio(m, s, ch, { from, [lane]: LV[lane], calm: [[READ, s.beats]] });
    }
  },
};
