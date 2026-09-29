// A tap on every string the reel draws. The kit's text calls (drawText,
// drawWords and drawTermLine) report what they set before they set it, so a
// test can render the reel and read back every caption, label and terminal
// line it showed (test/forbidden.test.ts). With no sink it costs one check.

/** What drew the text: a terminal line, a line of type, or a caption's words. */
export type InkKind = "term" | "text" | "words";

type Sink = (text: string, kind: InkKind) => void;

let sink: Sink | null = null;

/** Send every string drawn from now on to `fn` (null stops it); returns the previous sink. */
export function setInkSink(fn: Sink | null): Sink | null {
  const prev = sink;
  sink = fn;
  return prev;
}

/** Report a string about to be drawn. */
export function ink(text: string, kind: InkKind): void {
  if (sink) sink(text, kind);
}
