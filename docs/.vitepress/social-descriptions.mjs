import MarkdownIt from "markdown-it";

const md = new MarkdownIt({ html: true });

/** Plain prose only: skip code, components, lists, quotes, and callouts. */
export function extractDescription(source) {
  const body = source
    .replace(/^---\r?\n[\s\S]*?\r?\n---(?:\r?\n|$)/, "")
    .replace(/^:::[^\n]*\n[\s\S]*?^:::\s*$/gm, "");
  const tokens = md.parse(body, {});
  for (let i = 0; i < tokens.length; i++) {
    if (tokens[i].type !== "paragraph_open" || tokens[i].level !== 0) continue;
    const inline = tokens[i + 1];
    if (inline?.type !== "inline") continue;
    // Component markup and interpolations are not useful preview prose.
    if (/<[A-Za-z!/]|\{\{/.test(inline.content)) continue;
    const text = (inline.children ?? [])
      .map((token) =>
        ["text", "code_inline"].includes(token.type)
          ? token.content
          : ["softbreak", "hardbreak"].includes(token.type)
            ? " "
            : "",
      )
      .join("")
      .replace(/\s+/g, " ")
      .trim();
    if (text && !/^(?:Usage|Aliases|Effect|Source code):/.test(text))
      return text;
  }
  return "";
}

/** Prefer a complete sentence; otherwise stop at a word boundary. */
export function shortenDescription(text, limit = 160) {
  const clean = String(text).replace(/\s+/g, " ").trim();
  if (clean.length <= limit) return clean;
  const prefix = clean.slice(0, limit - 1);
  const sentence = prefix.match(/^([\s\S]*[.!?])\s/);
  if (sentence && sentence[1].length >= limit / 2) return sentence[1];
  const boundary = prefix.lastIndexOf(" ");
  return (boundary > 0 ? prefix.slice(0, boundary) : prefix) + "…";
}

export function pageDescription(source, frontmatter = {}) {
  return frontmatter.description
    ? String(frontmatter.description).replace(/\s+/g, " ").trim()
    : shortenDescription(extractDescription(source));
}
