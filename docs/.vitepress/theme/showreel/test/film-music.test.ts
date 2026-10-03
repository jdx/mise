// The films' music (film-music.ts): the song cut on its own bar lines,
// covering the film from its first frame, ending with its complete
// ring-out a few frames before the film does, its outro under the end
// card, and never restarted by a picture cut.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { playFilmMix } from "../audio";
import { MockContext } from "./mock-audio";
import { REPO } from "./repo";
import { test } from "node:test";
import { cuts, filmDuration, type Edition } from "../edit";
import {
  BAR,
  filmChordAt,
  MUSIC,
  musicChannel,
  musicCuts,
  musicEnd,
  SONG_CHART,
  songChordAt,
  SONG_END,
} from "../film-music";
import { reelChord } from "../film-audio";
import { CH } from "../score/harmony";
import { sec } from "../timeline";

/** The song's form (score/BED.md), bars. */
const FORM = { groove: 16, windDown: 168, outro: 172 } as const;

for (const edition of ["tour", "overview"] as const) {
  test(`${edition} music covers the film from its first frame and keeps the song's whole ending`, () => {
    const list = musicCuts(edition);
    let end = 0;
    for (const cut of list) {
      assert.equal(cut.start, end);
      assert.equal(cut.from % BAR, 0, "a join lands on a downbeat");
      assert.ok(cut.from >= 0 && cut.to <= MUSIC.duration);
      assert.ok(cut.to > cut.from);
      end = cut.end;
    }
    assert.equal(list.at(-1)!.to, MUSIC.duration);
    assert.equal(musicEnd(edition), end);
    // The ring-out dies out before the film ends, within a second of it.
    const film = filmDuration(edition);
    assert.ok(
      end < film && end > film - 1,
      `${edition}: music ends ${end} of ${film}`,
    );
    // The first groove drops once the open's card has left and the first
    // demonstration begins (edit.ts OPEN_TO), not under the open itself.
    const first = list[0];
    const drop = first.start + FORM.groove * BAR - first.from;
    const open = cuts(edition)[0];
    assert.ok(
      drop >= open.end - 1 && drop <= open.end + 2,
      `the groove drops at ${drop}`,
    );
    // The song's outro starts under the end card, or just before it.
    const last = list.at(-1)!;
    const outro = last.start + FORM.outro * BAR - last.from;
    const endCard = cuts(edition).at(-1)!;
    assert.ok(
      outro <= endCard.start && outro >= endCard.start - 8,
      `the outro starts at ${outro}`,
    );
    assert.ok(last.from <= FORM.windDown * BAR, "the wind-down plays whole");
  });

  test(`${edition}: picture cuts never stop or restart the music`, () => {
    const rate = 1000;
    const source = Float32Array.from(
      { length: Math.round(MUSIC.duration * rate) },
      (_, i) => i / rate,
    );
    const track = musicChannel(source, edition, rate);
    const joins = new Set(
      musicCuts(edition).map((c) => Math.round(c.start * rate)),
    );
    for (const cut of cuts(edition).slice(1)) {
      const frame = Math.round(cut.start * rate);
      if (joins.has(frame)) continue;
      // A monotonic source identifies a continuous playhead across the picture's cut.
      assert.ok(
        Math.abs(track[frame + 1] - track[frame - 1] - 2 / rate) < 0.00004,
        `${cut.id} at ${cut.start}s interrupts the music`,
      );
    }
  });
}

test("both arrangements preserve every last sample of the natural ring-out", () => {
  const rate = 1000;
  const source = new Float32Array(Math.round(MUSIC.duration * rate)).fill(0.25);
  source[source.length - 1] = 0.125;
  for (const edition of ["tour", "overview"] as Edition[]) {
    const track = musicChannel(source, edition, rate);
    const end = Math.round(musicEnd(edition) * rate);
    assert.equal(track[end - 1], 0.125);
    assert.equal(track[end], 0);
    assert.equal(track.at(-1), 0);
  }
});

test("the song's length is a whole number of its bars plus the ring-out", () => {
  assert.ok(SONG_END > 179 && SONG_END < 180);
});

test("the song chart is in order and the films' pitched effects read the chord under them", () => {
  for (let i = 1; i < SONG_CHART.length; i++)
    assert.ok(SONG_CHART[i][0] > SONG_CHART[i - 1][0]);
  assert.equal(SONG_CHART.at(-1)![0], 176);
  // The first groove's vamp: F minor, C minor, B♭, F minor.
  assert.equal(songChordAt(16 * BAR), CH.Fm);
  assert.equal(songChordAt(21 * BAR), CH.Cm);
  assert.equal(songChordAt(25 * BAR), CH.Bb);
  assert.equal(songChordAt(30 * BAR), CH.Fm);
  // Through 112–135 the F leaves its third out, as the source's parts do.
  assert.equal(songChordAt(113 * BAR), CH.F5);
  // The film clock: the tour opens in the intro's tail and plays the song straight through its first cut.
  const first = musicCuts("tour")[0];
  assert.equal(filmChordAt("tour", 0), songChordAt(first.from));
  assert.equal(filmChordAt("tour", 12), songChordAt(first.from + 12));
  assert.equal(filmChordAt("tour", musicEnd("tour") + 0.1), null);
  // A reel moment maps through the picture's cut to the film's: the switch
  // two seconds in plays at film 12.5 s in both editions.
  const t = sec("switch").start + 2;
  for (const edition of ["tour", "overview"] as const)
    assert.equal(reelChord(edition, t), filmChordAt(edition, 12.5));
  // A section the film does not play has no chord of the film's.
  assert.equal(reelChord("tour", sec("packslip").start + 1), null);
  assert.equal(reelChord("overview", sec("track").start + 1), null);
});

test("the renderer's original-song pin matches the committed bytes", () => {
  const bytes = readFileSync(join(REPO, MUSIC.file));
  assert.equal(createHash("sha256").update(bytes).digest("hex"), MUSIC.sha256);
});

test("the film music fader has no source breath or chapter automation", () => {
  const ac = new MockContext();
  const effects = {
    id: "edited-effects",
    duration: 284,
  } as unknown as AudioBuffer;
  const bed = { id: "film-music", duration: 284 } as unknown as AudioBuffer;
  playFilmMix(ac.context, ac.destination as AudioNode, effects, bed, [], 0.2);
  const curves = ac.calls.filter((c) => c.method === "setValueCurveAtTime");
  assert.equal(
    curves.length,
    1,
    "only the accent duck curve should ride the music",
  );
  assert.match(
    String(curves[0].args[0]),
    /min 1\)/,
    "without accents the music stays at full gain through the old breath",
  );
  assert.equal(
    ac.calls.filter((c) => c.method === "buffer=" && c.args[0] === "film-music")
      .length,
    1,
  );
});
