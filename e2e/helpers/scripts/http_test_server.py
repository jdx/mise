#!/usr/bin/env python3
"""
HTTP server for e2e tests, so they do not depend on external services.

Start it with `start_http_server` from e2e/assert.sh rather than directly.

Usage:
    http_test_server.py --port-file FILE [--dir DIR] [--bind ADDR]
                        [--headers-log DIR] [--handler FILE [ARGS...]]

By default it answers the fixed routes in TestHandler.ROUTES and serves any
other path from --dir (the working directory if omitted).

--headers-log writes each request's method, path, and headers to
request_NNNN.json in that directory.

--handler loads a Python file that defines a `Handler` class to use instead of
TestHandler. The file runs with sys.argv set to [FILE, *ARGS], with the working
directory set to --dir, and with `TestHandler` in its globals so it can extend
the default behavior. It may also define `SERVER_CLASS` to replace
ThreadingHTTPServer, e.g. with a single-threaded http.server.HTTPServer.

The server exits when the process that started it (the test shell) does, so
tests do not need to kill it.
"""

import argparse
import http.server
import json
import os
import runpy
import sys
import threading
import time
from pathlib import Path

TASK_SCRIPT = '#!/usr/bin/env bash\necho "running mytask"\n'
# A remote task whose header names a task template, to check that the template
# is resolved for tasks fetched at run time
EXTENDS_TASK_SCRIPT = (
    "#!/usr/bin/env bash\n"
    '#MISE extends="shared"\n'
    'echo "extends-task FOO=$FOO"\n'
)
SLIDESHOW_JSON = json.dumps(
    {
        "slideshow": {
            "author": "Yours Truly",
            "date": "date of publication",
            "title": "Sample Slide Show",
        }
    }
)


class TestHandler(http.server.SimpleHTTPRequestHandler):
    # path -> (status, content type, body)
    ROUTES = {
        "/test/mytask": (200, "text/plain", TASK_SCRIPT.encode()),
        "/test/extends-task": (200, "text/plain", EXTENDS_TASK_SCRIPT.encode()),
        "/status/200": (200, "text/plain", b"OK"),
        "/status/201": (201, "text/plain", b"Created"),
        "/status/202": (202, "text/plain", b"Accepted"),
        "/json": (200, "application/json", SLIDESHOW_JSON.encode()),
        "/tool": (
            200,
            "application/octet-stream",
            b"#!/bin/sh\necho tool-stub-checksum\n",
        ),
    }

    headers_log_dir = None
    headers_log_lock = threading.Lock()
    remote_revision = 0
    remote_revision_lock = threading.Lock()

    def do_GET(self):
        self.log_headers()
        if not self.send_route(head=False):
            super().do_GET()

    def do_HEAD(self):
        self.log_headers()
        if not self.send_route(head=True):
            super().do_HEAD()

    def send_route(self, head):
        path = self.path.split("?", 1)[0]
        if path == "/test/remote-changing":
            # Each GET returns a new revision of the task, to test caching
            if head:
                revision = TestHandler.remote_revision
            else:
                with TestHandler.remote_revision_lock:
                    TestHandler.remote_revision += 1
                    revision = TestHandler.remote_revision
            body = (
                "#!/usr/bin/env bash\n"
                f'#MISE description="remote revision {revision}"\n'
                f'echo "remote revision {revision}"\n'
            ).encode()
            route = (200, "text/plain", body)
        else:
            route = self.ROUTES.get(path)
        if route is None:
            return False
        status, content_type, body = route
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if not head:
            self.wfile.write(body)
        return True

    def log_headers(self):
        if not self.headers_log_dir:
            return
        log_dir = Path(self.headers_log_dir)
        log_dir.mkdir(parents=True, exist_ok=True)
        entry = json.dumps(
            {"path": self.path, "method": self.command, "headers": dict(self.headers)},
            indent=2,
        )
        with TestHandler.headers_log_lock:
            next_num = len(list(log_dir.glob("request_*.json"))) + 1
            (log_dir / f"request_{next_num:04d}.json").write_text(entry)

    def log_message(self, format, *args):
        pass


def exit_with_parent():
    parent = os.getppid()
    while os.getppid() == parent:
        time.sleep(0.2)
    os._exit(0)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port-file", required=True)
    parser.add_argument("--dir", default=".")
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--headers-log")
    argv = sys.argv[1:]
    handler_argv = []
    if "--handler" in argv:
        # Everything after the handler file belongs to the handler
        at = argv.index("--handler") + 2
        argv, handler_argv = argv[:at], argv[at:]
    parser.add_argument("--handler")
    args = parser.parse_args(argv)

    threading.Thread(target=exit_with_parent, daemon=True).start()

    # Resolve paths before changing into the served directory
    port_file = Path(args.port_file).absolute()
    handler_file = args.handler and os.path.abspath(args.handler)
    TestHandler.headers_log_dir = args.headers_log and os.path.abspath(
        args.headers_log
    )
    os.chdir(args.dir)
    handler = TestHandler
    server_class = http.server.ThreadingHTTPServer
    if handler_file:
        sys.argv = [handler_file, *handler_argv]
        module = runpy.run_path(handler_file, init_globals={"TestHandler": TestHandler})
        handler = module["Handler"]
        server_class = module.get("SERVER_CLASS", server_class)

    server = server_class((args.bind, 0), handler)
    server.daemon_threads = True
    # Write then rename, so a reader never sees a partial port number
    tmp = port_file.with_name(port_file.name + ".tmp")
    tmp.write_text(str(server.server_address[1]))
    tmp.rename(port_file)
    server.serve_forever()


if __name__ == "__main__":
    main()
