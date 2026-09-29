#!/usr/bin/env python3
"""Replay a raw capture through a VT emulator (pyte) and write its frames.

usage: frames.py CAPTURE_DIR

CAPTURE_DIR holds what record.py wrote: raw.bin (every byte the terminal
received), events.json ([t, offset, length] per read), marks.json and
meta.json. This writes:

  frames.json   every distinct screen the terminal showed, with its time
  final.txt     the last screen, as text
  marks.txt     the screen at each named mark, as text
  lines.txt     each distinct row the first time it appeared, with its time

Frames follow hk's capture rule: a terminal shows nothing between
ESC[?2026h and ESC[?2026l (synchronized output), so a frame is taken at every
ESC[?2026l and at the end of every read that is not inside such a block.
Consecutive identical frames are merged.

A capture that shares its shell with another (C18 and C19) has the whole
session in raw.bin and a "window" in meta.json. Every byte is replayed, so
the terminal state is right, but only frames inside the window are kept:
the first is the screen at the window's start. Times stay session times.

pyte has no dim attribute and folds bright colours into bold, so Screen
below replaces its SGR handling: cells carry dim, blink and bright colours
exactly as the program asked for them.

frames.json layout (colours per cell, stored as runs):
  {"width", "height",
   "styles": [{"fg", "bg", "bold", "dim", "italics", "underline",
               "strikethrough", "reverse", "blink"}],
   "rows":   [[[text, style_index], ...], ...],     # distinct rows
   "frames": [{"t", "offset", "rows": [row_index * height],
               "cursor": [x, y], "cursor_hidden"}],
   "marks":  [{"name", "t", "offset", "frame"}]}
Colours are "default", one of the 16 names ("red", "brightblue", ...),
"256:N" for palette entries 16-255, or "#rrggbb".
"""
import collections
import json
import os
import re
import sys

import pyte
from pyte import modes as mo

NAMES = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"]

Cell = collections.namedtuple(
    "Cell",
    ["data", "fg", "bg", "bold", "italics", "underscore", "strikethrough",
     "reverse", "dim", "blink"],
)


def color_256(n):
    if n < 8:
        return NAMES[n]
    if n < 16:
        return "bright" + NAMES[n - 8]
    return f"256:{n}"


class Screen(pyte.Screen):
    """pyte.Screen with lossless SGR attributes (dim, blink, bright colours)."""

    @property
    def default_char(self):
        return Cell(data=" ", fg="default", bg="default", bold=False,
                    italics=False, underscore=False, strikethrough=False,
                    reverse=mo.DECSCNM in self.mode, dim=False, blink=False)

    def reset(self):
        super().reset()
        self.cursor.attrs = self.default_char

    def select_graphic_rendition(self, *attrs, **_kw):
        a = list(attrs) or [0]
        rep = {}
        i = 0
        while i < len(a):
            x = a[i]
            i += 1
            if x == 0:
                rep.update(self.default_char._asdict())
                del rep["data"]
            elif x == 1:
                rep["bold"] = True
            elif x == 2:
                rep["dim"] = True
            elif x == 3:
                rep["italics"] = True
            elif x == 4:
                rep["underscore"] = True
            elif x in (5, 6):
                rep["blink"] = True
            elif x == 7:
                rep["reverse"] = True
            elif x == 9:
                rep["strikethrough"] = True
            elif x == 21:
                rep["underscore"] = True
            elif x == 22:
                rep["bold"] = False
                rep["dim"] = False
            elif x == 23:
                rep["italics"] = False
            elif x == 24:
                rep["underscore"] = False
            elif x == 25:
                rep["blink"] = False
            elif x == 27:
                rep["reverse"] = False
            elif x == 29:
                rep["strikethrough"] = False
            elif 30 <= x <= 37:
                rep["fg"] = NAMES[x - 30]
            elif x == 39:
                rep["fg"] = "default"
            elif 40 <= x <= 47:
                rep["bg"] = NAMES[x - 40]
            elif x == 49:
                rep["bg"] = "default"
            elif 90 <= x <= 97:
                rep["fg"] = "bright" + NAMES[x - 90]
            elif 100 <= x <= 107:
                rep["bg"] = "bright" + NAMES[x - 100]
            elif x in (38, 48):
                key = "fg" if x == 38 else "bg"
                if i < len(a) and a[i] == 5 and i + 1 < len(a):
                    rep[key] = color_256(a[i + 1])
                    i += 2
                elif i < len(a) and a[i] == 2 and i + 3 < len(a):
                    rep[key] = "#{:02x}{:02x}{:02x}".format(*a[i + 1:i + 4])
                    i += 4
        self.cursor.attrs = self.cursor.attrs._replace(**rep)


def ByteStream(screen):
    return pyte.ByteStream(screen)


SYNC = re.compile(rb"\x1b\[\?2026([hl])")


def style_of(c):
    return (c.fg, c.bg, c.bold, c.dim, c.italics, c.underscore,
            c.strikethrough, c.reverse, c.blink)


STYLE_KEYS = ["fg", "bg", "bold", "dim", "italics", "underline",
              "strikethrough", "reverse", "blink"]


class Encoder:
    def __init__(self, width, height):
        self.width, self.height = width, height
        self.styles, self.style_ix = [], {}
        self.rows, self.row_ix = [], {}

    def style(self, key):
        if key not in self.style_ix:
            self.style_ix[key] = len(self.styles)
            self.styles.append(dict(zip(STYLE_KEYS, key)))
        return self.style_ix[key]

    def row(self, line):
        # trailing blanks that paint nothing (no background, not reversed)
        # are not part of the row
        end = self.width
        while end > 0:
            c = line[end - 1]
            if c.data in (" ", "") and c.bg == "default" and not c.reverse:
                end -= 1
            else:
                break
        runs, cur, buf = [], None, []
        for x in range(end):
            c = line[x]
            if c.data == "":  # right half of a wide character
                continue
            k = self.style(style_of(c))
            if k != cur and buf:
                runs.append(["".join(buf), cur])
                buf = []
            cur = k
            buf.append(c.data)
        if buf:
            runs.append(["".join(buf), cur])
        key = json.dumps(runs, ensure_ascii=False)
        if key not in self.row_ix:
            self.row_ix[key] = len(self.rows)
            self.rows.append(runs)
        return self.row_ix[key]


def row_text(runs):
    return "".join(t for t, _ in runs)


def render(raw, events, width, height, extra=()):
    """Yield (t, offset, screen) at every point a terminal would show a frame,
    and at each (offset, t) in extra whatever the terminal is doing."""
    screen = Screen(width, height)
    stream = ByteStream(screen)
    # cut points: after each ESC[?2026l, and at the end of each read
    cuts = []  # (offset, t, kind)
    ends = [(off + n, t) for t, off, n in events]
    ei = 0
    for m in SYNC.finditer(raw):
        while ei < len(ends) and ends[ei][0] <= m.start():
            cuts.append((ends[ei][0], ends[ei][1], "read"))
            ei += 1
        # the time of a marker is the time of the read that delivered its end
        j = ei
        while j < len(ends) and ends[j][0] < m.end():
            j += 1
        t = ends[j][1] if j < len(ends) else ends[-1][1]
        cuts.append((m.end(), t, "sync-" + m.group(1).decode()))
    while ei < len(ends):
        cuts.append((ends[ei][0], ends[ei][1], "read"))
        ei += 1
    for off, t in extra:
        cuts.append((off, t, "window"))
    cuts.sort(key=lambda c: (c[0], {"window": 2, "read": 1}.get(c[2], 0)))
    pos, in_sync = 0, False
    for off, t, kind in cuts:
        if off > pos:
            stream.feed(bytes(raw[pos:off]))
            pos = off
        if kind == "window":
            yield t, off, screen
            continue
        if kind == "sync-h":
            in_sync = True
            continue
        if kind == "sync-l":
            in_sync = False
            yield t, off, screen
            continue
        if not in_sync:
            yield t, off, screen


def build(capdir):
    meta = json.load(open(os.path.join(capdir, "meta.json")))
    raw = open(os.path.join(capdir, "raw.bin"), "rb").read()
    events = json.load(open(os.path.join(capdir, "events.json")))
    marks = json.load(open(os.path.join(capdir, "marks.json")))
    w, h = meta["cols"], meta["rows"]
    win = meta.get("window")
    lo, hi = (win["from_offset"], win["to_offset"]) if win else (0, len(raw))
    enc = Encoder(w, h)
    frames = []
    last = None
    extra = [(lo, win["from_t"])] if win else []
    for t, off, scr in render(raw, events, w, h, extra):
        if off < lo or off > hi or (win and off == lo and frames):
            continue
        rows = [enc.row(scr.buffer[y]) for y in range(h)]
        cur = [scr.cursor.x, scr.cursor.y]
        key = (tuple(rows), tuple(cur), scr.cursor.hidden)
        if key == last:
            frames[-1]["offset_end"] = off
            continue
        last = key
        frames.append({"t": t, "offset": off, "rows": rows, "cursor": cur,
                       "cursor_hidden": bool(scr.cursor.hidden)})
    # a mark points at the last frame at or before its raw offset
    out_marks = []
    for m in marks:
        fi = 0
        for i, f in enumerate(frames):
            if f["offset"] <= m["offset"]:
                fi = i
            else:
                break
        out_marks.append({**m, "frame": fi})
    doc = {"id": meta["id"], "width": w, "height": h, "styles": enc.styles,
           "rows": enc.rows, "frames": frames, "marks": out_marks}
    if win:
        doc["window"] = win
    with open(os.path.join(capdir, "frames.json"), "w") as f:
        json.dump(doc, f, ensure_ascii=False, separators=(",", ":"))
    write_text(capdir, doc)
    return doc


def screen_text(doc, frame):
    lines = [row_text(doc["rows"][r]) for r in frame["rows"]]
    while lines and not lines[-1].strip():
        lines.pop()
    return "\n".join(lines)


def write_text(capdir, doc):
    fr = doc["frames"]
    with open(os.path.join(capdir, "final.txt"), "w") as f:
        f.write(screen_text(doc, fr[-1]) + "\n" if fr else "")
    with open(os.path.join(capdir, "marks.txt"), "w") as f:
        for m in doc["marks"]:
            fm = fr[m["frame"]]
            f.write(f"----- {m['name']} (t={fm['t']:.3f}s, frame {m['frame']})\n")
            f.write(screen_text(doc, fm) + "\n")
    seen = set()
    with open(os.path.join(capdir, "lines.txt"), "w") as f:
        for fm in fr:
            for r in fm["rows"]:
                if r in seen:
                    continue
                seen.add(r)
                txt = row_text(doc["rows"][r])
                if txt.strip():
                    f.write(f"{fm['t']:8.3f} | {txt}\n")


if __name__ == "__main__":
    for d in sys.argv[1:]:
        doc = build(d)
        print(f"{d}: {len(doc['frames'])} frames, {len(doc['rows'])} rows, "
              f"{len(doc['styles'])} styles", file=sys.stderr)
