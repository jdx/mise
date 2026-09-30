import assert from "node:assert/strict";
import test from "node:test";
import { formatNotes, parseNotes } from "./release-notes.mjs";

const release = {
  tag_name: "v2026.9.17",
  name: "v2026.9.17: Self-update waits 24 hours",
  body: "Summary.\r\n\r\n## Added\r\n\r\n- a thing\r\n\r\n## 💚 Sponsor mise\r\n\r\nPlease sponsor.\r\n",
};

test("notes keep the title and leave out the sponsor block", () => {
  assert.equal(
    formatNotes(release),
    "# Self-update waits 24 hours\n\nSummary.\n\n## Added\n\n- a thing\n",
  );
});

test("a release named by its tag has no title", () => {
  assert.equal(
    formatNotes({
      tag_name: "v2025.1.1",
      name: "v2025.1.1",
      body: "### Fixes\n",
    }),
    "### Fixes\n",
  );
});

test("a release with nothing but the sponsor block has no notes", () => {
  assert.equal(
    formatNotes({ ...release, body: "## Sponsor mise\n\nhello" }),
    null,
  );
});

test("parsing gives back the title and the body", () => {
  assert.deepEqual(parseNotes(formatNotes(release)), {
    title: "Self-update waits 24 hours",
    markdown: "Summary.\n\n## Added\n\n- a thing\n",
  });
  assert.deepEqual(parseNotes("### Fixes\n"), {
    title: "",
    markdown: "### Fixes\n",
  });
});
