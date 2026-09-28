#!/usr/bin/env python3
"""Resolve the versions the showreel shows, using the pinned mise itself.

usage: versions.py --mise PATH --cutoff ISO8601 --out versions.json
                   [--isolate DIR] [--rig DIR] [--print key|placeholders]

The rule (jdx, 2026-09-27):
  * LTS major: what this mise resolves "lts" to. That is the hand-kept
    ("lts", "<major>") alias in src/plugins/core/node.rs, read here from
    `mise tool-alias ls node`, never inferred from odd or even numbers.
  * Other major: the newest released Node major above the LTS major if there
    is one (`mise latest node`); otherwise the previous LTS major from mise's
    own lts-<codename> aliases.
  * Full versions for node (both majors), jq, npm:prettier, pitchfork and
    github:cli/cli come from `mise latest`, under the same
    minimum_release_age cutoff the captures use. hk is pinned to 2.1.0 and
    2.2.0 (only 2.2.0 has --junit-xml); both must be listed by ls-remote.
  * Postgres (C14): the newest major `mise latest postgres` gives, and the
    newest release of that major. The shop fixture reads the major.

The cache key. The captures are re-recorded (and the reel re-rendered) only
when something the reel is built around changes, never for noise:

  keyed      "schema" (bumped by hand to force a re-record), "rig" (the
             sha256 of the rig files that decide what the captures show:
             RIG_FILES below, which include the pinned hk pair, the fixture
             date and the Debian snapshot), and the two Node majors. When
             mise's `lts` alias moves, or Node ships a new major, the key
             changes and the reel follows by itself.
  not_keyed  everything else the captures print: the mise version and
             binary, Node's full versions, jq, npm:prettier, hk, pitchfork,
             gh and Postgres, and the cutoff. A patch release of any of them
             changes no scene and no caption, and a reel recorded before it
             still shows real output from a real run, so it does not
             re-record; the next capture records the new numbers.

key = sha256 of the keyed object serialised as json.dumps(keyed,
sort_keys=True, separators=(",", ":")). --print key writes exactly
{"key", "keyed", "not_keyed"} to stdout: the JSON the key is made from, and
the values it deliberately leaves out.

With --isolate, mise runs with HOME, every MISE_* directory, the system
config directory and the config search ceiling under DIR, so resolving never
touches the capture machine's state, the caller's config, or a stray
mise.toml above the working directory.
"""
import argparse
import concurrent.futures
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys

HK_OLD, HK_NEW = "2.1.0", "2.2.0"
# Bump to force every cached capture set to re-record.
KEY_SCHEMA = 1
# the rig files that decide what the captures show (README, scan, shape and
# export only describe, check or copy them)
RIG_FILES = ["Dockerfile", "entry.sh", "record.py", "frames.py", "fixtures.py",
             "versions.py", "steps.json"]


def rig_hashes(rig):
    """(combined sha256, {file: sha256}) of the rig files, in RIG_FILES order."""
    h, per = hashlib.sha256(), {}
    for name in RIG_FILES:
        body = open(os.path.join(rig, name), "rb").read()
        per[name] = hashlib.sha256(body).hexdigest()
        h.update(name.encode() + b"\0")
        h.update(body)
        h.update(b"\0")
    return h.hexdigest(), per


def key_of(keyed):
    return hashlib.sha256(json.dumps(keyed, sort_keys=True, separators=(",", ":"))
                          .encode()).hexdigest()


def key_doc(doc):
    """What --print key writes: the key, what it hashes, and what it leaves out."""
    return {"key": doc["key"], "keyed": doc["keyed"], "not_keyed": doc["not_keyed"]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--mise", required=True)
    ap.add_argument("--cutoff", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--isolate")
    ap.add_argument("--rig", help="hash these rig files into the key")
    ap.add_argument("--print", choices=["key", "placeholders"], default="placeholders")
    a = ap.parse_args()
    a.mise = os.path.abspath(a.mise)

    env = {
        "PATH": "/usr/local/bin:/usr/bin:/bin",
        "LANG": "C.UTF-8",
        "MISE_MINIMUM_RELEASE_AGE": a.cutoff,
        "MISE_DISABLE_UPDATE_WARNING": "1",
        "MISE_EXPERIMENTAL": "1",
        "MISE_HTTP_RETRIES": "5",
    }
    if a.isolate:
        d = os.path.abspath(a.isolate)
        for sub in ("home", "data", "cache", "state", "config", "system", "tmp"):
            os.makedirs(os.path.join(d, sub), exist_ok=True)
        env.update(HOME=f"{d}/home", XDG_CONFIG_HOME=f"{d}/home/.config",
                   XDG_DATA_HOME=f"{d}/home/.local/share",
                   XDG_CACHE_HOME=f"{d}/home/.cache",
                   XDG_STATE_HOME=f"{d}/home/.local/state",
                   MISE_DATA_DIR=f"{d}/data", MISE_CACHE_DIR=f"{d}/cache",
                   MISE_STATE_DIR=f"{d}/state", MISE_CONFIG_DIR=f"{d}/config",
                   MISE_GLOBAL_CONFIG_FILE=f"{d}/config/config.toml",
                   MISE_SYSTEM_CONFIG_DIR=f"{d}/system", MISE_TMP_DIR=f"{d}/tmp",
                   MISE_CEILING_PATHS=d)
        cwd = f"{d}/home"
    else:
        env["HOME"] = os.environ.get("HOME", "/tmp")
        cwd = env["HOME"]

    def mise(*args):
        p = subprocess.run([a.mise, *args], env=env, cwd=cwd,
                           capture_output=True, text=True)
        if p.returncode != 0:
            sys.exit(f"mise {' '.join(args)} failed ({p.returncode}):\n{p.stderr}")
        return p.stdout.strip(), p.stderr.strip()

    version_line, _ = mise("--version")
    mise_version = version_line.split()[0]
    if mise_version.endswith("-DEBUG") or "DEBUG" in version_line:
        sys.exit(f"refusing a dev build: {version_line}")

    notes = []

    # node: the aliases this mise ships
    out, _ = mise("tool-alias", "ls", "node")
    aliases = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[0] == "node":
            aliases[parts[1]] = parts[2]
    if "lts" not in aliases:
        sys.exit(f"no lts alias for node in this mise (the output of `mise tool-alias ls node` "
                 f"changed shape?):\n{out}")
    lts_alias = aliases["lts"]
    m = re.match(r"(\d+)", lts_alias)
    if not m:
        sys.exit(f"lts alias is not a major: {lts_alias!r}")
    lts_major = int(m.group(1))

    # the independent lookups run in parallel: each is one short request
    lookups = ["node@lts", f"node@{lts_alias}", "node", "jq", "npm:prettier", "pitchfork",
               "github:cli/cli", "postgres"]
    with concurrent.futures.ThreadPoolExecutor(len(lookups) + 1) as pool:
        hk_job = pool.submit(mise, "ls-remote", "hk")
        latest = dict(zip(lookups, pool.map(lambda t: mise("latest", t), lookups)))

    via_lts, lts_version = latest["node@lts"][0], latest[f"node@{lts_alias}"][0]
    if via_lts != lts_version:
        sys.exit(f"node@lts gave {via_lts}, node@{lts_alias} gave {lts_version}")
    newest = latest["node"][0]
    newest_major = int(newest.split(".")[0])
    if newest_major > lts_major:
        other_major, rule = newest_major, "newest-major-above-lts"
    else:
        prev = sorted({int(re.match(r"\d+", v).group()) for k, v in aliases.items()
                       if re.match(r"lts[-/]", k) and re.match(r"\d+", v)
                       and int(re.match(r"\d+", v).group()) < lts_major})
        if not prev:
            sys.exit("no Node major above the LTS and no earlier LTS alias")
        other_major, rule = prev[-1], "previous-lts-alias"

    tools = {}
    for tool in ("jq", "npm:prettier", "pitchfork", "github:cli/cli", "postgres"):
        v, err = latest[tool]
        tools[tool] = v
        if err:
            notes.append(f"mise latest {tool} printed on stderr (off camera): {err}")
    pg_major = re.match(r"(\d+)", tools.pop("postgres"))
    if not pg_major:
        sys.exit("mise latest postgres did not give a major version")
    pg_major = pg_major.group(1)
    with concurrent.futures.ThreadPoolExecutor(2) as pool:
        other_job = pool.submit(mise, "latest", f"node@{other_major}")
        pg_job = pool.submit(mise, "latest", f"postgres@{pg_major}")
        other_version, _ = other_job.result()
        tools["postgres"], err = pg_job.result()
    if err:
        notes.append(f"mise latest postgres@{pg_major} printed on stderr (off camera): {err}")
    if other_version.split(".")[0] != str(other_major):
        sys.exit(f"node@{other_major} resolved to {other_version}")

    hk_remote, err = hk_job.result()
    listed = hk_remote.split()
    for v in (HK_OLD, HK_NEW):
        if v not in listed:
            sys.exit(f"hk {v} is not installable under cutoff {a.cutoff}")
    if err:
        notes.append(f"mise ls-remote hk printed on stderr (off camera): {err}")
    tools["hk_old"], tools["hk_new"] = HK_OLD, HK_NEW

    sha = hashlib.sha256(open(a.mise, "rb").read()).hexdigest()
    rig_sha, rig_files = rig_hashes(a.rig) if a.rig else (None, {})
    keyed = {
        "schema": KEY_SCHEMA,
        "rig": rig_sha,
        "node_lts_major": lts_major,
        "node_other_major": other_major,
    }
    not_keyed = {
        "cutoff": a.cutoff,
        "mise": mise_version,
        "mise_sha256": sha,
        "node_lts_version": lts_version,
        "node_other_version": other_version,
        "node_other_rule": rule,
        "jq": tools["jq"],
        "npm:prettier": tools["npm:prettier"],
        "hk": [HK_OLD, HK_NEW],
        "pitchfork": tools["pitchfork"],
        "github:cli/cli": tools["github:cli/cli"],
        "postgres": tools["postgres"],
    }
    doc = {
        "schema": 2,
        "key": key_of(keyed),
        "keyed": keyed,
        "not_keyed": not_keyed,
        "resolved_at": datetime.datetime.now(datetime.timezone.utc)
        .strftime("%Y-%m-%dT%H:%M:%SZ"),
        "cutoff": a.cutoff,
        "mise": {"version": mise_version, "version_line": version_line,
                 "sha256": sha},
        "node": {
            "lts_alias": lts_alias,
            "lts_major": lts_major,
            "lts_version": lts_version,
            "newest_version": newest,
            "other_major": other_major,
            "other_version": other_version,
            "other_rule": rule,
            "lts_aliases": {k: v for k, v in aliases.items()
                            if k == "lts" or re.match(r"lts[-/]", k)},
        },
        "tools": tools,
        "rig": {"sha256": rig_sha, "files": rig_files},
        "notes": notes,
    }
    doc["placeholders"] = {
        "mise_version": mise_version,
        "lts_major": str(lts_major),
        "lts_version": lts_version,
        "other_major": str(other_major),
        "other_version": other_version,
        "jq_version": tools["jq"],
        "prettier_version": tools["npm:prettier"],
        "pitchfork_version": tools["pitchfork"],
        "gh_version": tools["github:cli/cli"],
        "hk_old": HK_OLD,
        "hk_new": HK_NEW,
        "postgres_major": pg_major,
        "postgres_version": tools["postgres"],
    }
    with open(a.out, "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    if a.print == "key":
        print(json.dumps(key_doc(doc), indent=2))
    else:
        print(json.dumps(doc["placeholders"]))


if __name__ == "__main__":
    main()
