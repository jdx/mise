import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

fixture = Path(__file__).parent
state = Path(sys.argv[1])
version = "0.1.0-dev.2"
repo = "mise-plugins/vfox-bfs"
base = f"https://github.com/{repo}/releases/download/v{version}"
release = {
    "tag_name": f"v{version}",
    "draft": False,
    "prerelease": True,
    "created_at": "2026-09-08T01:36:48Z",
    "published_at": "2026-09-08T01:36:48Z",
    "assets": [
        {"name": name, "url": f"{base}/{name}", "browser_download_url": f"{base}/{name}"}
        for name in ["packslip.sigstore.json", "vfox-bfs.tar.gz"]
    ],
}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path.endswith("/releases"):
            data = json.dumps([release]).encode()
        elif path.endswith("/tags"):
            data = json.dumps([{"name": f"v{version}", "commit": None}]).encode()
        elif "/releases/tags/" in path:
            data = json.dumps(release).encode()
        elif path.endswith("/packslip.sigstore.json"):
            data = (fixture / "packslip.sigstore.json").read_bytes()
        elif path.endswith("/vfox-bfs.tar.gz"):
            data = (fixture / "vfox-bfs.tar.gz").read_bytes()
            if (state / "corrupt").exists() and (state / "corrupt").read_text() == "yes":
                data += b"tampered"
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


server = HTTPServer(("127.0.0.1", 0), Handler)
(state / "port").write_text(str(server.server_port))
server.serve_forever()
