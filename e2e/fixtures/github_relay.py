"""A credential-free Unix-socket Git smart-HTTP fixture for session adapter tests."""

import http.server
import os
import socketserver
import subprocess
import sys
import urllib.parse


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.forward()

    def do_POST(self):
        self.forward()

    def forward(self):
        url = urllib.parse.urlsplit(self.path)
        if self.command == "GET" and url.path == "/_session":
            self.send_response(204)
            self.end_headers()
            return
        if self.headers.get("Authorization"):
            self.send_error(500, "remote adapter must not send credentials")
            return
        if self.command == "GET" and url.path == "/api/repos/owner/repo/releases":
            content = b'[{"tag_name":"v1.2.3","draft":false,"prerelease":false,"created_at":"2026-01-01T00:00:00Z","assets":[]}]'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(content)))
            self.end_headers()
            self.wfile.write(content)
            return
        allowed = (
            self.command == "GET"
            and url.path == "/git/owner/repo.git/info/refs"
            and url.query == "service=git-upload-pack"
        ) or (
            self.command == "POST"
            and url.path == "/git/owner/repo.git/git-upload-pack"
        )
        if not allowed:
            self.send_error(403)
            return
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        env = dict(os.environ)
        env.update(
            GIT_PROJECT_ROOT=sys.argv[2],
            GIT_HTTP_EXPORT_ALL="1",
            PATH_INFO=url.path.removeprefix("/git"),
            QUERY_STRING=url.query,
            REQUEST_METHOD=self.command,
            CONTENT_TYPE=self.headers.get("Content-Type", ""),
            CONTENT_LENGTH=str(len(body)),
            HTTP_GIT_PROTOCOL=self.headers.get("Git-Protocol", ""),
        )
        result = subprocess.run(
            ["git", "http-backend"], input=body, env=env, capture_output=True, check=True
        )
        headers, content = result.stdout.split(b"\r\n\r\n", 1)
        self.send_response(200)
        for line in headers.decode().split("\r\n"):
            key, value = line.split(":", 1)
            if key.lower() == "content-type":
                self.send_header(key, value.strip())
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        self.wfile.write(content)


with socketserver.UnixStreamServer(sys.argv[1], Handler) as server:
    server.serve_forever()
