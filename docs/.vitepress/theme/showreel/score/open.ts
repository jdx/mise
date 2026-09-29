// Open: `mise` is typed and its help screen prints beside the block chef.
// Bar 1 is the song's own intro, a dominant held and swelling: a C7 pad
// over a low C from the first frame, the brass swelling in as the chef's
// cells lift, tiny bells glinting up F minor as they fly. The chef resolves
// on beat 4, and so does the harmony: M1 lands its F there on the reed,
// its pickup on the and of 3 (the song's "In SHORT"), the service bell
// rings for the name card, and the groove enters under it, the bass and
// the pizzicato's off-beats on F minor. As "mise-en-place" writes on, a
// syllable a quarter beat, the celesta plays the name the way the end card
// sings it, C D♭ C, so the end card's line answers the open. At 60 BPM M1
// closes on its C a bar before the card leaves, so the band turns under
// the exit instead: B♭ minor from bar 3, the cello bowing F down to E, and
// the closing 2/4 bar on C, the dominant the pitch's F minor answers. The
// card's leave has no sound of its own.

import type { Part } from ".";
import { BEAT } from "../timeline";
import { cello, LV, oom, pah } from "./band";
import { CH } from "./harmony";
import { brass, celesta, reedLine } from "./instruments";
import { listen } from "./listen";
import { hz } from "./mix";
import { M1, onReel } from "./motif";
import { PAN, serviceBell } from "./props";
import { bassRun, pad, shimmer } from "./sounds";

/** The intro's dominant: C3 G3 B♭3 E4. */
const C7 = [48, 55, 58, 64];
/** F minor under the name card, with F3 at the bottom. */
const FM = [53, 56, 60, 65];
/** Under the leave, the same pad on B♭ minor, then C: F3 B♭3 D♭4 F4, E3 G3 C4 E4. */
const BBM = [53, 58, 61, 65];
const C = [52, 55, 60, 64];
/** Where the band turns under the exit: bar 3, and the closing 2/4 bar. */
const TURN = 8;
const HOME = 12;

/**
 * The name as the end card sings it: "mise-en-place" on C D♭ C (the
 * chorus's "It's mise-en-place", motif.ts CHORUS e 96 to 100), two octaves
 * up over the reed's reciting C.
 */
const NAME = [84, 85, 84];
/** The name card stands at the left of the stage, the chef at the right. */
const CARD = -0.3;

/** Bar 1 holds the dominant; the chef's resolve brings F minor; the exit turns iv, V. */
const changes = (resolve: number) =>
  [
    [0, CH.C7],
    [resolve, CH.Fm],
    [TURN, CH.Bbm],
    [HOME, CH.C],
  ] as const;

export const part: Part = {
  level: 0.75,
  cues(m, s, facts) {
    const c = listen("open", s, facts);
    const lift = c.or("lift", 2.5);
    const resolve = c.or("resolve", 4);
    // The cells fly off the help screen to the chef: tiny bells up F minor.
    shimmer(m, lift + 0.1, resolve - 0.06, 0.028, PAN.card);
    // The name card lands with the chef: the kitchen's bell.
    serviceBell(m, resolve, 0.045, PAN.card);
    c.all("name").forEach((t, i) =>
      celesta(m, t, hz(NAME[i % 3]), 0.05, CARD, 1.2, 0.4, "sfx"),
    );
  },
  bass(m, s, facts) {
    const c = listen("open", s, facts);
    const resolve = c.or("resolve", 4);
    // The low C under the intro, held and swelling a little.
    bassRun(m, [[s.start + 0.05, resolve - 0.03, 36, 1]], 0.04, 300, 0.4);
    const b = (resolve - s.start) / BEAT;
    oom(m, s, changes(b), {
      from: Math.ceil(b),
      level: 0.85 * LV.bass,
    });
  },
  lead(m, s, facts) {
    const resolve = listen("open", s, facts).or("resolve", 4);
    reedLine(m, onReel(M1, resolve), LV.lead);
  },
  pads(m, s, facts) {
    const c = listen("open", s, facts);
    const lift = c.or("lift", 2.5);
    const resolve = c.or("resolve", 4);
    pad(m, s.start + 0.02, resolve - 0.02, C7, 0.045, 1600, 1.8, 0.12);
    // The brass swells from the lift into the resolve and lets go on it.
    brass(m, lift, resolve - 0.01, [55, 58, 60, 64], 0.05, {
      attack: Math.max(0.1, resolve - lift - 0.12),
      sustain: 1,
      release: 0.06,
      bright: 2200,
      pan: 0.12,
    });
    pad(m, resolve, s.beat(TURN) - 0.02, FM, 0.035, 1400, 0.1, 0.3);
    pad(m, s.beat(TURN), s.beat(HOME) - 0.02, BBM, 0.035, 1400, 0.3, 0.3);
    pad(m, s.beat(HOME), s.end - 0.05, C, 0.035, 1400, 0.3, 0.3);
    const b = (resolve - s.start) / BEAT;
    pah(m, s, changes(b), { from: b, vel: 0.8 * LV.pah });
    cello(m, s, changes(b), { from: TURN, vel: 0.9 * LV.cello });
  },
};
