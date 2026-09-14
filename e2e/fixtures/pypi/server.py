"""A mutable package index with real wheels, for frozen-install regressions."""
import base64
import hashlib
import io
import json
import pathlib
import sys
import tarfile
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

root = pathlib.Path(sys.argv[1])
root.mkdir(exist_ok=True)


def wheel(name, version, requires=(), script=False):
    module = name.replace('-', '_')
    dist = f'{module}-{version}.dist-info'
    files = {
        f'{dist}/METADATA': f'Metadata-Version: 2.1\nName: {name}\nVersion: {version}\nRequires-Python: >=3.10\n'
        + ('Provides-Extra: feature\n' if script else '')
        + ''.join(f'Requires-Dist: {req}\n' for req in requires),
        f'{dist}/WHEEL': 'Wheel-Version: 1.0\nGenerator: mise-test\nRoot-Is-Purelib: true\nTag: py3-none-any\n',
    }
    if script:
        files[f'{module}.py'] = "import importlib.metadata\ndef main():\n    print('dependency=' + importlib.metadata.version('mise-lock-dep'))\n    try:\n        print('extra=' + importlib.metadata.version('mise-lock-extra'))\n    except importlib.metadata.PackageNotFoundError:\n        pass\n"
        files[f'{dist}/entry_points.txt'] = f'[console_scripts]\nlock-cli = {module}:main\n'
    record = ''.join(
        f'{path},sha256={base64.urlsafe_b64encode(hashlib.sha256(text.encode()).digest()).decode().rstrip("=")},{len(text.encode())}\n'
        for path, text in files.items()
    )
    files[f'{dist}/RECORD'] = record + f'{dist}/RECORD,,\n'
    archive = io.BytesIO()
    with zipfile.ZipFile(archive, 'w') as z:
        for path, text in files.items():
            z.writestr(path, text)
    filename = f'{module}-{version}-py3-none-any.whl'
    return filename, archive.getvalue()


def sdist(name, version):
    module = name.replace('-', '_')
    root_name = f'{module}-{version}'
    backend = f'''import pathlib
import zipfile

def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    filename = "{module}-{version}-py3-none-any.whl"
    dist = "{module}-{version}.dist-info"
    files = {{
        "{module}.py": "def main():\\n    print('source-built')\\n",
        f"{{dist}}/METADATA": "Metadata-Version: 2.1\\nName: {name}\\nVersion: {version}\\nRequires-Python: >=3.10\\n",
        f"{{dist}}/WHEEL": "Wheel-Version: 1.0\\nGenerator: mise-test\\nRoot-Is-Purelib: true\\nTag: py3-none-any\\n",
        f"{{dist}}/entry_points.txt": "[console_scripts]\\nsource-cli = {module}:main\\n",
    }}
    files[f"{{dist}}/RECORD"] = "".join(f"{{path}},,\\n" for path in files) + f"{{dist}}/RECORD,,\\n"
    target = pathlib.Path(wheel_directory, filename)
    with zipfile.ZipFile(target, "w") as archive:
        for path, text in files.items():
            archive.writestr(path, text)
    return filename
'''
    files = {
        'pyproject.toml': '[build-system]\nrequires = []\nbuild-backend = "backend"\nbackend-path = ["."]\n',
        'backend.py': backend,
        'PKG-INFO': f'Metadata-Version: 2.1\nName: {name}\nVersion: {version}\nRequires-Python: >=3.10\n',
    }
    archive = io.BytesIO()
    with tarfile.open(fileobj=archive, mode='w:gz') as tar:
        for path, text in files.items():
            data = text.encode()
            info = tarfile.TarInfo(f'{root_name}/{path}')
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return f'{root_name}.tar.gz', archive.getvalue()


wheels = dict([
    wheel('mise-lock-cli', '1.0.0', ['mise-lock-dep>=1,<3', 'mise-lock-marker==1.0.0; python_version < \"3.12\"', 'mise-lock-extra==1.0.0; extra == \"feature\"'], script=True),
    wheel('mise-lock-dep', '1.0.0'),
    wheel('mise-lock-dep', '2.0.0'),
    wheel('mise-lock-marker', '1.0.0'),
    wheel('mise-lock-extra', '1.0.0'),
    sdist('mise-source-cli', '1.0.0'),
])


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_HEAD(self):
        self.do_GET()

    def do_GET(self):
        path = self.path.split('?')[0]
        if path.startswith('/pypi/') and path.endswith('/json'):
            body = json.dumps({'info': {'requires_python': '>=3.10'}, 'releases': {'1.0.0': [{}]}}).encode()
            content_type = 'application/json'
        elif path.startswith(('/simple/', '/pypi/simple/')):
            package = path.strip('/').split('/')[-1].replace('-', '_')
            body = ''.join(
                f'<a href="/files/{name}#sha256={hashlib.sha256(data).hexdigest()}" data-requires-python="&gt;=3.10">{name}</a>\n'
                for name, data in wheels.items()
                if name.startswith(package + '-') and ('-2.0.0-' not in name or (root / 'publish').exists())
            ).encode()
            content_type = 'text/html'
        elif path.startswith('/files/') and path.split('/')[-1] in wheels:
            body = wheels[path.split('/')[-1]]
            if (root / 'corrupt').exists():
                archive = io.BytesIO()
                with zipfile.ZipFile(io.BytesIO(body)) as original, zipfile.ZipFile(archive, 'w') as changed:
                    for name in original.namelist():
                        data = original.read(name)
                        if name.endswith('/METADATA'):
                            data += b'\nModified artifact\n'
                        changed.writestr(name, data)
                body = archive.getvalue()
            content_type = 'application/octet-stream'
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
(root / 'port').write_text(str(server.server_address[1]))
server.serve_forever()
