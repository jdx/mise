// Renders release notes to HTML with the site's own markdown settings, so a
// code block in a release looks like one anywhere else in the docs. The Releases
// page loads the result one release at a time (see config.ts, which writes a
// file per release at build time and serves them in dev).
import { createMarkdownRenderer, type SiteConfig } from "vitepress";
import { releaseNotes } from "./release-notes.mjs";

export type RenderedNotes = { title: string; html: string };

let renderer: ReturnType<typeof createMarkdownRenderer> | undefined;

function markdownFor(siteConfig: SiteConfig) {
  // Release bodies are pull request titles and generated prose, so raw HTML in
  // them is shown as text rather than passed through.
  renderer ??= createMarkdownRenderer(
    siteConfig.srcDir,
    { ...siteConfig.markdown, html: false },
    siteConfig.site.base,
    siteConfig.logger,
  );
  return renderer;
}

function render(md: Awaited<typeof renderer>, markdown: string) {
  return (
    md!
      .render(markdown)
      // Many releases share headings ("Added", "Fixed"), so these anchors and
      // ids would repeat across the page.
      .replace(/<a class="header-anchor"[^>]*>.*?<\/a>/g, "")
      .replace(/<(h[1-6]) id="[^"]*"/g, "<$1")
  );
}

/** One release's rendered notes, or null when it has none. */
export async function renderReleaseNotes(
  siteConfig: SiteConfig,
  version: string,
): Promise<RenderedNotes | null> {
  const notes = (await releaseNotes()).get(version);
  if (!notes) return null;
  const md = await markdownFor(siteConfig);
  return { title: notes.title, html: render(md, notes.markdown) };
}

/** Every release's rendered notes, as Map<version, RenderedNotes>. */
export async function renderAllReleaseNotes(siteConfig: SiteConfig) {
  const md = await markdownFor(siteConfig);
  const out = new Map<string, RenderedNotes>();
  for (const [version, notes] of await releaseNotes()) {
    out.set(version, { title: notes.title, html: render(md, notes.markdown) });
  }
  return out;
}
