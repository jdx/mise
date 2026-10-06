import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { description } from "./llms.ts";

function summary(markdown: string): string | undefined {
  const dir = mkdtempSync(join(tmpdir(), "mise-llms-"));
  try {
    const file = join(dir, "guide.md");
    writeFileSync(file, markdown);
    return description(file);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("skips fenced code before the lead paragraph", () => {
  for (const [fence, indent, eol] of [
    ["```", "", "\n"],
    ["~~~", "", "\n"],
    ["````", "  ", "\n"],
    ["~~~~", "   ", "\r\n"],
  ]) {
    assert.equal(
      summary(
        `# Guide\n\n${indent}${fence}toml\n[tools]\nnode = "24"\n\n# Example heading\n${indent}${fence}\n\nInstall tools for your project.\n`.replaceAll(
          "\n",
          eol,
        ),
      ),
      "Install tools for your project.",
    );
  }
});

test("only a matching fence of sufficient length closes a code block", () => {
  assert.equal(
    summary(
      "# Guide\n\n````markdown\n```sh\nexample\n```\n~~~\nstill code\n`````\n\nThe actual summary.\n",
    ),
    "The actual summary.",
  );
});

test("headings inside fenced code do not start a page summary", () => {
  assert.equal(
    summary(
      "```markdown\n# Fake heading\nFake summary.\n```\n\n# Guide\n\nReal summary.\n",
    ),
    "Real summary.",
  );
});

test("a code block ends a prose paragraph even without a blank line", () => {
  assert.equal(
    summary("# Guide\n\nInstall tools.\n```sh\nmise install\n```\n"),
    "Install tools.",
  );
});

test("a page containing only code has no prose summary", () => {
  assert.equal(summary("# Guide\n\n```sh\nmise install\n```\n"), undefined);
  assert.equal(summary("# Guide\n\n```sh\nmise install\n"), undefined);
});

test("GitHub alert markers are not part of the summary", () => {
  for (const alert of ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"]) {
    assert.equal(
      summary(
        `# Guide\n\n> [!${alert}]\n> A useful warning.\n> More detail.\n`,
      ),
      "A useful warning. More detail.",
    );
  }
});

test("skips fenced code inside blockquotes before the lead paragraph", () => {
  for (const fence of ["```", "~~~"]) {
    for (const prefix of ["> ", "  > ", ">   "]) {
      assert.equal(
        summary(
          `# Guide\n\n${prefix}${fence}sh\n${prefix}mise install\n${prefix}\n${prefix}# Example heading\n${prefix}${fence}\n\nInstall tools for your project.\n`,
        ),
        "Install tools for your project.",
      );
    }
  }
});

test("skips an alert's fenced example before its warning text", () => {
  assert.equal(
    summary(
      "# Guide\n\n> [!WARNING]\n> ```sh\n> mise install\n> ```\n>\n> A useful warning.\n> More detail.\n",
    ),
    "A useful warning. More detail.",
  );
});

test("a quoted fence inside an ordinary code block does not close it", () => {
  for (const fence of ["```", "~~~"]) {
    assert.equal(
      summary(
        `# Guide\n\n${fence}markdown\n> ${fence}\nFake summary.\n${fence}\n\nReal summary.\n`,
      ),
      "Real summary.",
    );
  }
});

test("an unclosed quoted fence ends with its blockquote", () => {
  for (const fence of ["```", "~~~"]) {
    assert.equal(
      summary(`# Guide\n\n> ${fence}sh\n> mise install\nReal summary.\n`),
      "Real summary.",
    );
  }
});

test("reprocesses an unquoted fence after an unclosed quoted example", () => {
  for (const fence of ["```", "~~~"]) {
    for (const separator of ["", "\n"]) {
      assert.equal(
        summary(
          `# Guide\n\n> ${fence}sh\n> mise install\n${separator}${fence}\nnot a summary\n${fence}\n\nReal summary.\n`,
        ),
        "Real summary.",
      );
    }
  }
});

test("a quoted fence ends a blockquote summary", () => {
  assert.equal(
    summary("# Guide\n\n> Install tools.\n> ```sh\n> mise install\n> ```\n"),
    "Install tools.",
  );
});

test("keeps prose, code identifiers, and blockquote summaries", () => {
  assert.equal(
    summary(
      "---\r\ndescription: metadata\r\n---\r\n\r\n# Guide\r\n\r\n> Use `[bootstrap.packages]` to install **host packages**.\r\n> Keep snake_case intact.\r\n\r\nOther prose.\r\n",
    ),
    "Use [bootstrap.packages] to install host packages. Keep snake_case intact.",
  );
});
