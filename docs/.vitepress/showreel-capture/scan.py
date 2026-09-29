#!/usr/bin/env python3
"""Check recorded captures: forbidden text, timing text held on screen,
expected lines, and whether two runs agree.

usage: scan.py OUT_DIR            (reads OUT_DIR/runs/<run>/<capture>/frames.json)
       scan.py --compare RUN_DIR RUN_DIR [--report PATH]

Writes OUT_DIR/scan.json and prints a summary. Standard library only, so it
runs on the host after the containers are gone. --compare compares two
recorded runs from different invocations (out/runs/a of one, and of
another), the same way a --runs 2 invocation compares its runs a and b.

Forbidden text is reported wherever it appears on screen, with the time it
first and last shows, so a scene can see whether its cut avoids it. Nothing
here edits a capture.
"""
import difflib
import json
import os
import re
import sys

FORBIDDEN = [
    ("trust", re.compile(r"trust", re.I)),
    ("asdf", re.compile(r"asdf", re.I)),
    (".tool-versions", re.compile(r"\.tool-versions")),
    ("experimental", re.compile(r"experimental", re.I)),
    ("mise WARN", re.compile(r"mise WARN")),
    ("Finished in", re.compile(r"Finished in")),
    ("installed ... in", re.compile(r"installed \d+ tools?\b.* in ")),
    ("update notice", re.compile(r"mise version \S+ available")),
    ("release-age notice", re.compile(r"minimum_release_age|allowed cutoff")),
    ("dev build", re.compile(r"-DEBUG")),
]
NOTABLE = [
    ("mise hint", re.compile(r"mise hint")),
    ("mise ERROR", re.compile(r"mise ERROR")),
]
RATE = re.compile(r"\b\d+(?:\.\d+)?\s?(?:[kKMGT]i?B|B)/s\b")
DURATION = re.compile(r"(?<![\w.:/@-])\d+(?:\.\d+)?\s?(?:ms|s|m)\b(?![\w.-])")
HOLD_LIMIT = 0.1  # plan v2 timing.test: no timing text unchanged for longer


def load(capdir):
    doc = json.load(open(os.path.join(capdir, "frames.json")))
    meta = json.load(open(os.path.join(capdir, "meta.json")))
    texts = ["".join(t for t, _ in r) for r in doc["rows"]]
    return doc, meta, texts


def presence(doc, end_t):
    """row id -> list of [t_start, t_end] intervals it is on screen; the last
    frame stays up until the recording ends (end_t)."""
    fr = doc["frames"]
    spans, open_ = {}, {}
    for f in fr:
        now = set(f["rows"])
        for r in list(open_):
            if r not in now:
                spans.setdefault(r, []).append([open_.pop(r), f["t"]])
        for r in now:
            if r not in open_:
                open_[r] = f["t"]
    for r, t0 in open_.items():
        spans.setdefault(r, []).append([t0, end_t])
    return spans


# wall-clock text that differs between any two runs: timestamps (Postgres's
# log, `mise dot history`'s When column) and Postgres's process ids
CLOCK = re.compile(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?|(?<=UTC )\[\d+\]")


def norm(s):
    s = CLOCK.sub("<time>", s)
    s = RATE.sub("<rate>", s)
    return DURATION.sub("<dur>", s)


def screen(doc, texts, fi):
    lines = [texts[r] for r in doc["frames"][fi]["rows"]]
    while lines and not lines[-1].strip():
        lines.pop()
    return lines


def styled(doc, fi):
    out = []
    for r in doc["frames"][fi]["rows"]:
        runs = doc["rows"][r]
        out.append([[norm(t), doc["styles"][s]] for t, s in runs if t.strip()])
    while out and not out[-1]:
        out.pop()
    return out


def settled_rows(doc):
    """Row ids on screen at any mark: the output a command left behind, as
    opposed to progress rows that were redrawn away."""
    ids = set()
    for m in doc["marks"]:
        ids.update(doc["frames"][m["frame"]]["rows"])
    return ids


def output_lines(doc, texts):
    """Settled rows in the order they first appear."""
    keep = settled_rows(doc)
    seen, order = set(), []
    for f in doc["frames"]:
        for r in f["rows"]:
            if r in keep and r not in seen:
                seen.add(r)
                if texts[r].strip():
                    order.append(r)
    return order


HERE = os.path.dirname(os.path.abspath(__file__))


def captures(steps):
    return {c["id"]: c for c in steps["captures"] + steps.get("machine2", {}).get("captures", [])}


def expand(pat, values):
    return re.sub(r"\{\{([a-z0-9_]+)\}\}", lambda m: re.escape(values[m.group(1)]), pat)


def expects(cap, values, variant):
    """A capture's expected lines from steps.json, for this run and variant."""
    out = []
    for e in cap.get("expect", []):
        if isinstance(e, dict):
            if e.get("variant", variant) != variant:
                continue
            e = e["re"]
        out.append(expand(e, values))
    return out


def scan_capture(capdir, cap=None, values=None):
    doc, meta, texts = load(capdir)
    # a capture that shares its shell ends where its window ends
    spans = presence(doc, meta.get("window", {}).get("to_t", meta["duration"]))
    keep = settled_rows(doc)
    last = doc["frames"][-1]["rows"] if doc["frames"] else []
    res = {"id": meta["id"], "frames": len(doc["frames"]),
           "duration": meta["duration"], "errors": meta.get("errors", []),
           "forbidden": [], "notable": [], "timing": [], "expect": []}
    for r, txt in enumerate(texts):
        if r not in spans:
            continue
        for name, rx in FORBIDDEN + NOTABLE:
            if rx.search(txt):
                bucket = "forbidden" if (name, rx) in FORBIDDEN else "notable"
                res[bucket].append({"pattern": name, "text": txt,
                                    "shown": spans[r], "in_final": r in last})
        toks = RATE.findall(txt) + DURATION.findall(txt)
        if toks:
            held = max(b - a for a, b in spans[r])
            res["timing"].append({"tokens": toks, "text": txt, "shown": spans[r],
                                  "longest_hold": round(held, 3),
                                  "held": held > HOLD_LIMIT, "settled": r in keep,
                                  "in_final": r in last})
    pats = expects(cap, values, meta.get("variant")) if cap else meta.get("expect", [])
    for pat in pats:
        rx = re.compile(pat)
        hit = next((t for r, t in enumerate(texts) if r in spans and rx.search(t)), None)
        res["expect"].append({"pattern": pat, "ok": hit is not None, "line": hit})
    res["marks"] = {m["name"]: {"t": doc["frames"][m["frame"]]["t"],
                                "screen": screen(doc, texts, m["frame"])}
                    for m in doc["marks"]}
    return res, doc, texts, meta


def compare(a, b):
    (_, da, ta, _), (_, db, tb, _) = a, b
    out = {"marks": {}, "line_order": None}
    marks_b = {m["name"]: m for m in db["marks"]}
    for m in da["marks"]:
        n = m["name"]
        if n not in marks_b:
            out["marks"][n] = {"status": "missing in second run"}
            continue
        sa, sb = screen(da, ta, m["frame"]), screen(db, tb, marks_b[n]["frame"])
        if sa == sb:
            st = "identical"
        elif [norm(x) for x in sa] == [norm(x) for x in sb]:
            st = "same after masking durations, rates and clock times"
        else:
            st = "different"
        entry = {"status": st}
        if st != "identical":
            entry["diff"] = list(difflib.unified_diff(sa, sb, "run a", "run b", lineterm="", n=1))
        ca, cb = styled(da, m["frame"]), styled(db, marks_b[n]["frame"])
        entry["colors"] = "identical" if ca == cb else "different"
        if ca != cb:
            diffs = []
            for i in range(max(len(ca), len(cb))):
                x = ca[i] if i < len(ca) else None
                y = cb[i] if i < len(cb) else None
                if x != y:
                    diffs.append({"row": i, "a": x, "b": y})
            entry["color_diffs"] = diffs[:12]
        out["marks"][n] = entry
    la = [norm(ta[r]) for r in output_lines(da, ta)]
    lb = [norm(tb[r]) for r in output_lines(db, tb)]
    if la == lb:
        out["line_order"] = "identical"
    else:
        out["line_order"] = ("same lines, different order" if sorted(la) == sorted(lb)
                             else "different lines")
        out["line_order_diff"] = list(difflib.unified_diff(la, lb, "run a", "run b",
                                                           lineterm="", n=0))
    # colours of each settled line, whatever its position
    def colours(doc, texts):
        return {norm(texts[r]): [[norm(t), doc["styles"][s]] for t, s in doc["rows"][r]
                                 if t.strip()] for r in output_lines(doc, texts)}
    cola, colb = colours(da, ta), colours(db, tb)
    diffs = [{"line": k, "a": cola[k], "b": colb[k]} for k in cola
             if k in colb and cola[k] != colb[k]]
    out["line_colors"] = "identical" if not diffs else "different"
    if diffs:
        out["line_color_diffs"] = diffs[:12]
    return out


def compare_dirs(da, db, report_path):
    """Compare two run directories: every capture both have, the versions,
    the off-camera files and the file snapshots."""
    caps = captures(json.load(open(os.path.join(HERE, "steps.json"))))
    rep = {"a": da, "b": db, "captures": {}, "stability": {}, "versions": {}}
    for side, d in (("a", da), ("b", db)):
        vp = os.path.join(d, "versions.json")
        rep["versions"][side] = json.load(open(vp))["placeholders"] if os.path.exists(vp) else None
    rep["stability"]["versions"] = ("identical" if rep["versions"]["a"] == rep["versions"]["b"]
                                    else rep["versions"])
    files = {}
    for cap in sorted(set(os.listdir(da)) & set(os.listdir(db)), key=lambda c: (len(c), c)):
        ca, cb = os.path.join(da, cap), os.path.join(db, cap)
        if os.path.exists(os.path.join(ca, "frames.json")) and os.path.exists(
                os.path.join(cb, "frames.json")):
            la = scan_capture(ca, caps.get(cap), rep["versions"]["a"] or {})
            lb = scan_capture(cb, caps.get(cap), rep["versions"]["b"] or {})
            rep["captures"][cap] = {"a": la[0], "b": lb[0]}
            rep["stability"][cap] = compare(la, lb)
        for sub in ("files", "files-start", "offcam"):
            fa, fb = os.path.join(ca, sub), os.path.join(cb, sub)
            if not (os.path.isdir(fa) or os.path.isdir(fb)):
                continue
            names = set()
            for base in (fa, fb):
                for root, _, fs in os.walk(base):
                    names.update(os.path.relpath(os.path.join(root, f), base) for f in fs)
            for n in sorted(names):
                pa, pb = os.path.join(fa, n), os.path.join(fb, n)
                ta = open(pa, errors="replace").read() if os.path.exists(pa) else None
                tb = open(pb, errors="replace").read() if os.path.exists(pb) else None
                if ta == tb:
                    st = "identical"
                elif ta is None or tb is None:
                    st = "only in one run"
                elif norm(ta) == norm(tb):
                    st = "same after masking durations, rates and clock times"
                else:
                    st = "different"
                files[f"{cap}/{sub}/{n}"] = st
    rep["files"] = files
    with open(report_path, "w") as f:
        json.dump(rep, f, indent=2, ensure_ascii=False)
    summarize(rep)
    diff = {k: v for k, v in files.items() if v != "identical"}
    print(f"== files: {len(files) - len(diff)} of {len(files)} identical")
    for k, v in diff.items():
        print(f"  {k}: {v}")


def main():
    if sys.argv[1:2] == ["--compare"]:
        report = (sys.argv[5] if len(sys.argv) > 5 and sys.argv[4] == "--report"
                  else "compare.json")
        return compare_dirs(sys.argv[2], sys.argv[3], report)
    out = sys.argv[1]
    runs_dir = os.path.join(out, "runs")
    runs = sorted(d for d in os.listdir(runs_dir) if os.path.isdir(os.path.join(runs_dir, d)))
    report = {"runs": runs, "captures": {}, "stability": {}, "versions": {}}
    caps = captures(json.load(open(os.path.join(HERE, "steps.json"))))
    loaded = {}
    for run in runs:
        vp = os.path.join(runs_dir, run, "versions.json")
        if os.path.exists(vp):
            report["versions"][run] = json.load(open(vp))["placeholders"]
        for cap in sorted(os.listdir(os.path.join(runs_dir, run)), key=lambda c: (len(c), c)):
            capdir = os.path.join(runs_dir, run, cap)
            if not os.path.exists(os.path.join(capdir, "frames.json")) or cap.startswith("setup"):
                continue
            values = report["versions"].get(run, {})
            loaded[(run, cap)] = scan_capture(capdir, caps.get(cap), values)
            report["captures"].setdefault(cap, {})[run] = loaded[(run, cap)][0]
    if len(runs) >= 2:
        a, b = runs[0], runs[1]
        va, vb = report["versions"].get(a), report["versions"].get(b)
        report["stability"]["versions"] = "identical" if va == vb else {"a": va, "b": vb}
        for cap in report["captures"]:
            if (a, cap) in loaded and (b, cap) in loaded:
                report["stability"][cap] = compare(loaded[(a, cap)], loaded[(b, cap)])
    with open(os.path.join(out, "scan.json"), "w") as f:
        json.dump(report, f, indent=2, ensure_ascii=False)
    summarize(report)


def summarize(rep):
    for cap, per_run in rep["captures"].items():
        print(f"== {cap}")
        for run, r in per_run.items():
            bad = [e for e in r["expect"] if not e["ok"]]
            print(f"  run {run}: {r['frames']} frames, {r['duration']:.1f}s"
                  f"{', errors: ' + '; '.join(r['errors']) if r['errors'] else ''}"
                  f"; expect {len(r['expect']) - len(bad)}/{len(r['expect'])}")
            for e in bad:
                print(f"    MISSING expect: {e['pattern']}")
            for h in r["forbidden"]:
                s = h["shown"][0]
                print(f"    forbidden [{h['pattern']}] t={s[0]:.2f}-{h['shown'][-1][1]:.2f}"
                      f"{' (final)' if h['in_final'] else ''}: {h['text'].strip()}")
            for h in r["notable"]:
                print(f"    note [{h['pattern']}]: {h['text'].strip()}")
            for kind in ("settled", "transient"):
                held = [t for t in r["timing"] if t["held"] and t["settled"] == (kind == "settled")]
                if held:
                    longest = max(held, key=lambda t: t["longest_hold"])
                    print(f"    {kind} timing text held >{HOLD_LIMIT}s: {len(held)} rows, longest "
                          f"{longest['longest_hold']}s: {longest['text'].strip()}")
        st = rep["stability"].get(cap)
        if st:
            summary = {n: m["status"] + ("" if m["colors"] == "identical" else ", colors differ")
                       if "colors" in m else m["status"] for n, m in st["marks"].items()}
            print(f"  stability: line order {st['line_order']}; line colours "
                  f"{st['line_colors']}; marks {summary}")
            for line in st.get("line_order_diff", [])[:20]:
                print(f"    {line}")
    if "versions" in rep["stability"]:
        print(f"== versions across runs: {rep['stability']['versions']}")


if __name__ == "__main__":
    main()
