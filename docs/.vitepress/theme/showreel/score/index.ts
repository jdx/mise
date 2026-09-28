// from jdx/hk@37937824 docs/.vitepress/theme/showreel/score/index.ts
// The score, section by section. Each section's share lives in its own
// module named for its section id (the seven tickets share tickets.ts),
// written from the section's origin (`sec(id)`), so lengthening or
// inserting a section moves its sounds with it.
//
// Adapted for mise: 60 BPM in F minor, quoting the chorus of jdx's
// "mise-en-place" with no voice (motif.ts), arranged act by act as plan v3
// §5 sets out (band.ts, instruments.ts), with a sound for every sync point
// (props.ts). From the morph's held B♭ minor the recording takes over: half
// a second before the end card's downbeat it enters under the chord, which
// is gone by SCORE_END
// (score/song.ts), and from there the end card's recording is the only
// sound, mixed in at mux time.
//
// Cues for scene builders
// -----------------------
//
// The picture tells the score when its events happen. A scene module
// (scenes/<id>.ts) exports `cues`, named times in its own section beats:
//
//   import type { SceneCues } from "../score/cues";
//   export const cues: SceneCues<"use"> = { clear: CLEAR, seat: SEAT };
//
// or a function of the facts it draws with, for times read off a take:
//
//   export const cues: SceneCues<"switch"> = (facts) => {
//     const c = capture(reelData(facts), "C7");
//     return c ? { cdDashboard: beatOf(plays(c), stepOf(c, "cd-dashboard").enter) } : {};
//   };
//
// A value is one beat, a list (the same event several times: each lane
// line, each `linked` row), or null (this take has no such event). The
// names each section listens for, and what each marks, are LISTEN in
// score/cues.ts; SceneCues is typed from it, so an unknown name fails the
// typecheck. A name a scene leaves out falls back to LISTEN's value: the
// kit's constant for a kit event, the beat the scene drew it on when the
// score was written, or the storyboard's caption anchor or grid beat
// (sections.json); null waits for the scene. Ticket scenes take their
// feed, tear and lift from kit/style.ts TICKET.cue unless they say
// otherwise. The score reads the
// modules through score/scenes.ts; a scene imports from score/ only the
// cue types and score/song.ts (the end card's onsets), so there is no
// cycle. A part reads its section's cues with listen() (score/listen.ts).

import type { ReelFacts } from "../bible";
import { BEAT, SECTIONS, type Section, type SectionId, sec } from "../timeline";
import { part as args } from "./args";
import { part as backends } from "./backends";
import { part as bootstrap } from "./bootstrap";
import { part as breath } from "./breath";
import { part as clone } from "./clone";
import { part as daemons } from "./daemons";
import { part as depends } from "./depends";
import { part as lock } from "./lock";
import type { Mix, Pt } from "./mix";
import { part as morph } from "./morph";
import { part as open } from "./open";
import { part as packslip } from "./packslip";
import { part as pitch } from "./pitch";
import { part as redact } from "./redact";
import { part as registry } from "./registry";
import { part as skip } from "./skip";
import { part as switchPart } from "./switch";
import { TICKETS } from "./tickets";
import { part as track } from "./track";
import { part as use } from "./use";
import { part as vars } from "./vars";

/** One layer of a section's share: the mix, the section, and the facts the picture draws. */
type Layer = (m: Mix, s: Section, facts: ReelFacts | null) => void;

/** One section's share of the score. Every layer is optional. */
export interface Part {
  /**
   * The groove's fader for the section (the drums and music buses; the cues
   * keep their own levels), 1 by default. Together the sections' levels are
   * the reel's loudness arc: the song's own build of about 10 dB, with
   * Act III quieter, the valley before the clone near silent, and the
   * climax the peak.
   */
  level?: number;
  /** Sound design: a sound for each accent the section's picture choreographs. */
  cues?: Layer;
  /** The drums, and the kicks the bass and the pads pump with. */
  drums?: Layer;
  bass?: Layer;
  /** The tune. */
  lead?: Layer;
  /** The chords, pads and the band's figures. */
  pads?: Layer;
}

/** The end card's part: nothing; the recording plays there (score/song.ts). */
const RECORDING: Part = {};

/** Every section's part. A new section needs an entry, even an empty one. */
export const PARTS: Record<SectionId, Part> = {
  open,
  pitch,
  tools: TICKETS.tools,
  use,
  registry,
  backends,
  versions: TICKETS.versions,
  switch: switchPart,
  packslip,
  env: TICKETS.env,
  vars,
  redact,
  tasks: TICKETS.tasks,
  depends,
  skip,
  args,
  daemons,
  dotfiles: TICKETS.dotfiles,
  track,
  machines: TICKETS.machines,
  lock,
  new: TICKETS.new,
  bootstrap,
  breath,
  clone,
  morph,
  end: RECORDING,
};

/**
 * The groove's fader over the whole reel, from each section's level: it
 * moves over the half beat before a bar line, so the new level arrives
 * with the downbeat.
 */
export function arc(): Pt[] {
  const pts: Pt[] = [[0, PARTS[SECTIONS[0].id].level ?? 1]];
  for (let i = 1; i < SECTIONS.length; i++) {
    const from = PARTS[SECTIONS[i - 1].id].level ?? 1;
    const to = PARTS[SECTIONS[i].id].level ?? 1;
    if (from === to) continue;
    const t = sec(SECTIONS[i].id).start;
    pts.push([t - BEAT / 2, from], [t, to]);
  }
  return pts;
}

/** The layers, built in this order: the effects first, then the music under them. */
const LAYERS = ["cues", "drums", "bass", "lead", "pads"] as const;

/** Build one pass of the whole score into `m`. */
export function compose(m: Mix, facts: ReelFacts | null = null): void {
  for (const layer of LAYERS) {
    for (const { id } of SECTIONS) PARTS[id][layer]?.(m, sec(id), facts);
  }
}
