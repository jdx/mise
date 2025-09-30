#!/usr/bin/env python3
"""
HTTP Test Server for tool-stub E2E Testing
Serves mock endpoints to avoid external network dependencies
"""

import http.server
import socketserver
import sys
import json
from pathlib import Path

class ToolStubTestHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        """Handle GET requests for test endpoints"""
        if self.path == '/status/200':
            self.send_response(200)
            self.send_header('Content-Type', 'text/plain')
            self.end_headers()
            self.wfile.write(b'OK')
        elif self.path == '/status/201':
            self.send_response(201)
            self.send_header('Content-Type', 'text/plain')
            self.end_headers()
            self.wfile.write(b'Created')
        elif self.path == '/status/202':
            self.send_response(202)
            self.send_header('Content-Type', 'text/plain')
            self.end_headers()
            self.wfile.write(b'Accepted')
        elif self.path == '/json':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            content = json.dumps({
                "slideshow": {
                    "author": "Yours Truly",
                    "date": "date of publication",
                    "title": "Sample Slide Show"
                }
            })
            self.wfile.write(content.encode('utf-8'))
        else:
            # Return 404 for other paths
            self.send_error(404, "File not found")

    def log_message(self, format, *args):
        """Suppress log messages for cleaner test output"""
        pass

def start_server(port):
    """Start the HTTP test server"""
    with socketserver.TCPServer(("127.0.0.1", port), ToolStubTestHandler) as httpd:
        print(f"Tool stub test server running on port {port}", flush=True)
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nShutting down...", flush=True)

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("Usage: tool_stub_test_server.py <port>", file=sys.stderr)
        sys.exit(1)
    port = int(sys.argv[1])
    start_server(port)