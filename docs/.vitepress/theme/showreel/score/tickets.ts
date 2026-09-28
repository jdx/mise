// The tickets: each act opens on one 2/4 bar, a kitchen order ticket that
// prints on the rail, tears, hangs and lifts away (ART.md §10). The sound
// follows the paper: a printer tick for each of the feed's twelve steps
// (kit/style.ts TICKET), the tear, and air as it lifts. The music turns
// with it: the band stops, the bass lands F, and the act's own voice plays
// a fragment of M1 whose F lands on the tear (plan v3 §5), alternating the
// head (C → F, "In SHORT") and the sigh (A♭ G F, "PEO-ple WHO"):
//
//   I Dev tools      pizzicato     head
//   II Versions      muted brass   sigh
//   III Environments organ         head
//   IV Tasks         marimba       sigh, a woodblock on the tear
//   V Dotfiles       tape-worn electric piano, head
//   VI Everywhere    melodica      sigh
//   VII A new machine celesta      head, two octaves up, in the valley
//
// A ticket scene may export `print`, `tear` and `lift` (score/cues.ts).

import type { Part } from ".";
import { TICKET } from "../kit/style";
import type { Section } from "../timeline";
import { CH } from "./harmony";
import {
  brass,
  celesta,
  epiano,
  marimba,
  melodica,
  organ,
  pizz,
  woodblock,
} from "./instruments";
import { listen } from "./listen";
import { hz, type Mix } from "./mix";
import { HEAD, type HookNote, landing, onReel, SIGH } from "./motif";
import { air, feed, PAN, tear } from "./props";
import { bassRun, type Note } from "./sounds";

type TicketId =
  "tools" | "versions" | "env" | "tasks" | "dotfiles" | "machines" | "new";

interface Spec {
  fragment: readonly HookNote[];
  /** Semitones above the transcription's register. */
  transpose: number;
  /** The act's voice, playing the fragment's notes. */
  play: (m: Mix, notes: Note[]) => void;
  /** Under it, on the same voice, if it holds a chord. */
  under?: (m: Mix, t0: number, t1: number) => void;
  /** Where the ticket hangs: its stereo position. */
  pan: number;
  level?: number;
}

/** The fragment's last note, the F: where it lands on the tear. */
const lastE = (f: readonly HookNote[]): number => f[f.length - 1][0];

const SPECS: Record<TicketId, Spec> = {
  tools: {
    fragment: HEAD,
    transpose: 12,
    pan: PAN.center,
    play: (m, notes) =>
      notes.forEach(([a, , n, v]) => {
        pizz(m, a, hz(n), 0.1 * v, 0, 0.3, { len: 0.7 });
        pizz(m, a + 0.003, hz(n - 12), 0.06 * v, -0.1, 0.25, { len: 0.7 });
      }),
  },
  versions: {
    fragment: SIGH,
    transpose: 0,
    pan: PAN.center,
    play: (m, notes) =>
      notes.forEach(([a, b, n, v]) =>
        brass(m, a, b, [n], 0.075 * v, {
          mute: true,
          attack: 0.03,
          sustain: 0.8,
          release: 0.1,
          pan: 0.05,
        }),
      ),
  },
  env: {
    fragment: HEAD,
    transpose: 12,
    pan: PAN.center,
    play: (m, notes) =>
      notes.forEach(([a, b, n, v]) =>
        organ(m, a, b, [n], 0.11 * v, { speed: "slow", sub: 0.2 }),
      ),
    under: (m, t0, t1) =>
      organ(m, t0, t1, CH.Fm.mid, 0.06, { speed: "slow", click: false }),
  },
  tasks: {
    fragment: SIGH,
    transpose: 12,
    pan: PAN.center,
    play: (m, notes) =>
      notes.forEach(([a, , n, v], i) => {
        marimba(m, a, hz(n), 0.2 * v, 0.1);
        if (i === notes.length - 1) woodblock(m, a, 1250, 0.1, -0.2);
      }),
  },
  dotfiles: {
    fragment: HEAD,
    transpose: 12,
    pan: PAN.center,
    play: (m, notes) =>
      notes.forEach(([a, , n, v]) => {
        epiano(m, a, hz(n), 0.085 * v, { wow: true, len: 1.5 });
        epiano(m, a + 0.002, hz(n - 12), 0.045 * v, { wow: true, len: 1.5 });
      }),
  },
  machines: {
    fragment: SIGH,
    transpose: 12,
    pan: PAN.center,
    play: (m, notes) =>
      notes.forEach(([a, b, n, v]) => melodica(m, a, b, hz(n), 0.11 * v)),
  },
  new: {
    fragment: HEAD,
    transpose: 24,
    pan: 0.52,
    level: 0.6,
    play: (m, notes) =>
      notes.forEach(([a, , n, v]) =>
        celesta(m, a, hz(n), 0.07 * v, 0.52, 1.8, 0.45),
      ),
  },
};

function ticket(id: TicketId): Part {
  const spec = SPECS[id];
  const cues = (s: Section, facts: Parameters<typeof listen>[2]) => {
    const c = listen(id, s, facts);
    return {
      print: c.or("print", 0),
      tear: c.or("tear", TICKET.cue.tear),
      lift: c.at("lift"),
    };
  };
  return {
    level: spec.level ?? 0.9,
    cues(m, s, facts) {
      const { print, tear: torn, lift } = cues(s, facts);
      feed(m, print, torn, TICKET.feedSteps, 0.12, spec.pan);
      tear(m, torn, 0.1, spec.pan);
      if (lift !== null)
        air(m, lift, lift + s.len * 0.3, 0.1, spec.pan, spec.pan, true);
    },
    bass(m, s) {
      bassRun(m, [[s.start, s.start + 0.7, CH.Fm.root, 1]], 0.04, 330, 0.45);
    },
    lead(m, s, facts) {
      const { tear: torn } = cues(s, facts);
      const e0 = landing(lastE(spec.fragment), torn);
      spec.play(m, onReel(spec.fragment, e0, 1, spec.transpose));
    },
    pads(m, s) {
      spec.under?.(m, s.start + 0.02, s.end - 0.12);
    },
  };
}

export const TICKETS: Record<TicketId, Part> = {
  tools: ticket("tools"),
  versions: ticket("versions"),
  env: ticket("env"),
  tasks: ticket("tasks"),
  dotfiles: ticket("dotfiles"),
  machines: ticket("machines"),
  new: ticket("new"),
};
