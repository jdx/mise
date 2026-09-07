#!/usr/bin/env python3
"""Protocol-only test double. Exposes the file key; NEVER use for real data."""
import base64
import os
import sys


def receive():
    header = sys.stdin.readline().strip().split()
    if not header:
        raise EOFError("missing command")
    body = ""
    while True:
        line = sys.stdin.readline().strip()
        body += line
        if len(line) < 64:
            break
    return header[1:], base64.b64decode(body + "=" * (-len(body) % 4))


def send(command, body=b"", reply=True):
    encoded = base64.b64encode(body).decode().rstrip("=")
    sys.stdout.write("-> " + command + "\n")
    for start in range(0, len(encoded), 64):
        sys.stdout.write(encoded[start : start + 64] + "\n")
    if len(encoded) % 64 == 0:
        sys.stdout.write("\n")
    sys.stdout.flush()
    if reply:
        return receive()
    return None


key = None
while True:
    command, body = receive()
    if command[0] == "done":
        break
    if command[0] == "wrap-file-key":
        key = body
    if command[:3] == ["recipient-stanza", "0", "mise-test"]:
        key = body

mode = os.environ.get("MISE_TEST_PLUGIN_MODE", "ok")
if mode == "malformed":
    print("invalid protocol", flush=True)
elif mode == "cancel":
    send("request-secret", b"Test PIN")
    send("error identity 0", b"authorization cancelled")
    send("done", reply=False)
elif key is not None:
    if "recipient-v1" in sys.argv[1]:
        send("recipient-stanza 0 mise-test", key)
    else:
        send("file-key 0", key)
    send("done", reply=False)
else:
    send("done", reply=False)
