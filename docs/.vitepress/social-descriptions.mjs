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

/** Require authored metadata; never silently fall back to page prose. */
export function pageDescription(frontmatter, relativePath) {
  const description = frontmatter?.description;
  if (typeof description !== "string" || !description.trim()) {
    throw new Error(
      `Missing or invalid frontmatter description: ${relativePath}. Add a non-empty description string.`,
    );
  }
  return description.replace(/\s+/g, " ").trim();
}
