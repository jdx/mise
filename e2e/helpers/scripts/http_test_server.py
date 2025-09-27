#!/usr/bin/env python3
"""
Simple HTTP Server for E2E Testing
Serves test files to avoid external network dependencies
"""

import http.server
import socketserver
import sys
from pathlib import Path

class TestFileHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        """Handle GET requests for test files"""
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

    def log_message(self, format, *args):
        """Suppress log messages for cleaner test output"""
        pass

def find_available_port():
    """Find an available port starting from 8080"""
    import socket
    for port in range(8080, 8200):
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                s.bind(('', port))
                return port
        except OSError:
            continue
    raise RuntimeError("No available ports found")

def start_server(port=None):
    """Start the HTTP test server"""
    if port is None:
        port = find_available_port()

    with socketserver.TCPServer(("", port), TestFileHandler) as httpd:
        print(f"HTTP test server running on port {port}")

        # Save port info for tests
        with open('/tmp/mise_http_test_port', 'w') as f:
            f.write(str(port))

        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nShutting down...")
        finally:
            # Clean up port file
            Path('/tmp/mise_http_test_port').unlink(missing_ok=True)

if __name__ == '__main__':
    port = int(sys.argv[1]) if len(sys.argv) > 1 else None
    start_server(port)