from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from subprocess import run


# OATs drives acceptance tests by sending HTTP requests into a running service.
# mise is a one-shot CLI, so this tiny shim exposes `/run` and turns that
# request into `mise run hello` so the OTEL traces/logs can be verified end to end.
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/run":
            self.send_response(404)
            self.end_headers()
            return

        result = run(["mise", "run", "hello"], capture_output=True, text=True)
        if result.returncode != 0:
            self.send_response(500)
            self.end_headers()
            self.wfile.write(result.stderr.encode())
            return

        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok\n")

    def log_message(self, format, *args):
        pass


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", 8080), Handler).serve_forever()
