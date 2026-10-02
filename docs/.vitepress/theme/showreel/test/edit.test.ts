import assert from "node:assert/strict";
import { test } from "node:test";
import {
  cuts,
  cutAt,
  filmChapters,
  filmChaptersVtt,
  filmDuration,
  type Edition,
} from "../edit";
import { commandCallout } from "../film";
import { loadFacts } from "../load";
import { sec } from "../timeline";
import { REPO } from "./repo";
import type { ReelFacts } from "../bible";

for (const edition of ["tour", "overview"] satisfies Edition[]) {
  test(`${edition}: picture and sound use contiguous cuts at the original reading pace`, () => {
    const list = cuts(edition);
    let end = 0;
    for (const cut of list) {
      assert.equal(cut.start, end);
      assert.equal(cut.end - cut.start, cut.to - cut.from);
      const source = sec(cut.id);
      assert.ok(cut.from >= source.start && cut.to <= source.end);
      assert.ok(cut.to > cut.from);
      if (!cut.brand) {
        assert.equal(cut.from, source.start);
        assert.equal(
          cut.to,
          source.end,
          "retained scenes keep their complete reading holds",
        );
      }
      assert.equal(cutAt(edition, cut.start).id, cut.id);
      assert.equal(cutAt(edition, cut.end - 1 / 120).id, cut.id);
      end = cut.end;
    }
    assert.equal(end, filmDuration(edition));
    assert.equal(cutAt(edition, end).brand, "outro");
  });
  test(`${edition}: chapters cover the delivered film, including reordered scenes`, () => {
    let end = 0;
    const chapters = filmChapters(edition);
    for (const chapter of chapters) {
      assert.equal(chapter.start, end);
      assert.ok(chapter.end > chapter.start);
      end = chapter.end;
    }
    assert.equal(end, filmDuration(edition));
    assert.equal(
      filmChaptersVtt(edition).trimEnd().split("\n\n").length,
      chapters.length + 1,
    );
    assert.equal(new Set(chapters.map((c) => c.id)).size, chapters.length);
  });
}

test("the tour is under five minutes and the overview is 60–90 seconds", () => {
  assert.ok(filmDuration("tour") < 300);
  assert.ok(filmDuration("overview") >= 60 && filmDuration("overview") <= 90);
  assert.equal(
    cuts("tour")[1].id,
    "switch",
    "project switching is the first demonstration",
  );
});

test("callouts use the current recording's output, including both project versions", () => {
  const { facts: data } = loadFacts(REPO);
  assert.ok(data);
  const facts = data as unknown as ReelFacts;
  const callouts = new Set<string>();
  for (let t = 0; t < sec("switch").len; t += 1 / 30) {
    const value = commandCallout("switch", t, facts);
    if (value) callouts.add(value);
  }
  assert.ok(
    [...callouts].some((s) =>
      s.endsWith(`v${data.versions.node.other_version}`),
    ),
  );
  assert.ok(
    [...callouts].some((s) => s.endsWith(`v${data.versions.node.lts_version}`)),
  );
  assert.equal(commandCallout("switch", 0, facts), null);
  assert.equal(commandCallout("switch", 10, null), null);
});
