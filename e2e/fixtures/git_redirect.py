"""HTTPS-to-HTTP redirect fixture; record whether Git follows the redirect."""

import http.server
import pathlib
import ssl
import sys
import threading


class Target(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        pathlib.Path("redirect-followed").touch()
        self.send_error(404)


target = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Target)
threading.Thread(target=target.serve_forever, daemon=True).start()


class Redirect(Target):
    def do_GET(self):
        pathlib.Path("redirect-requested").touch()
        self.send_response(302)
        self.send_header("Location", f"http://127.0.0.1:{target.server_port}/repo")
        self.end_headers()


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Redirect)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(sys.argv[1], sys.argv[2])
server.socket = context.wrap_socket(server.socket, server_side=True)
pathlib.Path("redirect-port").write_text(str(server.server_port))
server.serve_forever()
