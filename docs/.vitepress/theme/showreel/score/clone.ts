// clone: on a fresh clone, `mise run ci` installs what mise.lock pins, then
// its tasks run with the project's env vars. The climax (plan v3 §5).
//
// The intro is the open's again, the dominant rising out of the breath,
// three and a half bars of it at 60 BPM: a filtered pulse on C opening up, in eighths
// while `mise run ci` is typed and in sixteenths from its slam; from the
// slam the open's C7 pad and low C under it, the brass swelling in over the
// install into the chorus's downbeat, and a snare roll into it. Then
// the chorus itself, bars 1 to 8, one chord per two beats as the song has
// them (F minor, B♭ minor, E♭, A♭, C: bar 8 is C whole, under the reed's
// E, as the song has it), with the whole kit and the
// bass from its downbeat, and M1 on the reed over its first two bars, where
// the song sings it. The lead rests while the lanes and the big type carry
// the picture, and comes back for bars 7 and 8 on the song's own line,
// "that simply starts for you", whose pickup lands F on the morph's
// downbeat. Each table that lights brings its act's instruments back:
// [tools] the pizzicato, brass and marimba, [tasks] the lane voices' figures,
// [env] the organ.
//
// The picture's cues: `mise run ci` slams in as big type on the dominant;
// each lane appears on its voice, ci's on all three; the card folds to its
// headers on air and closes with a click as [tasks] lights; the `[ci] api
// ready` line lands with the kitchen's bell, rung twice; and its pieces
// land home in their tables with a click each and the table's own voice
// from the pitch (the marimba for [tasks], the pizzicato for [tools], the
// organ for [env]), rising up the chord they land on (chorus bar 7's A♭ in
// this take, bar 8's C where they land later).

import type { Part } from ".";
import type { ReelFacts } from "../bible";
import { DUR } from "../kit/style";
import { BEAT, type Section, sec } from "../timeline";
import { kit, LV, mallets, oom, organBars, pah, pulse, trio } from "./band";
import { CH, type Changes, chordAt } from "./harmony";
import {
  brass,
  marimba,
  organ,
  pizz,
  reedLine,
  snare,
  woodblock,
} from "./instruments";
import { ciIn, type Lane, laneIn } from "./lanes";
import { beatIn, listen } from "./listen";
import { hz, type Mix } from "./mix";
import { M1, onReel, STARTS } from "./motif";
import { air, PAN, seat, serviceBell, slam } from "./props";
import { PATTER } from "./pitch";
import { bassRun, pad } from "./sounds";

/**
 * Chorus bar 1 falls 16 beats before the morph, so bar 8's pickup lands F
 * on its downbeat: the clone's beat 4 when it was 20 beats, its beat 14 at
 * the retime's 30 (the intro's dominant pulse stretches to fill).
 */
export const CHORUS_AT = sec("clone").beats - 16;

const CHANGES: Changes = [
  [0, CH.C7],
  [CHORUS_AT, CH.Fm],
  [CHORUS_AT + 4, CH.Bbm],
  [CHORUS_AT + 8, CH.Eb],
  [CHORUS_AT + 12, CH.Ab],
  [CHORUS_AT + 14, CH.C],
];

/**
 * A home landing, by the table it lands in (LISTEN.clone.home's order): its
 * voice, on the chord's tones two octaves over its middle voicing, rising.
 */
const HOME_VOICES = [
  (m: Mix, t: number, n: number) => {
    woodblock(m, t, 1250, 0.14, PAN.card, 0.12, "sfx");
    marimba(m, t + 0.004, hz(n), 0.11, PAN.card, { bus: "sfx" });
  },
  (m: Mix, t: number, n: number) =>
    pizz(m, t, hz(n), 0.1, PAN.card, 0.3, { bus: "sfx", len: 0.6 }),
  (m: Mix, t: number, n: number) =>
    organ(m, t, t + 0.22, [n], 0.06, {
      bus: "sfx",
      speed: "fast",
      release: 0.2,
      sub: 0,
    }),
] as const;

/** The intro's dominant under the slam, the open's: C3 G3 B♭3 E4. */
const C7 = [48, 55, 58, 64];
/** The brass swells in over the last bar of the intro. */
const SWELL = 3;

/** Where a table's light brings its instruments back, in section beats. */
function lights(s: Section, facts: ReelFacts | null) {
  const c = listen("clone", s, facts);
  const at = (name: "tools" | "tasks" | "env", or: number) =>
    Math.max(CHORUS_AT, beatIn(s, c.or(name, or)));
  return {
    tools: at("tools", 4.5),
    tasks: at("tasks", 7),
    env: at("env", 10.5),
  };
}

const LANES: Lane[] = ["test", "lint", "build"];

export const part: Part = {
  level: 1.1,
  cues(m, s, facts) {
    const c = listen("clone", s, facts);
    for (const t of c.all("slam"))
      slam(m, t, chordAt(CHANGES, beatIn(s, t)).mid, 1.1, PAN.term);
    for (const lane of LANES)
      for (const t of c.all(lane))
        laneIn(m, t, lane, chordAt(CHANGES, beatIn(s, t) + 1e-3), 0.9);
    for (const t of c.all("ci")) ciIn(m, t, chordAt(CHANGES, beatIn(s, t)), 0);
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
      const tones = chordAt(CHANGES, beatIn(s, t) + 1e-3).high;
      HOME_VOICES[i % 3](m, t, tones[i % 3] + 12);
    });
  },
  drums(m, s) {
    // The roll into the chorus, sixteenths then 32nds, growing.
    for (let k = 0; k < 12; k++) {
      const b = CHORUS_AT - 2 + (k < 4 ? k / 4 : 1 + (k - 4) / 8);
      snare(m, s.beat(b), 0.12 + 0.04 * k);
    }
    kit(m, s, { from: CHORUS_AT, vel: 1, hats: 0.7 });
  },
  bass(m, s, facts) {
    // The low C under the intro from the slam, as under the open's.
    const slamAt = listen("clone", s, facts).at("slam");
    const chorus = s.beat(CHORUS_AT);
    if (slamAt !== null && slamAt < chorus - BEAT)
      bassRun(m, [[slamAt + 0.05, chorus - 0.03, 36, 1]], 0.04, 300, 0.4);
    oom(m, s, CHANGES, { from: CHORUS_AT, level: 1.05 * LV.bass });
  },
  lead(m, s) {
    const e0 = s.beat(CHORUS_AT);
    reedLine(m, onReel(M1, e0), 1.1 * LV.lead);
    reedLine(m, onReel(STARTS, e0), 1.05 * LV.lead);
  },
  pads(m, s, facts) {
    // The dominant rising out of the breath: the pulse on C, opening, in
    // eighths until the slam and sixteenths from it.
    const slamAt = listen("clone", s, facts).at("slam");
    const slam = slamAt === null ? CHORUS_AT / 2 : beatIn(s, slamAt);
    pulse(m, s, {
      note: 36,
      to: CHORUS_AT,
      vel: 0.05,
      cutoff: (b) => 320 * 8 ** (b / CHORUS_AT),
      calm: [[0, slam]],
    });
    const chorus = s.beat(CHORUS_AT);
    if (slam < CHORUS_AT - 1) {
      pad(m, s.beat(slam), chorus - 0.02, C7, 0.04, 1600, 1.5, 0.12);
      const from = s.beat(Math.max(slam, CHORUS_AT - SWELL));
      brass(m, from, chorus - 0.01, [55, 58, 60, 64], 0.05, {
        attack: Math.max(0.1, chorus - from - 0.12),
        sustain: 1,
        release: 0.06,
        bright: 2200,
        pan: 0.12,
      });
    }
    const on = lights(s, facts);
    pah(m, s, CHANGES, { from: on.tools, vel: LV.pah, brass: 1.2 * LV.brass });
    mallets(m, s, CHANGES, PATTER, { from: on.tools, vel: 0.8 * LV.mallet });
    trio(m, s, CHANGES, {
      from: on.tasks,
      lint: LV.lint,
      build: 0.8 * LV.build,
    });
    organBars(m, s, CHANGES, {
      from: on.env,
      vel: 0.8 * LV.organ,
      speed: "fast",
    });
  },
};
