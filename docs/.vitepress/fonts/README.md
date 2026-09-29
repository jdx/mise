# Bundled fonts

These fonts are bundled so that the social images and the showreel render
the same everywhere, without system fonts or network access during the
build.

| File                           | Used for                                                                                                                 | Source                                                                                                                                              | License                                                                                |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `SpaceGrotesk.ttf`             | Social images; the showreel's captions and type                                                                          | [Google Fonts](https://github.com/google/fonts/tree/main/ofl/spacegrotesk)                                                                          | SIL Open Font License 1.1, in [`OFL.txt`](OFL.txt)                                     |
| `JetBrainsMono.ttf`            | The showreel's terminals and code (variable, weights 100–800)                                                            | [JetBrains Mono 2.304](https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304), `fonts/variable/JetBrainsMono[wght].ttf`, unmodified        | SIL Open Font License 1.1, in [`JetBrainsMono-OFL.txt`](JetBrainsMono-OFL.txt)         |
| `CormorantGaramond-Italic.ttf` | The showreel's tickets, act titles and name card (italic, variable, weights 300–700; the reel sets no upright Cormorant) | [Google Fonts](https://github.com/google/fonts/tree/main/ofl/cormorantgaramond), `CormorantGaramond-Italic[wght].ttf` (Cormorant 4.001), unmodified | SIL Open Font License 1.1, in [`CormorantGaramond-OFL.txt`](CormorantGaramond-OFL.txt) |

JetBrains Mono is the site's mono face and Cormorant Garamond its heading face;
the site itself loads both from Google Fonts. The upstream JetBrains Mono
release is used rather than Google's build, because Google's lacks `✓` and
the box-drawing characters that mise's output prints. The glyphs neither
build has (`✔`, `ℹ`, `↳` and the braille spinner) are drawn as vectors by the
showreel's terminal (`theme/showreel/kit/term.ts`).

The showreel loads these files with `FontFace` from the list in
`theme/showreel/fonts.ts`.
