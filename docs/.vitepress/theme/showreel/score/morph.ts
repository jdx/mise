// morph: the card's lit headers become the toque's lobes and the toque
// settles on the chef. Chorus bars 9 to 12 (plan v3 §5), one chord per two
// beats, F minor, F minor, B♭ minor, B♭ minor: the reed sings "The laptop
// and the CI both become interoperable" with no words, F on the downbeat,
// the G G G F sigh from beat 1.5, D♭ recited from beat 4. The kit and the
// band play bars 9 and 10; from beat 4, as the toque drops, only the reed,
// a held B♭ in the bass and the organ are left; and bar 12 (beat 6) is the
// held B♭ minor alone, the organ's chord the recording's own organ plays
// as it enters under it half a second before the end card (score/song.ts).
// Nothing changes there but timbre and tempo.
//
// The picture's cues: the three headers lift off, glinting as the open's
// cells did; each settles over its lobe on the celesta, F, A♭, C, the notes
// its table lit on in the pitch; the toque's drop starts on the band's last
// kick, on the downbeat; and it touches the head with a puff and the
// kitchen's bell, as the chef blooms. Under the held chord a cymbal's air
// swells in, so the recording's top octaves, which come in with its first
// breath, do not switch on at the join: the organ's drawbars stop at 2.8
// kHz, and the recording's gap before "It's" does not.

import type { Part } from ".";
import { sec } from "../timeline";
import { kit, LV, oom, organBars, pah } from "./band";
import { CH, type Changes } from "./harmony";
import { celesta, kick, organ, reedLine } from "./instruments";
import { listen } from "./listen";
import { hold, hz } from "./mix";
import { LAPTOP, landing, onReel } from "./motif";
import { serviceBell } from "./props";
import { SCORE_END } from "./song";
import { bassRun, puff, shimmer, tick } from "./sounds";

/** Chorus bar 12 (beat 6): from here the score holds B♭ minor alone. */
export const HELD = sec("morph").beat(6);
/** The drums and the band stop here (beat 4), with the toque's drop. */
export const DRUMS_OUT = 4;
/** The held chord: B♭2 B♭3 D♭4 F4, the chord the recording enters on. */
export const HELD_CHORD = [46, 58, 61, 65];
/**
 * Its level, set so it meets the recording's entrance at the same loudness
 * (about -16.5 LUFS over the held bar against the line's -16.3), after the
 * master's makeup (audio.ts MAKEUP).
 */
const HELD_LEVEL = 0.205;
/**
 * The air over it at the join, as a peak: about -58 dBFS in the 4 kHz
 * octave and -68 in the 8 kHz, the recording's own gap before "It's".
 */
const HELD_AIR = 0.012;

const CHANGES: Changes = [
  [0, CH.Fm],
  [4, CH.Bbm],
];

/** The lobes sit over the chef's head, left to right: tools, env, tasks. */
const LOBES = [0.27, 0.36, 0.45];
/** Each lobe's note: its table's in the pitch, F A♭ C up the tonic chord. */
const LOBE_NOTES = [77, 80, 84];

export const part: Part = {
  level: 0.75,
  cues(m, s, facts) {
    const c = listen("morph", s, facts);
    const lands = c.all("lands");
    for (const t of c.all("lift"))
      shimmer(m, t, lands[0] ?? t + 1.5, 0.03, LOBES[1]);
    lands.forEach((t, i) => {
      celesta(
        m,
        t,
        hz(LOBE_NOTES[i % 3]),
        0.07,
        LOBES[i % 3],
        1.6,
        0.45,
        "sfx",
      );
      tick(m, t, 3000, 0.12, LOBES[i % 3], 0.12);
    });
    for (const t of c.all("drop")) kick(m, t, 0.8, false);
    for (const t of c.all("land")) {
      puff(m, t, 0.05, LOBES[1], false);
      serviceBell(m, t + 0.005, 0.045, LOBES[1]);
    }
  },
  drums: (m, s) => kit(m, s, { to: DRUMS_OUT - 0.25, vel: 0.9, hats: 0.6 }),
  bass(m, s) {
    oom(m, s, CHANGES, { to: DRUMS_OUT, level: LV.bass });
    bassRun(
      m,
      [[s.beat(DRUMS_OUT), HELD - 0.05, CH.Bbm.root - 12, 1]],
      0.06,
      300,
      0.7,
    );
  },
  lead(m, s) {
    reedLine(m, onReel(LAPTOP, landing(64, s.start)), 0.9 * LV.lead);
  },
  pads(m, s) {
    pah(m, s, CHANGES, { to: DRUMS_OUT, vel: 0.8 * LV.pah });
    organBars(m, s, CHANGES, { to: 6, vel: 0.9 * LV.organ, speed: "slow" });
    organ(m, HELD, SCORE_END + 0.08, HELD_CHORD, HELD_LEVEL, {
      speed: "slow",
      sub: 0.3,
      bright: 4500,
      attack: 0.05,
      release: 0.1,
      click: false,
    });
    const air = m.voice(hold(HELD, 0.8, HELD_AIR, SCORE_END, HELD_AIR, 0.08), {
      bus: "music",
      send: 0.3,
      hold: true,
    });
    if (air) air.noise("pink", 1, air.filter("bandpass", 4500, 0.8));
  },
};
