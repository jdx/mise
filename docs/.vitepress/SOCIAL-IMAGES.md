# Social previews

The docs build generates a 1200×630 PNG per page from its title and mise's
existing logo, with a short subtitle. The homepage uses a product headline. Images are
rendered locally with resvg and the bundled OFL-licensed Space Grotesk font;
no rendering service or system font is required. Image URLs include a content
hash of the final PNG bytes so title, artwork, font, and renderer changes get a
new URL.

Descriptions default to the first standalone prose paragraph in the Markdown
source. Extraction skips headings, lists (including generated CLI metadata),
code, components, and callouts. Link labels and inline code remain readable.
CLI pages therefore use their generated command help without manual edits.
The CLI index has a dedicated summary in the site configuration.

Set frontmatter `description` to override the automatic summary for HTML,
Open Graph, Twitter, and structured metadata. Set `socialDescription` when the
image needs a shorter editorial subtitle, for example:

```yaml
description: Install tools from signed release manifests with mise.
socialDescription: Signed releases and verified downloads.
```

Subtitles are limited to approximately 100 characters and two measured lines.
Titles use at most three lines, shrinking and truncating when necessary. Both
have reserved space separate from the logo and footer. Empty page summaries
fail the build so new pages cannot silently inherit generic site copy.

`docs:build` tests text wrapping and PNG rendering, then checks the built HTML
for matching Open Graph/Twitter metadata and the exact image expected from each
page title and subtitle. Checks also cover description consistency, canonical
URLs, image type and dimensions, and the error page's noindex directive.
Regression tests reject swapped images and empty alt text. Edit
`social-images.mjs` to adjust colors or layout.
