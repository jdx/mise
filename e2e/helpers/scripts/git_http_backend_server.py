#!/usr/bin/env python3
"""
Git HTTP Backend Server for E2E Testing
Serves git repositories using git-http-backend CGI
"""

import os
import sys
import subprocess
import tempfile
import shutil
import http.server
import socketserver
from pathlib import Path

class GitHTTPHandler(http.server.BaseHTTPRequestHandler):
    def __init__(self, *args, repo_dir=None, **kwargs):
        self.repo_dir = repo_dir
        super().__init__(*args, **kwargs)

    def do_GET(self):
        self.handle_git_request()

    def do_POST(self):
        self.handle_git_request()

    def handle_git_request(self):
        # Set up environment for git-http-backend
        env = os.environ.copy()
        env['GIT_PROJECT_ROOT'] = str(self.repo_dir)
        env['GIT_HTTP_EXPORT_ALL'] = '1'

        # Map /repo.git paths to /repo
        path_info = self.path
        if path_info.startswith('/repo.git'):
            path_info = path_info.replace('/repo.git', '/repo', 1)

        env['PATH_INFO'] = path_info
        env['REQUEST_METHOD'] = self.command
        env['QUERY_STRING'] = ''
        env['REMOTE_ADDR'] = self.client_address[0]

        if '?' in path_info:
            env['PATH_INFO'], env['QUERY_STRING'] = path_info.split('?', 1)

        # Read request body for POST
        content_length = int(self.headers.get('Content-Length', 0))
        request_body = self.rfile.read(content_length) if content_length > 0 else b''

        # Set content type if provided
        if 'Content-Type' in self.headers:
            env['CONTENT_TYPE'] = self.headers['Content-Type']

        # Run git-http-backend
        try:
            proc = subprocess.Popen(
                ['git', 'http-backend'],
                env=env,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE
            )

            stdout, stderr = proc.communicate(input=request_body)

            if proc.returncode == 0:
                # Parse CGI response
                headers_done = False
                lines = stdout.split(b'\n')

                # Default to 200 OK
                self.send_response(200)

                # Process headers
                body_start = len(lines)  # Default to end if no separator found
                for i, line in enumerate(lines):
                    if not headers_done:
                        if line == b'' or line == b'\r':
                            headers_done = True
                            body_start = i + 1
                            break
                        elif b':' in line:
                            header_line = line.decode('utf-8', errors='ignore').strip()
                            if ':' in header_line:
                                key, value = header_line.split(':', 1)
                                self.send_header(key.strip(), value.strip())

                # Always call end_headers even if no empty line was found
                if not headers_done:
                    # No empty line found, but we still need to end headers
                    body_start = 0  # Treat entire output as body if no headers found

                self.end_headers()

                # Send body content
                if body_start < len(lines):
                    body = b'\n'.join(lines[body_start:])
                    self.wfile.write(body)
            else:
                self.send_error(500, f"Git backend error: {stderr.decode()}")

        except Exception as e:
            self.send_error(500, f"Server error: {str(e)}")

def create_test_repo(repo_path):
    """Create a minimal test repository"""
    # Create a regular (non-bare) repository
    subprocess.run(['git', 'init', repo_path], check=True)

    # Configure git in the repository
    subprocess.run(['git', 'config', 'user.name', 'Test User'], cwd=repo_path, check=True)
    subprocess.run(['git', 'config', 'user.email', 'test@example.com'], cwd=repo_path, check=True)

    # Create test files
    xtasks_dir = Path(repo_path) / 'xtasks' / 'lint'
    xtasks_dir.mkdir(parents=True)

    ripgrep_file = xtasks_dir / 'ripgrep'
    ripgrep_file.write_text('#!/usr/bin/env bash\necho "ripgrep task executed"\n')
    ripgrep_file.chmod(0o755)

    # Commit files
    subprocess.run(['git', 'add', '.'], cwd=repo_path, check=True)
    subprocess.run(['git', 'commit', '-m', 'Add test files'], cwd=repo_path, check=True)
    subprocess.run(['git', 'tag', 'v2025.1.17'], cwd=repo_path, check=True)

    # Handle branch naming
    current_branch = subprocess.run(
        ['git', 'branch', '--show-current'],
        cwd=repo_path,
        capture_output=True,
        text=True
    ).stdout.strip()

    if current_branch != 'main':
        subprocess.run(['git', 'branch', '-m', current_branch, 'main'], cwd=repo_path, check=True)

    # Configure repo for HTTP serving
    subprocess.run(['git', 'config', 'http.receivepack', 'true'], cwd=repo_path, check=True)
    subprocess.run(['git', 'config', 'http.uploadpack', 'true'], cwd=repo_path, check=True)
    subprocess.run(['git', 'update-server-info'], cwd=repo_path, check=True)

def start_server(port=0):
    # Create temp directory
    temp_dir = Path(tempfile.mkdtemp(prefix='mise_git_http_'))
    repo_path = temp_dir / 'repo'

    print(f"Creating test repository at {repo_path}")
    create_test_repo(str(repo_path))

    # Create handler with repo directory
    def handler(*args, **kwargs):
        return GitHTTPHandler(*args, repo_dir=temp_dir, **kwargs)

    # Let the OS assign an available port if port=0
    # This avoids race conditions between finding and binding
    with socketserver.TCPServer(("", port), handler) as httpd:
        actual_port = httpd.server_address[1]
        print(f"Git HTTP server running on port {actual_port}")
        print(f"Repository URL: http://localhost:{actual_port}/repo.git")

        # Write the actual port to a file for the test to read
        port_file = Path('/tmp/mise_git_http_port')
        port_file.write_text(str(actual_port))

        # Also write a ready marker file
        ready_file = Path('/tmp/mise_git_http_ready')
        ready_file.write_text('ready')

        # Save cleanup info
        with open('/tmp/mise_git_http_info', 'w') as f:
            f.write(f"{temp_dir}\n")

        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nShutting down...")
        finally:
            shutil.rmtree(temp_dir, ignore_errors=True)
            port_file.unlink(missing_ok=True)
            ready_file.unlink(missing_ok=True)

if __name__ == '__main__':
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    start_server(port)