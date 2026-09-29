// lock: mise.lock records the versions mise.toml asks for; commit both. The
// skank carries on over F minor | B♭ minor | E♭ | D♭ | C, the dominant
// held through the push-in, into the whip. The reel's one riser
// climbs the dominant's fifth over its last three and a half beats; from
// the whip's wind-up (whip.ts WHIP_START) the band stops under it, and it
// cuts dead on the bar line as the whip lands on the new machine; the
// whip's air rushes right to left across it.
//
// The picture's cues: the ledger slides in on air; the thread from the
// request to the version ticks; the ledger's chip docks by
// `install_args: --locked` with the click and a soft landing; and
// `run: mise run ci` lights with a lighter click. A chop that would land on
// or just before a click rests: `run` fell on one and was buried.

import type { Part } from ".";
import { BEAT } from "../timeline";
import { WHIP_AT, WHIP_END, WHIP_LEAVE, WHIP_START } from "../whip";
import { LV, offbeatsNear, skank } from "./band";
import { CH, perBar } from "./harmony";
import { beatIn, listen } from "./listen";
import { ad, line, sweep } from "./mix";
import { air, PAN, seat } from "./props";
import { puff, riser, tick, whoosh } from "./sounds";

const CHANGES = perBar(CH.Fm, CH.Bbm, CH.Eb, CH.Db, CH.C);

/** The riser's length, beats: it ends on the whip's bar line. */
const RISE = 3.5;

export const part: Part = {
  level: 1.45,
  cues(m, s, facts) {
    const c = listen("lock", s, facts);
    for (const t of c.all("ledger"))
      air(m, t, t + 0.4, 0.09, 0.7, PAN.card, false);
    for (const t of c.all("thread")) tick(m, t, 3200, 0.14, PAN.card, 0.1);
    for (const t of c.all("dock")) {
      seat(m, t, 1.3);
      puff(m, t, 0.05, 0, false);
    }
    for (const t of c.all("run")) seat(m, t, 1.1, 0);
    // The whip: the riser from the wind-up's approach, cut dead on the bar
    // line; a swell as the content winds up; the rush out to the left and
    // the new content's arrival from the right. Kept under the climax's
    // bell: at 0.45 and a 0.1 rush it was the loudest moment of the reel.
    riser(m, WHIP_AT - RISE * BEAT, WHIP_AT, {
      vel: 0.3,
      doubles: [WHIP_START, WHIP_LEAVE],
    });
    whoosh(
      m,
      ad(WHIP_START, WHIP_LEAVE, 0.03, WHIP_LEAVE + 0.05),
      sweep(WHIP_START, 600, WHIP_LEAVE, 1400),
      1.2,
      { pan: line(WHIP_START, 0, WHIP_LEAVE, 0.25), send: 0.2 },
    );
    whoosh(
      m,
      [
        [WHIP_LEAVE, 0],
        [WHIP_AT - 0.02, 0.07],
        [WHIP_AT + 0.05, 0.05],
        [WHIP_END, 0.0001, "exp"],
        [WHIP_END + 0.004, 0],
      ],
      sweep(WHIP_LEAVE, 1500, WHIP_AT, 6000),
      1.1,
      { pan: line(WHIP_LEAVE, 0.3, WHIP_END, -0.7), send: 0.22 },
      "white",
    );
  },
  pads(m, s, facts) {
    const c = listen("lock", s, facts);
    const clicks = [...c.all("thread"), ...c.all("dock"), ...c.all("run")];
    skank(m, s, CHANGES, {
      chop: LV.chop,
      bass: 0.045,
      rests: [[beatIn(s, WHIP_START), s.beats]],
      chopRests: offbeatsNear(s, clicks, 0.3, 0.05),
    });
  },
};
