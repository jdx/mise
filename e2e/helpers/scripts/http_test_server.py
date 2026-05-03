#!/usr/bin/env python3
"""
Simple HTTP Server for E2E Testing
Serves test files to avoid external network dependencies

Usage:
    http_test_server.py [port] [headers_log_dir]

If headers_log_dir is provided, request headers will be logged to files in that directory.
"""

import http.server
import json
import os
import socketserver
import sys
from pathlib import Path

# Global headers log directory (set via command line)
HEADERS_LOG_DIR = None


class TestFileHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        """Handle GET requests for test files"""
        self._log_headers()

        if self.path == '/test/mytask':
            # Return the test task script
            self.send_response(200)
            self.send_header('Content-Type', 'text/plain')
            self.end_headers()
            content = '#!/usr/bin/env bash\necho "running mytask"\n'
            self.wfile.write(content.encode('utf-8'))
        else:
            # Return 404 for other paths
            self.send_error(404, "File not found")

    def do_HEAD(self):
        """Handle HEAD requests"""
        self._log_headers()
        self.send_response(200)
        self.end_headers()

    def _log_headers(self):
        """Log request headers to file if log directory is configured"""
        if HEADERS_LOG_DIR:
            log_dir = Path(HEADERS_LOG_DIR)
            log_dir.mkdir(parents=True, exist_ok=True)

            # Use incrementing counter for log files
            existing = list(log_dir.glob("request_*.json"))
            next_num = len(existing) + 1
            log_file = log_dir / f"request_{next_num:04d}.json"

            headers_dict = dict(self.headers)
            log_data = {
                "path": self.path,
                "method": self.command,
                "headers": headers_dict,
            }
            log_file.write_text(json.dumps(log_data, indent=2))

    def log_message(self, format, *args):
        """Suppress log messages for cleaner test output"""
        pass


def start_server(port=None, headers_log_dir=None):
    """Start the HTTP test server"""
    global HEADERS_LOG_DIR
    HEADERS_LOG_DIR = headers_log_dir

    if port is None:
        port = 0

    with socketserver.TCPServer(("", port), TestFileHandler) as httpd:
        actual_port = httpd.server_address[1]
        print(f"HTTP test server running on port {actual_port}")

        # Save port info for tests. The e2e harness can place this under the
        # per-test TMPDIR so parallel runs do not share state.
        port_file = Path(os.environ.get('MISE_HTTP_TEST_PORT_FILE', '/tmp/mise_http_test_port'))
        port_file.parent.mkdir(parents=True, exist_ok=True)
        with open(port_file, 'w') as f:
            f.write(str(actual_port))

        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nShutting down...")
        finally:
            # Clean up port file
            port_file.unlink(missing_ok=True)


if __name__ == '__main__':
    port = int(sys.argv[1]) if len(sys.argv) > 1 else None
    headers_log_dir = sys.argv[2] if len(sys.argv) > 2 else None
    start_server(port, headers_log_dir)
