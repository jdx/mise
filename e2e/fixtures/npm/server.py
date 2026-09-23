"""A metadata-only npm registry for tests that resolve versions but never install.

Serves full packuments at `/<name>` (scoped names as `/@scope%2fname`). Each
package mirrors the real registry's shape closely enough to pin a behavior:
publish dates for minimum_release_age, dist-tags that point past a cutoff, and
deprecated releases. Point mise at it with NPM_CONFIG_REGISTRY; see start.sh.
"""
import json
import pathlib
import sys
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

root = pathlib.Path(sys.argv[1])
root.mkdir(parents=True, exist_ok=True)

# name -> (latest dist-tag, {version: (publish time, deprecation message or None)})
PACKAGES = {
    # 3.0.0 is the first release after the 2023-06-01 cutoff the tests use, and
    # 3.1.0 the newest before 2024-01-01. 2.0.0 keeps relative cutoffs like 5y
    # resolvable.
    'prettier': ('3.6.2', {
        '2.0.0': ('2020-03-21T20:02:37.785Z', None),
        '2.8.7': ('2023-03-23T19:29:51.564Z', None),
        '2.8.8': ('2023-04-23T16:34:32.380Z', None),
        '3.0.0': ('2023-07-05T12:26:35.925Z', None),
        '3.1.0': ('2023-11-13T11:48:54.045Z', None),
        '3.6.2': ('2025-06-27T08:06:24.019Z', None),
    }),
    # 3.0.0 was published by accident and deprecated in favor of 2.x.
    'aws-cdk': ('2.1010.0', {
        '2.1006.0': ('2025-03-26T15:02:28.281Z', None),
        '2.1007.0': ('2025-04-01T13:05:07.736Z', None),
        '3.0.0': ('2025-04-02T20:48:44.207Z', 'aws-cdk@3.0.0 was released by mistake, use aws-cdk@2 instead'),
        '2.1010.0': ('2025-04-17T15:04:08.620Z', None),
    }),
    # Every release is deprecated, which npm treats as a deprecated package.
    'request': ('2.88.2', {
        '2.88.0': ('2018-08-10T19:58:27.595Z', 'request has been deprecated, see https://github.com/request/request/issues/3142'),
        '2.88.2': ('2020-02-11T16:35:40.588Z', 'request has been deprecated, see https://github.com/request/request/issues/3142'),
    }),
    'left-pad': ('1.3.0', {
        '1.2.0': ('2017-11-29T15:44:34.567Z', None),
        '1.3.0': ('2018-04-09T01:03:25.412Z', None),
    }),
}


def packument(name, base):
    latest, releases = PACKAGES[name]
    versions = {}
    for version, (_, deprecated) in releases.items():
        entry = {
            '_id': f'{name}@{version}',
            'name': name,
            'version': version,
            'dist': {
                'tarball': f'{base}/{name}/-/{name.split("/")[-1]}-{version}.tgz',
                'shasum': '0' * 40,
            },
        }
        if deprecated:
            entry['deprecated'] = deprecated
        versions[version] = entry
    times = {version: published for version, (published, _) in releases.items()}
    times['created'] = min(times.values())
    times['modified'] = max(times.values())
    # `npm view` labels its output with `_id`, so both levels carry one.
    return {'_id': name, 'name': name, 'dist-tags': {'latest': latest}, 'versions': versions, 'time': times}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_HEAD(self):
        self.do_GET()

    def do_GET(self):
        name = urllib.parse.unquote(self.path.split('?')[0].strip('/'))
        if name not in PACKAGES:
            self.send_error(404)
            return
        with (root / 'requests').open('a') as log:
            log.write(name + '\n')
        host, port = self.server.server_address[:2]
        body = json.dumps(packument(name, f'http://{host}:{port}')).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        if self.command != 'HEAD':
            self.wfile.write(body)


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
(root / 'port').write_text(str(server.server_address[1]))
server.serve_forever()
