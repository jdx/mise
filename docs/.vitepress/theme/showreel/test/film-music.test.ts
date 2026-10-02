import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { playFilmMix } from "../audio";
import { MockContext } from "./mock-audio";
import { REPO } from "./repo";
import { test } from "node:test";
import { filmDuration, type Edition } from "../edit";
import { MUSIC, musicChannel, musicCuts } from "../film-music";

for (const edition of ["tour", "overview"] as const) {
  test(`${edition} music has continuous coverage and retains the song's entire ending`, () => {
    const cuts = musicCuts(edition);
    let end = 0;
    for (const cut of cuts) {
      assert.equal(cut.start, end);
      assert.equal(cut.from % 2, 0, "join must be on a downbeat");
      assert.ok(cut.from >= 0 && cut.to <= MUSIC.duration);
      end = cut.end;
    }
    assert.equal(cuts.at(-1)!.to, MUSIC.duration);
    assert.ok(Math.abs(filmDuration(edition) - end - 0.385) < 1e-8);
  });
}

test("picture cuts and the old breath never stop or restart tour music", () => {
  const rate = 1000;
  const source = Float32Array.from(
    { length: Math.round(MUSIC.duration * rate) },
    (_, i) => i / rate,
  );
  const tour = musicChannel(source, "tour", rate);
  for (const time of [6, 27, 28, 90, 112, 150, 210, 214, 238, 240, 276]) {
    const frame = time * rate;
    // A monotonic source identifies a continuous playhead, independent of
    // the visual edit and original source reel's music fades.
    assert.ok(
      Math.abs(tour[frame + 1] - tour[frame - 1] - 2 / rate) < 0.00004,
      `${time}s interrupts music`,
    );
  }
  assert.ok(
    tour[244 * rate] > tour[244 * rate - 151],
    "4:04 starts the final chorus instead of a quiet breath",
  );
});

test("both music arrangements preserve every last sample of the natural ring-out", () => {
  const rate = 1000;
  const source = new Float32Array(Math.round(MUSIC.duration * rate)).fill(0.25);
  source[source.length - 1] = 0.125;
  for (const edition of ["tour", "overview"] as Edition[]) {
    const track = musicChannel(source, edition, rate);
    const end = Math.round(musicCuts(edition).at(-1)!.end * rate);
    assert.equal(track[end - 1], 0.125);
    assert.equal(track[end], 0);
    assert.equal(track.at(-1), 0);
  }
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
