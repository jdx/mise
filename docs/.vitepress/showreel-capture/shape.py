#!/usr/bin/env python3
"""Check that every capture has the shape the scenes are built for.

usage: shape.py OUT_DIR [--run RUN] [--report PATH]

Reads OUT_DIR/runs/<run>/<capture>/{frames,meta}.json and the "shape" of
each capture in steps.json (next to this file). Writes OUT_DIR/shape.json
(or --report; with --run, only that run is checked), prints one line per
failure, and exits 1 if anything failed, so a capture run that no longer
shows what the reel needs stops instead of shipping.

A capture fails when:
  * the recorder reported an error (a step timed out, no first prompt);
  * a line in "expect" never showed, or, with "order", they first showed
    out of order;
  * a line in "reject" showed;
  * "tail" (mark -> patterns) does not match the last non-blank rows on the
    screen at that mark ("cd prints nothing" is a tail of two prompts);
  * forbidden text (scan.py FORBIDDEN: trust, asdf, experimental, mise
    WARN, Finished in, install summaries, ...) showed and no "allow" rule
    covers it. An allow rule is either
      {"pattern": NAME, "rows_from": N}  only on screen rows N and below
                                         (a viewport crop, C1), or
      {"pattern": NAME, "after": RE}     only after a row matching RE had
                                         already shown, so the take has
                                         what it needs before it (a cut);
  * a row still on screen at a mark carries a duration or a rate, and no
    allow rule covers it;
  * C4 (steps.json host_captures): the registry list the pinned mise
    printed is missing, or has fewer names than "min_count" (the reel's
    "1,000+" rests on it).

For every row an "after" rule lets through, shape.json records the cut: the
last frame before that row first showed, with its time. Scenes cut there.
The expected lines and the rules are read from steps.json, so a recorded
run can be checked again after they change.
Standard library only.
"""
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import scan as SC  # noqa: E402


def check(capdir, cap, values):
    rule = cap.get("shape", {})
    doc, meta, texts = SC.load(capdir)
    frames = doc["frames"]
    fails, cuts, firsts = [], {}, {}
    for e in meta.get("errors", []):
        fails.append(f"recorder: {e}")
    if not frames:
        return fails + ["no frames"], cuts, firsts
    # first frame index and top-most screen row of every row id
    first, top = {}, {}
    for i, f in enumerate(frames):
        for y, r in enumerate(f["rows"]):
            first.setdefault(r, i)
            top[r] = min(top.get(r, y), y)

    def first_match(rx):
        hits = [first[r] for r in first if rx.search(texts[r])]
        return min(hits) if hits else None

    # expect, in order
    last_i = -1
    for pat in SC.expects(cap, values, meta.get("variant")):
        i = first_match(re.compile(pat))
        firsts[pat] = None if i is None else frames[i]["t"]
        if i is None:
            fails.append(f"missing expected line /{pat}/")
        elif rule.get("order") and i < last_i:
            fails.append(f"expected line /{pat}/ showed before the one listed ahead of it")
        if i is not None:
            last_i = max(last_i, i)
    for pat in rule.get("reject", []):
        pat = SC.expand(pat, values)
        for r in first:
            if re.search(pat, texts[r]):
                fails.append(f"rejected text /{pat}/ showed: {texts[r].strip()!r}")
                break
    marks = {m["name"]: m for m in doc["marks"]}
    for name, pats in rule.get("tail", {}).items():
        if name not in marks:
            fails.append(f"no mark {name!r} for its tail check")
            continue
        rows = [texts[r] for r in frames[marks[name]["frame"]]["rows"] if texts[r].strip()]
        got = rows[-len(pats):]
        if len(got) != len(pats) or not all(re.search(SC.expand(p, values), g)
                                            for p, g in zip(pats, got)):
            fails.append(f"screen at mark {name!r} ends {got!r}, expected {pats!r}")

    allows = rule.get("allow", [])
    covered = set()
    for r in first:
        for name, rx in SC.FORBIDDEN:
            if not rx.search(texts[r]):
                continue
            ok = False
            for a in allows:
                if a["pattern"] != name:
                    continue
                if "rows_from" in a and top[r] >= a["rows_from"]:
                    ok = True
                if "after" in a:
                    j = first_match(re.compile(SC.expand(a["after"], values)))
                    if j is not None and j < first[r]:
                        ok = True
                        cuts.setdefault(name, []).append({
                            "text": texts[r].strip(), "first_t": frames[first[r]]["t"],
                            "last_clean_frame": first[r] - 1,
                            "last_clean_t": frames[first[r] - 1]["t"]})
            if ok:
                covered.add(r)
            else:
                fails.append(f"forbidden [{name}] at t={frames[first[r]]['t']:.2f}, "
                             f"screen row {top[r]}: {texts[r].strip()!r}")
    for c in cuts.values():
        c.sort(key=lambda x: x["first_t"])
    settled = SC.settled_rows(doc)
    for r in settled:
        if r in covered or not texts[r].strip():
            continue
        toks = SC.RATE.findall(texts[r]) + SC.DURATION.findall(texts[r])
        if toks:
            fails.append(f"timing text {toks} stays on screen at a mark: {texts[r].strip()!r}")
    return fails, cuts, firsts


def registry_names(path):
    """C4: the short names `mise registry --hide-aliased` printed, in order."""
    if not os.path.exists(path):
        return None
    return [line.split()[0] for line in open(path) if line.strip()]


def check_c4(rd, steps):
    spec = steps.get("host_captures", {}).get("C4", {})
    names = registry_names(os.path.join(rd, "C4", "registry.txt"))
    if names is None:
        return ["no C4/registry.txt (machine 1's `mise registry` step did not run)"], 0
    fails = []
    if len(names) < spec.get("min_count", 1):
        fails.append(f"the registry lists {len(names)} names, fewer than {spec['min_count']}")
    if len(set(names)) != len(names):
        fails.append("the registry list repeats a name")
    return fails, len(names)


HINT = """
What to do: a capture no longer shows what the scenes are built around.
  * If mise, a tool or the network misbehaved, run it again.
  * If the output changed on purpose (a new mise release prints something
    else), update the capture's "expect" and "shape" in steps.json and the
    scene that shows it, then record again."""


def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--run", help="check only this run")
    ap.add_argument("--report", help="where to write the report (default OUT/shape.json)")
    ap.add_argument("--quiet", action="store_true", help="print failures only")
    a = ap.parse_args()
    out = a.out
    steps = json.load(open(os.path.join(HERE, "steps.json")))
    rules = SC.captures(steps)
    runs_dir = os.path.join(out, "runs")
    report, failed = {}, []
    for run in sorted(os.listdir(runs_dir)):
        rd = os.path.join(runs_dir, run)
        if not os.path.isdir(rd) or (a.run and run != a.run):
            continue
        values = json.load(open(os.path.join(rd, "versions.json")))["placeholders"]
        seen = set()
        for cap, spec in rules.items():
            capdir = os.path.join(rd, cap)
            meta_path = os.path.join(capdir, "meta.json")
            if not os.path.exists(meta_path):
                continue
            seen.add(cap)
            meta = json.load(open(meta_path))
            if meta.get("offcam"):
                report.setdefault(run, {})[cap] = {"ok": True, "offcam": True}
                continue
            fails, cuts, firsts = check(capdir, spec, values)
            report.setdefault(run, {})[cap] = {"ok": not fails, "failures": fails,
                                               "cuts": cuts, "expect_first_t": firsts,
                                               "variant": meta.get("variant")}
            for f in fails:
                failed.append(f"{cap} (run {run}): {f}")
        run_json = [os.path.join(rd, n) for n in ("run.json", "run-machine2.json")]
        until = any(json.load(open(p)).get("until") for p in run_json if os.path.exists(p))
        if not until:
            for cap in rules:
                if cap not in seen:
                    failed.append(f"{cap} (run {run}): not recorded")
            fails, count = check_c4(rd, steps)
            report.setdefault(run, {})["C4"] = {"ok": not fails, "failures": fails,
                                                "count": count}
            failed += [f"C4 (run {run}): {f}" for f in fails]
    with open(a.report or os.path.join(out, "shape.json"), "w") as f:
        json.dump({"ok": not failed, "failures": failed, "runs": report}, f, indent=2,
                  ensure_ascii=False)
    if failed:
        print(f"capture shape changed: {len(failed)} failure(s)", file=sys.stderr)
        for line in failed:
            print(f"  {line}", file=sys.stderr)
        if not a.quiet:
            print(HINT, file=sys.stderr)
        sys.exit(1)
    n = sum(len(v) for v in report.values())
    if not a.quiet:
        print(f"capture shape check passed: {n} captures in {len(report)} run(s)")


if __name__ == "__main__":
    main()
