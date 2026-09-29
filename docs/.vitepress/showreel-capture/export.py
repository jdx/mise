#!/usr/bin/env python3
"""Export one recorded run as the showreel's committed reference capture set.

usage: export.py OUT_DIR REF_DIR [--run RUN]

REF_DIR (docs/.vitepress/theme/showreel/test/captures) is what the reel's
tests and renders read when no capture run is on the machine: PR CI has no
Docker and no network. It holds exactly what load.ts reads, laid out the
way OUT_DIR is, so the loader reads both with the same code:

  versions.json, C4.json         as the run wrote them
  source.json                    which run this is: its key, rig hash, cutoff,
                                 machine 2 variant and shape check
  runs/a/run-machine2.json       machine 2's variant
  runs/a/fixtures.json           the fixture files, as {path: text}
  runs/a/<id>/frames.json        the frames, without their raw byte offsets
  runs/a/<id>/marks.json         each mark's name and time
  runs/a/<id>/input.json         the keys typed
  runs/a/<id>/files.json         files/, files-start/ and offcam/ as
                                 {"files/work/api/mise.toml": text, ...}

Files are bundled into JSON so none of them acts on the checkout it is
committed to: a captured .gitignore would hide its neighbours from git, and
the linters would reformat a captured mise.toml or README.md, which must
stay byte for byte what the take printed. The loader reads a bundle and a
directory alike.

It leaves out what the loader never reads: raw.bin, events.json, meta.json
(which holds the full environment), the text dumps and the setup logs. The
run is always exported as "a", the loader's default.
"""
import argparse
import json
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def dump(obj, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(obj, f, ensure_ascii=False, separators=(",", ":"))
        f.write("\n")


def bundle(base, subs):
    """Every file under base/<sub>, as {"<sub>/<relative path>": text}."""
    out = {}
    for sub in subs:
        d = os.path.join(base, sub)
        for root, dirs, files in os.walk(d):
            dirs.sort()
            for name in sorted(files):
                p = os.path.join(root, name)
                # newline="": a captured \r stays a \r, as the loader reads it
                with open(p, encoding="utf-8", newline="") as f:
                    out[f"{sub}/{os.path.relpath(p, d)}"] = f.read()
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("ref")
    ap.add_argument("--run", default="a")
    a = ap.parse_args()
    rd = os.path.join(a.out, "runs", a.run)
    for f in ("versions.json", "C4.json", "shape.json"):
        if not os.path.exists(os.path.join(a.out, f)):
            sys.exit(f"export: {a.out} has no {f}; record a run first")
    shape = json.load(open(os.path.join(a.out, "shape.json")))
    if not shape.get("ok"):
        sys.exit("export: the run did not pass its shape check")
    steps = json.load(open(os.path.join(HERE, "steps.json")))
    ids = [c["id"] for c in steps["captures"] + steps["machine2"]["captures"]
           if not c.get("offcam")]

    # rebuilt from nothing, so a capture the rig no longer records goes too
    for name in ("versions.json", "C4.json", "source.json", "runs"):
        p = os.path.join(a.ref, name)
        if os.path.isdir(p):
            shutil.rmtree(p)
        elif os.path.exists(p):
            os.remove(p)
    versions = json.load(open(os.path.join(a.out, "versions.json")))
    dump(versions, os.path.join(a.ref, "versions.json"))
    dump(json.load(open(os.path.join(a.out, "C4.json"))), os.path.join(a.ref, "C4.json"))
    run_json = json.load(open(os.path.join(a.out, "run.json")))
    m2 = os.path.join(rd, "run-machine2.json")
    variant = json.load(open(m2)).get("variant") if os.path.exists(m2) else None
    if variant:
        dump({"variant": variant}, os.path.join(a.ref, "runs", "a", "run-machine2.json"))
    first = json.load(open(os.path.join(rd, ids[0], "meta.json")))
    dump({
        "about": "The showreel's reference capture set: one real run of "
                 "docs/.vitepress/showreel-capture, trimmed to what load.ts reads. "
                 "Regenerate with `mise run docs:showreel-capture -- --update-reference`.",
        "run": a.run,
        "recorded_at": first.get("started_at"),
        "key": versions.get("key"),
        "keyed": versions.get("keyed"),
        "rig": versions.get("rig"),
        "cutoff": versions.get("cutoff"),
        "machine2": variant,
        "shape_ok": True,
        "captures": ids,
        "elapsed_s": run_json.get("elapsed_s"),
    }, os.path.join(a.ref, "source.json"))
    dump({k[len("fixtures/"):]: v for k, v in bundle(rd, ["fixtures"]).items()},
         os.path.join(a.ref, "runs", "a", "fixtures.json"))
    for cid in ids:
        src = os.path.join(rd, cid)
        dst = os.path.join(a.ref, "runs", "a", cid)
        frames = json.load(open(os.path.join(src, "frames.json")))
        for f in frames["frames"]:
            f.pop("offset", None)
            f.pop("offset_end", None)
            if not f.get("cursor_hidden"):
                f.pop("cursor_hidden", None)
        for m in frames.get("marks", []):
            for k in ("offset", "step", "capture"):
                m.pop(k, None)
        dump(frames, os.path.join(dst, "frames.json"))
        marks = json.load(open(os.path.join(src, "marks.json")))
        dump([{"name": m["name"], "t": m["t"]} for m in marks], os.path.join(dst, "marks.json"))
        dump(json.load(open(os.path.join(src, "input.json"))), os.path.join(dst, "input.json"))
        files = bundle(src, ["files", "files-start", "offcam"])
        if files:
            dump(files, os.path.join(dst, "files.json"))
    total = sum(os.path.getsize(os.path.join(r, f))
                for r, _, fs in os.walk(a.ref) for f in fs)
    print(f"reference set: {len(ids)} captures from run {a.run} ({versions.get('key', '')[:12]}), "
          f"{total // 1024} KiB in {a.ref}")


if __name__ == "__main__":
    main()
