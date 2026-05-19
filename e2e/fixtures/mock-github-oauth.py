import http.server
import json
import os
import urllib.parse


TOKEN = os.environ.get("MOCK_GITHUB_OAUTH_TOKEN", "ghu-native-oauth-token")
REFRESH_TOKEN = os.environ.get("MOCK_GITHUB_OAUTH_REFRESH_TOKEN", "ghr-native-refresh-token")
REFRESHED_TOKEN = os.environ.get(
    "MOCK_GITHUB_OAUTH_REFRESHED_TOKEN", f"{TOKEN}-refreshed"
)
PORT_FILE = os.environ.get(
    "MOCK_GITHUB_OAUTH_PORT_FILE",
    os.path.join(os.environ["HOME"], "mock-github-oauth-port"),
)


class Handler(http.server.BaseHTTPRequestHandler):
    device_token_count = 0

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length).decode()
        form = urllib.parse.parse_qs(body)

        if self.path == "/login/device/code":
            payload = {
                "device_code": "device-mock",
                "user_code": "ABCD-1234",
                "verification_uri": "https://github.com/login/device",
                "expires_in": 600,
                "interval": 1,
            }
        elif self.path == "/login/oauth/access_token":
            payload = self.token_payload(form)
        else:
            self.send_response(404)
            self.end_headers()
            return

        data = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def token_payload(self, form):
        grant_type = form.get("grant_type", [""])[0]
        if grant_type == "urn:ietf:params:oauth:grant-type:device_code":
            Handler.device_token_count += 1
            token = TOKEN
            if Handler.device_token_count > 1:
                token = f"{TOKEN}-{Handler.device_token_count}"
            return {
                "access_token": token,
                "expires_in": 28800,
                "refresh_token": REFRESH_TOKEN,
                "refresh_token_expires_in": 15897600,
                "token_type": "bearer",
                "scope": "",
            }
        if grant_type == "refresh_token":
            return {
                "access_token": REFRESHED_TOKEN,
                "expires_in": 28800,
                "token_type": "bearer",
                "scope": "",
            }
        return {"error": "unsupported_grant_type"}

    def log_message(self, format, *args):
        pass


server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
with open(PORT_FILE, "w") as f:
    f.write(str(server.server_address[1]))
server.serve_forever()
