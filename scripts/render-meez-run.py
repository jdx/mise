# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=12", "numpy>=2", "fonttools>=4", "brotli>=1"]
# ///
"""Render the Meez Run motion graphics, using the supplied song's timed lyrics.

Requires ffmpeg/ffprobe on PATH. From the repository root:
  uv run scripts/render-meez-run.py '/path/to/Meez Run (2).m4a'
  uv run scripts/render-meez-run.py '/path/to/Meez Run (2).m4a' --preview /tmp/meez

The original soundtrack is an input, not an additional copy in the repository.
The MP4 contains that audio encoded as AAC. All graphics are drawn here, using
the same Roc Grotesk fonts as the documentation. Terminal scenes are illustrative.
The embedded subtitle timing drives both the scene changes and English captions.
"""

import argparse
import bisect
import functools
import itertools
import json
import math
import subprocess
import tempfile
from pathlib import Path

import numpy as np
from fontTools.ttLib import TTFont
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
W, H, FPS = 1920, 1080, 30
INK = (17, 20, 29)
PAPER = (246, 241, 231)
MINT = (148, 242, 206)
LILAC = (183, 163, 255)
CORAL = (255, 126, 137)
GOLD = (245, 208, 133)
MUTED = (141, 149, 167)
SECTIONS = [
    "Intro",
    "Verse 1",
    "Build-Up",
    "Drop",
    "Verse 2",
    "Breakdown",
    "Build-Up 2",
    "Drop 2",
    "Outro",
]


def command(*args):
    return subprocess.check_output(args)


def seconds(value):
    h, m, s = value.replace(",", ".").split(":")
    return int(h) * 3600 + int(m) * 60 + float(s)


def timestamp(value):
    ms = round(value * 1000)
    return (
        f"{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02}.{ms % 1000:03}"
    )


def normalize(text):
    # The song uses phonetic spellings to guide its singer. Show the tool names.
    for before, after in {
        "See-eye": "CI",
        "see-eye": "CI",
        "see-dee": "cd",
        "En-vee-em": "nvm",
        "Pie-env, ar-bee-env": "pyenv, rbenv",
        "Make-file, deer-env": "Makefile, direnv",
        "Meez dot tommel": "mise.toml",
        "Meez use": "mise use",
        "Meez install": "mise install",
        "Meez run test": "mise run test",
    }.items():
        text = text.replace(before, after)
    return text


def read_song(audio):
    info = json.loads(
        command("ffprobe", "-v", "error", "-show_streams", "-of", "json", str(audio))
    )
    duration = float(
        next(s for s in info["streams"] if s["codec_type"] == "audio")["duration"]
    )
    srt = command(
        "ffmpeg", "-v", "error", "-i", str(audio), "-map", "0:s:0", "-f", "srt", "-"
    ).decode()
    cues, chapters = [], []
    for block in srt.strip().split("\n\n"):
        lines = block.splitlines()
        if len(lines) < 3:
            continue
        start, end = map(seconds, lines[1].split(" --> "))
        text = " ".join(lines[2:])
        if text.strip("[]") in SECTIONS:
            chapters.append((start, text.strip("[]")))
        elif not text.startswith("[") and start < duration and end > start:
            cues.append((start, min(end, duration), normalize(text)))
    if [name for _, name in chapters] != SECTIONS:
        raise ValueError(
            "Expected the nine Meez Run sections in the input's timed lyrics"
        )
    chapters[0] = (0.0, "Intro")
    return duration, cues, chapters


def envelope(audio, duration):
    raw = command(
        "ffmpeg",
        "-v",
        "error",
        "-i",
        str(audio),
        "-map",
        "0:a:0",
        "-ar",
        "12000",
        "-ac",
        "1",
        "-f",
        "f32le",
        "-",
    )
    samples = np.frombuffer(raw, dtype="<f4")
    levels, bands = [], []
    edges = np.geomspace(3, 700, 49).astype(int)
    for frame in range(math.ceil(duration * FPS)):
        center = int(frame * 12000 / FPS)
        chunk = samples[max(0, center - 1024) : center + 1024]
        chunk = np.pad(chunk, (0, 2048 - len(chunk)))
        levels.append(np.sqrt(np.mean(chunk * chunk)))
        spectrum = np.abs(np.fft.rfft(chunk * np.hanning(2048)))
        bands.append(
            [np.mean(spectrum[a : max(a + 1, b)]) for a, b in itertools.pairwise(edges)]
        )
    levels = np.array(levels)
    levels = np.clip(levels / max(np.percentile(levels, 95), 0.001), 0, 1)
    bands = np.log1p(np.array(bands))
    bands = np.clip(bands / max(np.percentile(bands, 97), 0.001), 0, 1)
    # Smooth the display envelope; the soundtrack is never modified.
    for i in range(1, len(levels)):
        levels[i] = 0.6 * levels[i] + 0.4 * levels[i - 1]
        bands[i] = 0.45 * bands[i] + 0.55 * bands[i - 1]
    return levels, bands


class Film:
    def __init__(self, duration, cues, chapters, levels, bands, font_dir):
        self.duration, self.cues, self.chapters = duration, cues, chapters
        self.starts = [cue[0] for cue in cues]
        self.chapter_starts = [c[0] for c in chapters]
        self.levels, self.bands, self.font_dir = levels, bands, font_dir
        self.font = functools.lru_cache(maxsize=128)(self.font)
        self.label = functools.lru_cache(maxsize=768)(self.label)
        self.backgrounds = {}
        for light in (False, True):
            bg = Image.new("RGB", (W, H), PAPER if light else INK)
            d = ImageDraw.Draw(bg)
            dot = (226, 219, 205) if light else (36, 40, 53)
            for x in range(48, W, 48):
                for y in range(48, H, 48):
                    d.ellipse((x, y, x + 1, y + 1), fill=dot)
            self.backgrounds[light] = bg

    def font(self, size, weight="bold"):
        return ImageFont.truetype(str(self.font_dir / f"{weight}.ttf"), size)

    def label(self, text, size, color, weight="bold"):
        font = self.font(size, weight)
        box = font.getbbox(text)
        layer = Image.new(
            "RGBA", (max(1, box[2] - box[0] + 4), max(1, box[3] - box[1] + 4))
        )
        ImageDraw.Draw(layer).text(
            (2 - box[0], 2 - box[1]), text, font=font, fill=color
        )
        return layer

    def text(
        self,
        image,
        text,
        x,
        y,
        size=60,
        color=PAPER,
        weight="bold",
        center=False,
        limit=None,
    ):
        if limit:
            while self.font(size, weight).getlength(text) > limit:
                size -= 2
        layer = self.label(text, size, color, weight)
        image.paste(
            layer, (round(x - layer.width / 2 if center else x), round(y)), layer
        )

    def lines(
        self, image, lines, x, y, size=144, color=PAPER, leading=0.95, center=False
    ):
        for i, line in enumerate(lines):
            self.text(
                image, line, x, y + i * size * leading, size, color, center=center
            )

    def card(self, image, x, y, width, height, title, lines, accent=MINT):
        d = ImageDraw.Draw(image)
        d.rounded_rectangle(
            (x, y, x + width, y + height),
            18,
            fill=(25, 30, 42),
            outline=(65, 73, 94),
            width=2,
        )
        d.line((x, y + 70, x + width, y + 70), fill=(65, 73, 94), width=2)
        for i, color in enumerate((CORAL, GOLD, MINT)):
            d.ellipse((x + 25 + i * 25, y + 28, x + 35 + i * 25, y + 38), fill=color)
        self.text(
            image, title, x + 124, y + 24, 24, MUTED, "regular", limit=width - 144
        )
        for i, line in enumerate(lines):
            self.text(
                image,
                line,
                x + 36,
                y + 110 + 64 * i,
                37,
                accent if i == 0 else PAPER,
                "regular",
                limit=width - 72,
            )

    def frame(self, t, poster=False):
        frame = min(int(t * FPS), len(self.levels) - 1)
        energy = float(self.levels[frame])
        chapter = max(0, bisect.bisect_right(self.chapter_starts, t) - 1)
        start, name = self.chapters[chapter]
        end = (
            self.chapter_starts[chapter + 1]
            if chapter + 1 < len(self.chapters)
            else self.duration
        )
        progress = (t - start) / (end - start)
        cue_idx = max(0, bisect.bisect_right(self.starts, t) - 1)
        cue_start, cue_end, lyric = self.cues[cue_idx]
        current = lyric.lower()
        light = name == "Breakdown"
        ink, subdued = (INK, (104, 101, 102)) if light else (PAPER, MUTED)
        accent = (
            GOLD
            if name == "Drop 2"
            else LILAC
            if name in ("Build-Up", "Drop")
            else MINT
        )
        image = self.backgrounds[light].copy()
        d = ImageDraw.Draw(image)

        if name in ("Intro", "Outro") or poster:
            # A modular arrangement assembles around the title, then settles.
            for i in range(5):
                x = 1110 + (i % 2) * 246
                y = 215 + (i // 2) * 194
                drift = (1 - min(1, progress * 3)) * 80 * math.sin(t * 0.5 + i)
                d.rounded_rectangle(
                    (x + drift, y, x + 204 + drift, y + 152),
                    14,
                    outline=accent if i % 2 else (79, 91, 107),
                    width=3,
                )
                self.text(
                    image,
                    ["TOOLS", "ENV", "TASKS", "LOCK", "RUN"][i],
                    x + 24 + drift,
                    y + 63,
                    26,
                    accent if i % 2 else PAPER,
                )
            if name == "Intro" and "shell just works" in current and not poster:
                self.lines(image, ["YOUR SHELL.", "JUST WORKS."], 110, 308, 134, PAPER)
                self.text(image, "> ready_", 122, 648, 79, MINT, "regular")
            else:
                self.text(image, "mise", 110, 235, 390)
                d.rounded_rectangle((125, 601, 659, 740), 12, fill=accent)
                self.text(image, "RUN", 390, 617, 135, INK, center=True)
                self.text(
                    image, "Everything in its place.", 127, 783, 48, PAPER, "regular"
                )
            if name == "Outro" and "exit code" in current:
                image = self.backgrounds[False].copy()
                d = ImageDraw.Draw(image)
                self.text(image, "> exit code", W / 2, 304, 114, PAPER, center=True)
                self.text(image, "0", W / 2, 446, 258, MINT, center=True)
                self.text(
                    image, "mise.jdx.dev", W / 2, 772, 45, PAPER, "regular", center=True
                )

        elif name == "Verse 1":
            if "red" in current:
                title, label, lines, accent = (
                    ["03:00", "CI IS RED."],
                    "workflow / test",
                    ["$ node --test", "version mismatch", "job failed"],
                    CORAL,
                )
            elif "drifted" in current:
                title, label, lines, accent = (
                    ["PIN IT.", "KEEP IT."],
                    "project / tools",
                    ["node   python   go", "laptop  /  CI", "versions drifted"],
                    CORAL,
                )
            elif "lockfile" in current or "same every" in current:
                title, label, lines = (
                    ["HOLD", "THE LINE."],
                    "mise.lock",
                    ["one lockfile", "laptop  =  CI", "same tool versions"],
                )
            elif "shell" in current:
                title, label, lines = (
                    ["CD.", "AT EASE."],
                    "shell / my-project",
                    ["$ cd my-project", "tools + env", "ready for this project"],
                )
            else:
                title, label, lines = (
                    ["ALL IN", "PLACE."],
                    "mise.toml",
                    ["[tools]", "[env]", "[tasks]"],
                )
            self.lines(image, title, 110, 290, 147, accent)
            self.card(
                image, 1000, 255 + 8 * math.sin(t), 792, 484, label, lines, accent
            )
            d.line((116, 760, 825, 760), fill=accent, width=3)
            self.text(
                image,
                "LAPTOP  /  CI  /  SAME EVERY TIME",
                118,
                789,
                25,
                MUTED,
                "regular",
            )

        elif name == "Build-Up":
            for row in range(6):
                for col in range(10):
                    x, y = 90 + col * 178, 191 + row * 99
                    color = (
                        (52, 49, 73) if (row + col + int(t * 2)) % 5 else (98, 85, 134)
                    )
                    d.rounded_rectangle(
                        (x, y, x + 148, y + 72), 5, outline=color, width=2
                    )
            if "fifty thousand" in current:
                words, size = ["50,000", "TERMINALS."], 214
            elif "say..." in current:
                words, size = ["SAY..."], 236
            elif "command" in current:
                words, size = ["ONE", "COMMAND."], 207
            elif "file" in current:
                words, size = ["ONE FILE.", "EVERYTHING."], 175
            else:
                words, size = ["MISE!", "RUN!"], 239
            self.lines(image, words, W / 2, 250, size, PAPER, center=True)
            d.rectangle((112, 824, 112 + int(1696 * progress), 832), fill=LILAC)

        elif name in ("Drop", "Drop 2"):
            # Moving perspective lines and a spectrum respond to the actual mix.
            vx, vy = 1320, 450
            for i in range(-6, 10):
                d.line((vx, vy, i * 280, 913), fill=(53, 50, 72), width=2)
            for i in range(10):
                p = (i / 10 + t * 0.11) % 1
                y = vy + int(p * p * 460)
                d.line((90, y, 1830, y), fill=(68, 62, 83), width=2)
            for i in range(4):
                radius = 170 + i * 75 + 16 * energy
                d.ellipse(
                    (vx - radius, vy - radius, vx + radius, vy + radius),
                    outline=(59 + i * 8, 56 + i * 6, 82 + i * 9),
                    width=2,
                )
            variation = int((t - start) / 8) % 3
            if variation == 0:
                self.text(image, "mise", 102, 178 - 9 * energy, 357, PAPER)
                self.text(image, "run", 115, 467 + 8 * energy, 356, accent)
                for i, band in enumerate(self.bands[frame][::2]):
                    x = 1080 + i * 29
                    height = 20 + 330 * float(band)
                    d.rounded_rectangle(
                        (x, 640 - height / 2, x + 14, 640 + height / 2),
                        6,
                        fill=accent if i % 3 else PAPER,
                    )
                self.text(image, "EVERYTHING", 1080, 240, 48, accent)
                self.text(image, "IN ITS PLACE.", 1080, 297, 48, accent)
            elif variation == 1:
                self.text(
                    image, "MISE", W / 2 + 14 * energy, 160, 344, PAPER, center=True
                )
                self.text(
                    image, "RUN", W / 2 - 14 * energy, 481, 347, accent, center=True
                )
                for side in (180, 1640):
                    for j in range(9):
                        height = 30 + float(self.bands[frame][j * 5]) * 39
                        d.rounded_rectangle(
                            (side, 230 + j * 62, side + 52, 230 + j * 62 + height),
                            4,
                            fill=accent,
                        )
            else:
                self.text(
                    image,
                    "EVERYTHING IN ITS PLACE.",
                    W / 2,
                    197,
                    70,
                    PAPER,
                    center=True,
                )
                for j, word in enumerate(("TOOLS", "ENV", "TASKS")):
                    x = 240 + j * 506
                    lift = float(self.bands[frame][j * 16]) * 24
                    d.rounded_rectangle(
                        (x, 343 - lift, x + 430, 558 - lift),
                        16,
                        fill=(31, 36, 48),
                        outline=accent,
                        width=3,
                    )
                    self.text(image, word, x + 215, 418 - lift, 63, accent, center=True)
                self.text(image, "> mise run", W / 2, 665, 121, PAPER, center=True)
            if "work to do" in current:
                self.text(
                    image, "PEOPLE WITH WORK TO DO.", 1080, 792, 31, PAPER, limit=720
                )

        elif name == "Verse 2":
            entries = [
                ("pinned node", "WHO PINNED NODE?", "mise use node@26", "TOOLS"),
                ("new hire", "NEW HIRE MONDAY?", "mise install", "ONBOARD"),
                ("tests", "WHO RUNS THE TESTS?", "mise run test", "TASKS"),
                ("config", "WHERE'S THE CONFIG?", "mise.toml", "CONFIG"),
            ]
            match = next((v for v in entries if v[0] in current), None)
            if match:
                _, question, response, category = match
                self.text(image, question, 117, 263, 64, MUTED)
                self.text(image, "> " + response, 110, 411, 137, MINT, limit=1700)
                d.rounded_rectangle((121, 670, 489, 738), 10, fill=(43, 59, 58))
                self.text(image, category, 305, 688, 30, MINT, center=True)
            else:
                self.text(image, "ONE TOOL.", W / 2, 241, 199, PAPER, center=True)
                self.text(image, "mise", W / 2, 439, 310, MINT, center=True)

        elif name == "Breakdown":
            self.lines(image, ["Everything", "in its place."], 112, 235, 151, INK)
            self.text(image, "mise en place", 119, 580, 45, (120, 81, 95), "regular")
            for i, (title, detail) in enumerate(
                (
                    ("tools", "the right versions"),
                    ("env", "the right context"),
                    ("tasks", "the work to do"),
                )
            ):
                x = 1120 + 16 * math.sin(t * 0.3 + i)
                y = 214 + i * 197
                d.rounded_rectangle(
                    (x, y, x + 644, y + 162),
                    12,
                    fill=(234, 227, 214),
                    outline=(213, 203, 185),
                    width=2,
                )
                self.text(
                    image, f"0{i + 1}", x + 28, y + 38, 29, (140, 109, 98), "regular"
                )
                self.text(image, title, x + 108, y + 26, 57, INK)
                self.text(
                    image, detail, x + 111, y + 103, 29, (104, 101, 102), "regular"
                )
            d.line((120, 715, 790, 715), fill=(161, 125, 139), width=2)

        elif name == "Build-Up 2":
            if any(
                word in current
                for word in (
                    "rebuilding",
                    "flake",
                    "gigs",
                    "experimental",
                    "year",
                    "recursion",
                    "configuring",
                )
            ):
                for i in range(9):
                    inset = i * 29 + 10 * math.sin(t * 0.8)
                    d.rounded_rectangle(
                        (1120 + inset, 180 + inset, 1770 - inset, 830 - inset),
                        9,
                        outline=(92 + i * 9, 57 + i * 4, 73 + i * 5),
                        width=2,
                    )
                if "recursion" in current:
                    words, size = ["INFINITE", "RECURSION."], 129
                elif "configuring" in current:
                    words, size = ["WEEKEND", "CONFIGURING."], 116
                elif "gigs" in current:
                    words, size = ["40 GB.", "STILL WAITING."], 118
                elif "experimental" in current or "year" in current:
                    words, size = ["STILL", "EXPERIMENTAL."], 111
                elif "flake" in current:
                    words, size = ["FLAKE.", "FLAKE."], 154
                else:
                    words, size = ["ALL THAT.", "TO BOIL WATER."], 113
                self.lines(image, words, 110, 320, size, CORAL)
            elif "work to do" in current:
                self.lines(
                    image,
                    ["PEOPLE WITH", "WORK TO DO."],
                    W / 2,
                    267,
                    183,
                    MINT,
                    center=True,
                )
            elif "get to work" in current:
                self.text(
                    image, "> so get to work_", W / 2, 413, 120, MINT, center=True
                )
            else:
                self.text(image, "SHIP.", W / 2, 250, 360, MINT, center=True)
                self.text(
                    image, "WE SPENT IT SHIPPING.", W / 2, 691, 55, PAPER, center=True
                )
                for i in range(5):
                    x = 1220 + i * 108 + (t * 180 % 108)
                    d.line((x, 730, x + 32, 754, x, 778), fill=MINT, width=5)

        # A short eased entrance moves the scene on each vocal cue. Lyrics and
        # framing stay fixed, so they remain readable while the graphics move.
        elapsed = t - max(start, cue_start)
        if 0 <= elapsed < 0.42 and not poster:
            shift = round(46 * (1 - elapsed / 0.42) ** 3)
            panel = image.crop((90, 158, 1830, 870))
            image.paste(self.backgrounds[light].crop((90, 158, 1830, 870)), (90, 158))
            image.paste(
                panel.crop((0, 0, panel.width - shift, panel.height)), (90 + shift, 158)
            )

        # Consistent typography and margins keep every chapter legible on mobile.
        d = ImageDraw.Draw(image)
        self.text(image, "mise / run", 112, 66, 31, ink)
        self.text(
            image,
            "EVERYTHING IN ITS PLACE",
            W / 2,
            72,
            21,
            subdued,
            "regular",
            center=True,
        )
        label = self.label(f"0{chapter + 1} / {name.upper()}", 21, subdued, "regular")
        image.paste(label, (1807 - label.width, 73), label)
        d.line(
            (112, 120, 1808, 120),
            fill=(207, 200, 187) if light else (55, 62, 78),
            width=2,
        )

        if not poster and cue_start <= t < cue_end:
            size = 41
            words = lyric.split()
            line, wrapped = "", []
            for word in words:
                trial = (line + " " + word).strip()
                if self.font(size, "regular").getlength(trial) > 1640:
                    wrapped.append(line)
                    line = word
                else:
                    line = trial
            wrapped.append(line)
            for i, line in enumerate(wrapped):
                self.text(
                    image,
                    line,
                    W / 2,
                    906 + i * 47 - (len(wrapped) - 1) * 28,
                    size,
                    ink,
                    "regular",
                    center=True,
                )
        for i, value in enumerate(self.bands[frame]):
            x = 118 + i * 8
            height = 3 + int(value * 25)
            d.rectangle((x, 1006 - height, x + 3, 1006), fill=subdued)
        self.text(image, "mise.jdx.dev", 1634, 983, 26, subdued, "regular")
        d.line(
            (112, 1036, 1808, 1036),
            fill=(207, 200, 187) if light else (55, 62, 78),
            width=2,
        )
        if not poster:
            d.line(
                (112, 1036, 112 + int(1696 * t / self.duration), 1036),
                fill=(140, 80, 107) if light else accent,
                width=3,
            )
        return image


def render(args):
    audio = args.audio.resolve()
    duration, cues, chapters = read_song(audio)
    levels, bands = envelope(audio, duration)
    with tempfile.TemporaryDirectory(prefix="meez-run-") as temp:
        temp = Path(temp)
        for weight in ("bold", "regular"):
            font = TTFont(
                ROOT / "docs/public/fonts" / f"rocgrotesk-{weight}-webfont.woff2"
            )
            font.flavor = None
            font.save(temp / f"{weight}.ttf")
        film = Film(duration, cues, chapters, levels, bands, temp)
        if args.preview:
            args.preview.mkdir(parents=True, exist_ok=True)
            samples = [5, 37, 44, 64, 80, 95, 134, 158, 191, 204, 239, 300]
            sheet = Image.new("RGB", (1440, 1620), INK)
            for i, t in enumerate(samples):
                frame = film.frame(min(t, duration - 0.1))
                frame.save(args.preview / f"{t:03}.png")
                sheet.paste(frame.resize((480, 270)), ((i % 3) * 480, (i // 3) * 405))
                ImageDraw.Draw(sheet).text(
                    ((i % 3) * 480 + 16, (i // 3) * 405 + 287),
                    f"{t // 60}:{t % 60:02}",
                    fill=PAPER,
                    font=film.font(28),
                )
            sheet.save(args.preview / "contact-sheet.png")
            return

        output = ROOT / "docs/tapes/meez-run.mp4"
        poster = output.with_suffix(".png")
        vtt = ROOT / "docs/public/meez-run.en.vtt"
        film.frame(5, poster=True).save(poster, optimize=True)
        captions = (
            "WEBVTT\n\n"
            + "\n\n".join(
                f"{timestamp(a)} --> {timestamp(b)}\n{line}" for a, b, line in cues
            )
            + "\n"
        )
        vtt.write_text(captions)
        subtitle = temp / "captions.srt"
        subtitle.write_text(
            "\n\n".join(
                f"{i + 1}\n{timestamp(a).replace('.', ',')} --> {timestamp(b).replace('.', ',')}\n{line}"
                for i, (a, b, line) in enumerate(cues)
            )
        )
        encoder = subprocess.Popen(
            [
                "ffmpeg",
                "-y",
                "-v",
                "warning",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgb24",
                "-video_size",
                f"{W}x{H}",
                "-framerate",
                str(FPS),
                "-i",
                "-",
                "-i",
                str(audio),
                "-i",
                str(subtitle),
                "-map",
                "0:v",
                "-map",
                "1:a:0",
                "-map",
                "2:s:0",
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "24",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "160k",
                "-c:s",
                "mov_text",
                "-metadata:s:s:0",
                "language=eng",
                "-metadata",
                "title=Meez Run",
                "-metadata",
                "comment=Animated mise music video; soundtrack supplied by jdx",
                "-t",
                str(duration),
                "-movflags",
                "+faststart",
                str(output),
            ],
            stdin=subprocess.PIPE,
        )
        try:
            for n in range(math.ceil(duration * FPS)):
                encoder.stdin.write(film.frame(n / FPS).tobytes())
                if n % (30 * FPS) == 0:
                    print(f"Rendered {n / FPS:.0f}/{duration:.1f} seconds", flush=True)
        finally:
            encoder.stdin.close()
        if encoder.wait() != 0:
            raise RuntimeError("ffmpeg encoding failed")
        print(
            f"Wrote {output} ({output.stat().st_size / 1024 / 1024:.1f} MiB)",
            flush=True,
        )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("audio", type=Path)
    parser.add_argument(
        "--preview", type=Path, help="Render a contact sheet without encoding the film"
    )
    render(parser.parse_args())
