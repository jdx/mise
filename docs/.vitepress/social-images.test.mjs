import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  socialCard,
  textWidth,
  wrapTitle,
  writeSocialCard,
} from "./social-images.mjs";

test("long headings and command names stay out of the logo column", () => {
  for (const title of [
    "W".repeat(120),
    "Machine-wide compilation scheduling and memory budgets",
    "mise help --verbose",
  ]) {
    const lines = wrapTitle(title);
    assert.ok(lines.length > 0);
    for (const line of lines) assert.ok(textWidth(line, 62) <= 720, line);
  }
});

test("escapes title markup and gives changed content a new image URL", () => {
  const first = socialCard('Rust & C++ <build> "cache"');
  assert.match(first.svg, /Rust &amp; C\+\+/);
  assert.ok(!first.svg.includes("<build>"));
  assert.equal(first.path, socialCard('Rust & C++ <build> "cache"').path);
  assert.notEqual(first.path, socialCard("Another page").path);
});

test("renders a self-contained 1200 by 630 PNG", () => {
  const dir = mkdtempSync(join(tmpdir(), "mise-social-"));
  try {
    const card = socialCard("Get started");
    writeSocialCard(dir, card);
    const png = readFileSync(join(dir, card.path));
    assert.deepEqual(
      [...png.subarray(0, 8)],
      [137, 80, 78, 71, 13, 10, 26, 10],
    );
    assert.equal(png.readUInt32BE(16), 1200);
    assert.equal(png.readUInt32BE(20), 630);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
