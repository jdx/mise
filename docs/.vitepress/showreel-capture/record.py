#!/usr/bin/env python3
"""Record the showreel's terminal captures on one machine.

usage: record.py --machine machine1|machine2 --variant systemd|plain
                 --steps steps.json --out /out/RUN --cutoff ISO8601
                 [--versions versions.json] [--until C8]
                 [--mise /usr/local/bin/mise]

Runs inside a capture container as the user `you` (entry.sh starts it).
In order, it:

  1. resolves versions.json with the pinned mise (versions.py), in a
     throwaway HOME so the machine below starts empty; machine 2 is given
     machine 1's file with --versions instead;
  2. writes the fixture files from it (fixtures.py);
  3. runs the machine's "setup": the off-camera actions that make it;
  4. records each capture: an interactive zsh in a PTY, started in the
     capture's directory, typed into at a fixed speed. A command is done
     when its output has been quiet for a moment and the cursor sits after
     a fresh `%~ $ ` prompt, so a slow download never changes what is typed
     or when. A capture marked "same_shell" continues the previous
     capture's shell (C19 after C18); a capture marked "offcam" records
     nothing and only runs its setup (C16);
  5. replays every capture through pyte into frames.json (frames.py).

Each capture directory holds:
  raw.bin      every byte the terminal received, unmodified (for a shared
               shell, the whole session; meta.json "window" says which part
               is this capture)
  events.json  [t, offset, length] for each read (t in seconds from start)
  input.json   [t, text] for each key sent
  replies.json [t, text] for each answer the terminal gave a program's query
  marks.json   named points: the screen once a step's command has finished
  meta.json    size, directory, argv, the full environment, errors, expects
  files/       the mise config files as they were after the capture
  files-start/ the same files as the capture started ("snapshot_start")
  offcam/      output of off-camera commands that save it ("save", "copy")
  frames.json, final.txt, marks.txt, lines.txt (frames.py)

The terminal answers what a real terminal answers at once: a cursor
position report (CSI 6n), device status (CSI 5n), device attributes (CSI c)
and the default colours (OSC 10 and 11, with the reel's own terminal
colours). A program that asks and gets no answer waits for its timeout
(gh --version waits 5 s), which no real terminal would show.

Nothing is removed from the output. Off-camera actions are listed in the
steps file; their output goes to the machine's setup directory so it can be
checked too.
"""
import argparse
import datetime
import fcntl
import json
import os
import re
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import fixtures as FX  # noqa: E402,F401
import frames as FR  # noqa: E402

HOME = "/home/you"
# the reel's terminal (bible.ts TERM.text and TERM.window), as OSC 10 and 11
# report them
OSC_COLOURS = {b"10": "f3f3/eaea/f0f0", b"11": "2525/1d1d/2626"}
OSC_QUERY = re.compile(rb"\x1b\](10|11);\?(\x07|\x1b\\)")
PROMPT_RE = re.compile(r"^(~|/)\S* \$ $")
KEYS = {"tab": "\t", "ctrl-c": "\x03", "ctrl-l": "\x0c", "enter": "\r"}
PLACEHOLDER = re.compile(r"\{\{([a-z0-9_]+)\}\}")
MACHINES = {"machine1": {"setup_dir": "setup", "suffix": ""},
            "machine2": {"setup_dir": "setup-machine2", "suffix": "-machine2"}}


def base_env(cutoff, machine, variant):
    env = {
        "HOME": HOME,
        "USER": "you",
        "LOGNAME": "you",
        "SHELL": "/usr/bin/zsh",
        "LANG": "C.UTF-8",
        "TERM": "xterm-256color",
        "PATH": "/usr/local/bin:/usr/bin:/bin",
        # off camera: no config file on screen carries these
        "MISE_EXPERIMENTAL": "1",
        "MISE_TASK_TIMINGS": "0",
        "MISE_DISABLE_UPDATE_WARNING": "1",
        # a transient network error is retried instead of failing a take
        "MISE_HTTP_RETRIES": "5",
        # freezes what "latest" means for the whole run (default is 24h)
        "MISE_MINIMUM_RELEASE_AGE": cutoff,
    }
    if machine == "machine2" and variant == "systemd":
        # what pam_systemd gives a login session; `docker exec` has no login
        env["XDG_RUNTIME_DIR"] = "/run/user/1000"
        env["DBUS_SESSION_BUS_ADDRESS"] = "unix:path=/run/user/1000/bus"
    return env


def expand(v, values):
    if isinstance(v, str):
        return PLACEHOLDER.sub(lambda m: values[m.group(1)], v)
    if isinstance(v, list):
        return [expand(x, values) for x in v]
    if isinstance(v, dict):
        return {k: expand(x, values) for k, x in v.items()}
    return v


def applies(item, variant):
    """Items may carry "variant": "systemd" | "plain" and then apply only there."""
    return not (isinstance(item, dict) and "variant" in item and item["variant"] != variant)


def home(p):
    return HOME + p[1:] if p.startswith("~") else p


class Pty:
    def __init__(self, argv, env, cwd, cols, rows):
        master, slave = os.openpty()
        # the size is set before the shell starts, like `stty cols 80 rows N`
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        pid = os.fork()
        if pid == 0:
            try:
                os.close(master)
                os.setsid()
                fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
                for fd in (0, 1, 2):
                    os.dup2(slave, fd)
                if slave > 2:
                    os.close(slave)
                os.chdir(cwd)
                os.execvpe(argv[0], argv, env)
            finally:
                os._exit(127)
        os.close(slave)
        self.pid, self.fd = pid, master
        self.cols, self.rows = cols, rows
        self.t0 = time.monotonic()
        self.wall0 = datetime.datetime.now(datetime.timezone.utc)
        self.raw = bytearray()
        self.events, self.inputs, self.marks, self.replies = [], [], [], []
        self.screen = FR.Screen(cols, rows)
        # pyte answers CSI 5n, 6n and c through this hook; OSC queries are
        # answered in feed()
        self.screen.write_process_input = self.reply
        self.stream = FR.ByteStream(self.screen)
        self.tail = b""
        self.last_out = self.t0
        self.alive = True
        self.status = None

    def now(self):
        return round(time.monotonic() - self.t0, 6)

    def pump(self, dur):
        end = time.monotonic() + dur
        while self.alive:
            rem = end - time.monotonic()
            if rem <= 0:
                return
            r, _, _ = select.select([self.fd], [], [], min(rem, 0.01))
            if not r:
                continue
            try:
                data = os.read(self.fd, 65536)
            except OSError:
                data = b""
            if not data:
                self.alive = False
                return
            self.events.append([self.now(), len(self.raw), len(data)])
            self.raw += data
            self.feed(data)
            self.last_out = time.monotonic()

    def feed(self, data):
        """Feed the screen, answering each query in the order it was asked
        (a program that asks for a colour and then the cursor position reads
        the answers in that order)."""
        hay = self.tail + data
        base, pos = len(self.tail), 0
        for m in OSC_QUERY.finditer(hay):
            if m.end() <= base:
                continue  # answered with the read it ended in
            end = m.end() - base
            self.stream.feed(data[pos:end])
            pos = end
            self.reply(f"\x1b]{m.group(1).decode()};rgb:{OSC_COLOURS[m.group(1)]}"
                       + m.group(2).decode())
        self.stream.feed(data[pos:])
        self.tail = hay[-16:]

    def reply(self, text):
        os.write(self.fd, text.encode())
        self.replies.append([self.now(), text])

    def send(self, text):
        os.write(self.fd, text.encode())
        self.inputs.append([self.now(), text])

    def cursor_prefix(self):
        c = self.screen.cursor
        line = self.screen.buffer[c.y]
        return "".join(line[x].data for x in range(c.x))

    def at_prompt(self):
        return PROMPT_RE.match(self.cursor_prefix()) is not None

    def screen_lines(self):
        return ["".join(self.screen.buffer[y][x].data for x in range(self.cols)).rstrip()
                for y in range(self.rows)]

    def screen_has(self, rx):
        return any(rx.search(line) for line in self.screen_lines())

    def wait(self, kind, idle, timeout, since, match=None, dialog=None):
        """Wait until the step is done: "prompt" (the shell's prompt is back),
        "idle" (output went quiet) or "screen" (match is on screen). With
        dialog, a confirm prompt matching it on screen ends the wait early
        and returns "dialog", so the caller can answer it."""
        deadline = time.monotonic() + timeout
        rx = re.compile(match) if match else None
        drx = re.compile(dialog) if dialog else None
        while self.alive and time.monotonic() < deadline:
            self.pump(0.02)
            quiet = time.monotonic() - self.last_out
            got_output = self.last_out > since
            if quiet < idle:
                continue
            if kind == "prompt" and got_output and self.at_prompt():
                return True
            if kind == "idle" and got_output:
                return True
            if kind == "screen" and got_output and self.screen_has(rx):
                return True
            if drx and got_output and self.screen_has(drx):
                return "dialog"
        return False

    def exited(self):
        if self.status is not None:
            return True
        pid, st = os.waitpid(self.pid, os.WNOHANG)
        if pid:
            self.status = st
            return True
        return False

    def mark(self, name, step, capture=None):
        self.marks.append({"name": name, "t": self.now(), "offset": len(self.raw),
                           "step": step, "capture": capture})

    def close(self):
        if self.alive and not self.exited():
            try:
                os.killpg(self.pid, signal.SIGHUP)
            except ProcessLookupError:
                pass
            self.pump(0.5)
        for _ in range(50):
            if self.exited():
                break
            time.sleep(0.1)
        else:
            os.killpg(self.pid, signal.SIGKILL)
            _, self.status = os.waitpid(self.pid, 0)
        os.close(self.fd)

    def save(self, d, meta, marks=None):
        os.makedirs(d, exist_ok=True)
        with open(os.path.join(d, "raw.bin"), "wb") as f:
            f.write(self.raw)
        marks = self.marks if marks is None else marks
        for name, obj in (("events", self.events), ("input", self.inputs),
                          ("replies", self.replies), ("marks", marks)):
            with open(os.path.join(d, f"{name}.json"), "w") as f:
                json.dump(obj, f)
        meta = {**meta, "cols": self.cols, "rows": self.rows,
                "started_at": self.wall0.strftime("%Y-%m-%dT%H:%M:%S.%fZ"),
                "duration": self.now(), "bytes": len(self.raw)}
        with open(os.path.join(d, "meta.json"), "w") as f:
            json.dump(meta, f, indent=2)


class Machine:
    def __init__(self, env, fixtures, values, log, setup_dir, out, variant):
        self.env, self.fixtures, self.values = env, fixtures, values
        self.log, self.setup_dir, self.out = log, setup_dir, out
        self.variant = variant
        self.n = 0

    def say(self, msg):
        self.log.write(msg + "\n")
        self.log.flush()
        print(msg, file=sys.stderr, flush=True)

    def ops(self, ops):
        for op in ops:
            if not applies(op, self.variant):
                continue
            op = expand(op, self.values)
            if set(op) <= {"note", "variant"}:
                self.say(f"# {op.get('note', '')}")
            elif "mkdir" in op:
                os.makedirs(home(op["mkdir"]), exist_ok=True)
            elif "write" in op:
                dst = home(op["write"])
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                # with its fixed date (fixtures.py FIXTURE_DATE)
                shutil.copy2(os.path.join(self.fixtures, op["from"]), dst)
                if "mode" in op:
                    os.chmod(dst, int(op["mode"], 8))
                self.say(f"write {op['write']} <- fixtures/{op['from']}")
            elif "append" in op:
                with open(home(op["append"]), "a") as f:
                    f.write(open(os.path.join(self.fixtures, op["from"])).read())
                self.say(f"append {op['append']} <- fixtures/{op['from']}")
            elif "edit" in op:
                self.edit(op)
            elif "copy" in op:
                self.copy(op)
            elif "sh" in op:
                self.sh(op)
            elif "pty" in op:
                self.pty(op)
            elif "tmux" in op:
                self.tmux_start(op)
            elif "tmux_wait" in op:
                self.tmux_wait(op)
            elif "tmux_stop" in op:
                self.tmux_stop(op)
            else:
                raise SystemExit(f"unknown op {op}")

    def edit(self, op):
        path = home(op["edit"])
        text = open(path).read()
        if "replace" in op:
            if text.count(op["replace"]) != 1:
                raise SystemExit(f"{path}: expected one {op['replace']!r}")
            text = text.replace(op["replace"], op["with"])
        else:
            lines = text.split("\n")
            hits = [i for i, l in enumerate(lines) if l == op["after"]]
            if len(hits) != 1:
                raise SystemExit(f"{path}: expected one line {op['after']!r}")
            lines[hits[0] + 1:hits[0] + 1] = op["insert"].split("\n")
            text = "\n".join(lines)
        with open(path, "w") as f:
            f.write(text)
        self.say(f"edit {op['edit']}: {json.dumps({k: v for k, v in op.items() if k != 'edit'})}")

    def copy(self, op):
        """Copy a file the machine holds (C8: the packslip statement mise
        saved when it installed hk) into the capture set, as it is."""
        src, dst = home(op["copy"]), os.path.join(self.out, op["to"])
        if not os.path.isfile(src):
            raise SystemExit(f"copy: {op['copy']} does not exist")
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copyfile(src, dst)
        text = open(dst, errors="replace").read()
        self.say(f"copy {op['copy']} -> {op['to']} ({len(text)} bytes)")
        self.check(op, text, f"copy {op['copy']}")

    def save_output(self, op, text):
        if "save" in op:
            dst = os.path.join(self.out, op["save"])
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            with open(dst, "w") as f:
                f.write(text)

    def check(self, op, text, what):
        for pat in op.get("expect", []):
            if not re.search(pat, text, re.M):
                raise SystemExit(f"{what}: expected {pat!r} in its output:\n{text}")
        for pat in op.get("reject", []):
            if re.search(pat, text, re.M):
                raise SystemExit(f"{what}: {pat!r} must not print:\n{text}")

    def sh(self, op):
        cwd = home(op.get("cwd", "~"))
        self.say(f"$ (cd {op.get('cwd', '~')}) {op['sh']}")
        # every commit made off camera carries the fixture date, so what
        # machine 1 pushes is the same in every run
        env = {**self.env, "GIT_AUTHOR_DATE": FX.FIXTURE_DATE,
               "GIT_COMMITTER_DATE": FX.FIXTURE_DATE}
        # output goes to a file, not a pipe: a daemon the command starts (the
        # pitchfork supervisor, tmux) keeps inherited descriptors open, and a
        # pipe would never reach EOF
        with tempfile.TemporaryFile() as f:
            p = subprocess.run(["bash", "-c", op["sh"]], cwd=cwd, env=env,
                               stdin=subprocess.DEVNULL, stdout=f, stderr=subprocess.STDOUT)
            f.seek(0)
            text = f.read().decode(errors="replace")
        if op.get("quiet"):
            self.say(f"({len(text.splitlines())} lines, saved to {op.get('save')})")
        else:
            self.say(text.rstrip())
        self.save_output(op, text)
        if p.returncode != 0 and not op.get("allow_fail"):
            raise SystemExit(f"setup failed ({p.returncode}): {op['sh']}")
        self.check(op, text, f"setup `{op['sh']}`")

    def pty(self, op):
        self.n += 1
        name = op.get("log", f"pty{self.n}")
        cwd = home(op.get("cwd", "~"))
        self.say(f"$ (pty, cd {op.get('cwd', '~')}) {op['pty']}")
        p = Pty(["bash", "-c", op["pty"]], self.env, cwd, 80, 30)
        deadline = time.monotonic() + 900
        # done when the command exits, not when every descriptor closes
        while p.alive and not p.exited() and time.monotonic() < deadline:
            p.pump(0.1)
        p.pump(0.3)
        p.close()
        d = os.path.join(self.setup_dir, name)
        p.mark("end", -1)
        p.save(d, {"id": name, "cwd": op.get("cwd", "~"), "argv": ["bash", "-c", op["pty"]],
                   "env": self.env, "offcam": True})
        FR.build(d)
        text = open(os.path.join(d, "final.txt")).read()
        self.say(text.rstrip())
        if op.get("expect") and isinstance(op["expect"], str):
            op = {**op, "expect": [re.escape(op["expect"])]}
        self.check(op, text, f"off-camera pty {name}")

    # an off-camera terminal: a tmux window with the user's own shell, where
    # a long-running command (the history watcher) runs in the foreground
    def tmux(self, *args, check=True):
        return subprocess.run(["tmux", *args], env=self.env, cwd=HOME, check=check,
                              stdin=subprocess.DEVNULL, capture_output=True, text=True)

    def pane(self, session):
        return self.tmux("capture-pane", "-p", "-J", "-S", "-5000", "-t", session).stdout

    def tmux_start(self, op):
        s = op["session"]
        self.say(f"$ (tmux window {s}, cd {op.get('cwd', '~')}) {op['tmux']}")
        self.tmux("new-session", "-d", "-s", s, "-x", "80", "-y", "30",
                  "-c", home(op.get("cwd", "~")))
        self.tmux("set-option", "-t", s, "history-limit", "20000")
        self.tmux("set-option", "-t", s, "remain-on-exit", "on")
        # wait for the shell's prompt, then type the command into it
        self.tmux_wait({"tmux_wait": r"\$\s*$", "session": s, "timeout": 30})
        self.tmux("send-keys", "-t", s, "-l", op["tmux"])
        self.tmux("send-keys", "-t", s, "Enter")
        if "ready" in op:
            self.tmux_wait({"tmux_wait": op["ready"], "session": s,
                            "timeout": op.get("timeout", 60)})

    def tmux_wait(self, op):
        # capture-pane drops trailing blanks, so a prompt reads "~ $"
        s, rx = op["session"], re.compile(op["tmux_wait"], re.M)
        deadline = time.monotonic() + op.get("timeout", 60)
        while time.monotonic() < deadline:
            if rx.search(self.pane(s)):
                return
            time.sleep(0.2)
        raise SystemExit(f"tmux window {s}: {op['tmux_wait']!r} never showed:\n{self.pane(s)}")

    def tmux_stop(self, op):
        s = op["tmux_stop"]
        # the watcher reloads after a configuration change; stopping it
        # inside that window can record a spurious deletion, so wait until
        # its window has been unchanged for a while
        quiet, last, since = op.get("quiet", 8), None, time.monotonic()
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            text = self.pane(s)
            if text != last:
                last, since = text, time.monotonic()
            elif time.monotonic() - since >= quiet:
                break
            time.sleep(0.5)
        self.tmux("send-keys", "-t", s, "C-c")
        time.sleep(2)
        text = self.pane(s)
        log = os.path.join(self.setup_dir, f"{op.get('log', s)}.txt")
        with open(log, "w") as f:
            f.write(text)
        self.say(f"# tmux window {s}, as it was when stopped:\n{text.rstrip()}")
        self.tmux("kill-server", check=False)
        self.check(op, text, f"tmux window {s}")


def run_session(caps, steps_doc, machine_doc, machine, outdir, values, variant):
    """Record one or more captures in one interactive shell."""
    typing = steps_doc["typing"]
    first = caps[0]
    cols = first.get("cols", steps_doc["cols"])
    rows = first.get("rows", steps_doc["rows"])
    cwd = home(first["cwd"])
    os.makedirs(cwd, exist_ok=True)
    argv = ["zsh", "-i"]
    p = Pty(argv, machine.env, cwd, cols, rows)
    errors = {c["id"]: [] for c in caps}
    windows = {}
    for n, cap in enumerate(caps):
        machine.say(f"== {cap['id']}: {cap['title']} ({cols}x{rows}, {cap['cwd']})")
        if n == 0:
            if not p.wait("prompt", 0.5, 30, since=0):
                errors[cap["id"]].append("no first prompt")
        windows[cap["id"]] = {"from_offset": len(p.raw), "from_t": p.now()}
        p.mark("start", -1, cap["id"])
        if cap.get("snapshot_start"):
            snapshot(machine_doc, os.path.join(outdir, cap["id"]), "files-start")
        for i, st in enumerate(cap["steps"]):
            if not applies(st, variant):
                continue
            st = expand(st, values)
            if set(st) <= {"note", "variant"}:
                continue
            p.pump(st.get("before", typing["before"]))
            since = time.monotonic()
            if "type" in st:
                for ch in st["type"]:
                    p.send("\r" if ch == "\n" else ch)
                    p.pump(typing["char"])
                ends_line = st["type"].endswith("\n")
            else:
                p.send(KEYS[st["key"]])
                ends_line = st["key"] in ("ctrl-c", "ctrl-l", "enter")
            kind = st.get("wait", "prompt" if ends_line else "idle")
            idle = st.get("idle", 0.4 if kind == "prompt" else 0.8)
            if not ends_line and "type" in st and "wait" not in st:
                kind = None  # typed text with no Enter: nothing to wait for
            # "answer": confirm prompts this command asks along the way, each
            # answered once it has been on screen for a moment, like a person
            ans = st.get("answer")
            answered = 0
            deadline = time.monotonic() + st.get("timeout", 900)
            while kind:
                dialog = ans["match"] if ans and answered < ans.get("max", 1) else None
                got = p.wait(kind, idle, max(0, deadline - time.monotonic()), since,
                             st.get("match"), dialog)
                if got == "dialog":
                    answered += 1
                    p.mark(f"{ans.get('mark', 'confirm')}-{answered}", i, cap["id"])
                    p.pump(ans.get("hold", 1.0))
                    p.send(KEYS[ans.get("key", "enter")])
                    since = time.monotonic()
                    continue
                if not got:
                    screen = "\n".join(l for l in p.screen_lines() if l)
                    errors[cap["id"]].append(
                        f"step {i}: timed out waiting for {kind}"
                        + (f" {st.get('match')!r}" if st.get("match") else "")
                        + f" ({answered} prompt(s) answered); the screen was:\n{screen}")
                    p.send("\x03")
                    p.wait("prompt", 0.4, 15, time.monotonic())
                break
            if "mark" in st:
                p.mark(st["mark"], i, cap["id"])
            p.pump(st.get("hold", typing["hold"]) if "mark" in st else 0)
            if "offcam" in st:
                machine.ops(st["offcam"])
        p.pump(0.3)
        snapshot(machine_doc, os.path.join(outdir, cap["id"]))
        if cap.get("window_from"):
            # the capture starts at one of its own marks (C19: once Ctrl-L
            # has cleared C18's screen); the keys before it are in input.json
            m = [m for m in p.marks if m["capture"] == cap["id"] and m["name"] == cap["window_from"]]
            if not m:
                raise SystemExit(f"{cap['id']}: no mark {cap['window_from']!r} to start its window at")
            windows[cap["id"]].update(from_offset=m[0]["offset"], from_t=m[0]["t"])
        windows[cap["id"]]["to_offset"] = len(p.raw)
        windows[cap["id"]]["to_t"] = p.now()
        p.mark("end", -1, cap["id"])
    p.close()
    for cap in caps:
        d = os.path.join(outdir, cap["id"])
        meta = {"id": cap["id"], "title": cap["title"], "cwd": cap["cwd"],
                "machine": machine_name(steps_doc, cap), "variant": variant,
                "argv": argv, "env": machine.env, "errors": errors[cap["id"]],
                "steps": [expand(s, values) for s in cap["steps"] if applies(s, variant)],
                "expect": [expand(e["re"] if isinstance(e, dict) else e, values)
                           for e in cap.get("expect", []) if applies(e, variant)],
                "exit_status": p.status}
        if len(caps) > 1:
            meta["session"] = [c["id"] for c in caps]
            meta["window"] = windows[cap["id"]]
        p.save(d, meta, [m for m in p.marks if m["capture"] == cap["id"]])
        FR.build(d)
        machine.say(open(os.path.join(d, "final.txt")).read().rstrip())
        if errors[cap["id"]]:
            machine.say(f"!! {cap['id']}: {errors[cap['id']]}")
    return errors


def machine_name(steps_doc, cap):
    return "machine2" if cap in steps_doc.get("machine2", {}).get("captures", []) else "machine1"


def snapshot(machine_doc, d, sub="files"):
    snap = os.path.join(d, sub)
    for f in machine_doc.get("snapshot_files", []):
        src = home(f)
        if os.path.exists(src):
            dst = os.path.join(snap, os.path.relpath(src, HOME))
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.copyfile(src, dst)


def dpkg_versions(pkgs):
    out = {}
    for pkg in pkgs:
        p = subprocess.run(["dpkg-query", "-W", "-f=${Version}", pkg],
                           capture_output=True, text=True)
        out[pkg] = p.stdout.strip() or None
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--machine", choices=sorted(MACHINES), default="machine1")
    ap.add_argument("--variant", choices=["systemd", "plain"], default="plain")
    ap.add_argument("--steps", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--cutoff", required=True)
    ap.add_argument("--versions", help="use this versions.json instead of resolving")
    ap.add_argument("--mise", default="/usr/local/bin/mise")
    ap.add_argument("--until")
    a = ap.parse_args()

    os.makedirs(a.out, exist_ok=True)
    steps_doc = json.load(open(a.steps))
    machine_doc = steps_doc if a.machine == "machine1" else steps_doc["machine2"]
    names = MACHINES[a.machine]
    env = base_env(a.cutoff, a.machine, a.variant)

    versions_path = os.path.join(a.out, "versions.json")
    if a.versions:
        if os.path.abspath(a.versions) != os.path.abspath(versions_path):
            shutil.copyfile(a.versions, versions_path)
    else:
        subprocess.run([sys.executable, os.path.join(HERE, "versions.py"), "--mise", a.mise,
                        "--cutoff", a.cutoff, "--out", versions_path, "--rig", HERE,
                        "--isolate", "/tmp/showreel-resolve"], check=True)
        shutil.rmtree("/tmp/showreel-resolve", ignore_errors=True)
    versions = json.load(open(versions_path))
    values = {**versions["placeholders"], "variant": a.variant}

    fixtures = os.path.join(a.out, "fixtures")
    if a.machine == "machine1":
        subprocess.run([sys.executable, os.path.join(HERE, "fixtures.py"), versions_path,
                        fixtures], check=True)

    machine_info = {
        "machine": a.machine,
        "variant": a.variant,
        "image": os.environ.get("CAPTURE_IMAGE"),
        "packages": dpkg_versions(["zsh", "git", "tmux", "curl", "python3",
                                   "python3-pyte", "ca-certificates", "systemd"]),
        "mise": versions["mise"],
        "env": env,
    }
    with open(os.path.join(a.out, f"machine{names['suffix']}.json"), "w") as f:
        json.dump(machine_info, f, indent=2)

    setup_dir = os.path.join(a.out, names["setup_dir"])
    os.makedirs(setup_dir, exist_ok=True)
    log = open(os.path.join(setup_dir, "setup.log"), "w")
    m = Machine(env, fixtures, values, log, setup_dir, a.out, a.variant)
    m.say(f"# {a.machine} ({a.variant}), mise {versions['mise']['version_line']}")
    m.ops(machine_doc.get("setup", []))

    summary = {"machine": a.machine, "variant": a.variant, "until": a.until,
               "captures": {}}
    caps = machine_doc["captures"]
    i = 0
    stop = False
    while i < len(caps) and not stop:
        group = [caps[i]]
        while i + len(group) < len(caps) and caps[i + len(group)].get("same_shell"):
            group.append(caps[i + len(group)])
        i += len(group)
        for cap in group:
            if cap.get("setup"):
                m.say(f"# setup for {cap['id']}")
                if cap is group[0]:
                    m.ops(cap["setup"])
                else:
                    raise SystemExit(f"{cap['id']}: a same_shell capture cannot have setup")
        if group[0].get("offcam"):
            cap = group[0]
            m.say(f"== {cap['id']}: {cap['title']} (off camera)")
            d = os.path.join(a.out, cap["id"])
            os.makedirs(d, exist_ok=True)
            with open(os.path.join(d, "meta.json"), "w") as f:
                json.dump({"id": cap["id"], "title": cap["title"], "offcam": True,
                           "machine": a.machine, "variant": a.variant}, f, indent=2)
            snapshot(machine_doc, d)
            summary["captures"][cap["id"]] = {"errors": [], "offcam": True}
        else:
            errors = run_session(group, steps_doc, machine_doc, m, a.out, values, a.variant)
            for cap in group:
                summary["captures"][cap["id"]] = {"errors": errors[cap["id"]]}
        for cap in group:
            if cap.get("after"):
                m.say(f"# after {cap['id']}")
                m.ops(cap["after"])
            if a.until and cap["id"] == a.until:
                stop = True
    if not stop:
        m.ops(machine_doc.get("teardown", []))
    # what the machine holds afterwards, for checking the scenes' file cards
    tree = subprocess.run(
        ["find", ".", "-not", "-path", "./.local/*", "-not", "-path", "./.cache/*",
         "-not", "-path", "*/.git/*", "-not", "-path", "*/node_modules/*",
         "-not", "-path", "*/dist/*"],
        cwd=HOME, capture_output=True, text=True).stdout
    with open(os.path.join(a.out, f"tree{names['suffix']}.txt"), "w") as f:
        f.write("\n".join(sorted(tree.splitlines())) + "\n")
    with open(os.path.join(a.out, f"run{names['suffix']}.json"), "w") as f:
        json.dump(summary, f, indent=2)


if __name__ == "__main__":
    main()
