#!/usr/bin/env python3
'''Stand-in for `fnox env --json` (schema 1) in mise secrets e2e tests.

Knobs: FNOX_STUB_LOG (jsonl call log), FNOX_STUB_LEGACY=1 (no env command),
FNOX_STUB_SCHEMA=<n>, FNOX_STUB_FAIL=<key> (resolution error),
FNOX_STUB_MODE=garbage|extra|nul|ansi.
'''
import json
import os
import sys

PEM = '\n'.join(['-----BEGIN TEST KEY-----', 'pem-line-s3cr3t-0004', 'pem-line-s3cr3t-0005', 'Zm9v',
                 '-----END TEST KEY-----'])
SECRETS = {
    'DATABASE_URL': {'env': True, 'value': 'postgres://app:db-s3cr3t-0002@db/app', 'description': 'app database'},
    'DEPLOY_KEY': {'env': 'exec', 'value': 'deploy-s3cr3t-0001'},
    'STRIPE_KEY': {'env': 'exec', 'value': 'stripe-s3cr3t-0003'},
    'PEM_KEY': {'env': 'exec', 'value': PEM},
    'DB_PASSWORD': {'env': 'exec', 'value': 'pw-s3cr3t-0007'},
    'GCP_SA_JSON': {'env': 'exec', 'as_file': True, 'value': '{"private_key":"gcp-s3cr3t-0006"}'},
    'OPTIONAL_TOKEN': {'env': True, 'value': None},
    'SIGNING_KEY': {'env': False, 'value': 'signing-s3cr3t-0008', 'description': 'release signing key'},
}
LEASES = {'AWS_ACCESS_KEY_ID': ('aws', 'aws-s3cr3t-0009')}
SCRUB = ['FNOX_AGE_KEY', 'FNOX_AGE_KEY_FILE', 'ENPASS_PASSWORD', 'FNOX_ENPASS_PASSWORD']


def injectable(mode):
    return mode is True or mode == 'exec'


def out(doc, code=0):
    sys.stdout.write(json.dumps(doc) + '\n')
    sys.stdout.flush()
    sys.exit(code)


def main(argv):
    with open(os.environ.get('FNOX_STUB_LOG', 'fnox-calls.jsonl'), 'a') as log:
        log.write(json.dumps({
            'argv': argv,
            'cwd': os.getcwd(),
            'stdin_tty': sys.stdin.isatty(),
            'env_keys': sorted(os.environ),
            'path': os.environ.get('PATH', ''),
            'leaked': sorted(k for k, v in os.environ.items() if 's3cr3t' in v),
        }) + '\n')
    if argv == ['--version']:
        print('fnox 1.38.0')
        return 0
    profile, i = ['default'], 0
    while i < len(argv) and argv[i] != 'env':
        if argv[i] in ('-P', '--profile'):
            profile = argv[i + 1].split(',')
            i += 1
        i += 1
    if i == len(argv) or os.environ.get('FNOX_STUB_LEGACY'):
        print('error: unrecognized subcommand env', file=sys.stderr)
        return 2
    rest = argv[i + 1:]
    schema = int(os.environ.get('FNOX_STUB_SCHEMA', '1'))
    mode = os.environ.get('FNOX_STUB_MODE', '')
    if mode == 'garbage':
        sys.stdout.write('{"schema":1,"set":{"DEPLOY_KEY":"deploy-s3cr3t-0001"')
        return 0
    if '--describe' in rest:
        keys = []
        for k, s in SECRETS.items():
            info = {'key': k, 'kind': 'secret', 'env': s['env'], 'as_file': s.get('as_file', False),
                    'injectable': {'exec': injectable(s['env']), 'shell': s['env'] is True}}
            if 'description' in s:
                info['description'] = s['description']
            keys.append(info)
        for k, (lease, _) in LEASES.items():
            keys.append({'key': k, 'kind': 'lease', 'lease': lease, 'injectable': {'exec': True, 'shell': False}})
        version = '1.\x1b[31m38.0' if mode == 'ansi' else '1.38.0'
        if mode == 'ansi':
            profile = ['dev\x1b]0;pwned\x07']
        out({'schema': schema, 'fnox_version': version, 'profile': profile, 'keys': keys, 'dynamic_leases': []})
    if '--keys' in rest:
        keys = list(dict.fromkeys(rest[rest.index('--keys') + 1].split(',')))
    else:
        keys = [k for k, s in SECRETS.items() if injectable(s['env'])] + list(LEASES)
    unknown = [k for k in keys if k not in SECRETS and k not in LEASES]
    hidden = [k for k in keys if k in SECRETS and SECRETS[k]['env'] is False]
    if unknown or hidden:
        err = {'kind': 'invalid_keys', 'message': 'fnox env cannot provide: ' + ', '.join(unknown + hidden)}
        if unknown:
            err['unknown'] = unknown
        if hidden:
            err['not_injectable'] = [{'key': k, 'env': False} for k in hidden]
        out({'schema': schema, 'error': err}, 1)
    if os.environ.get('FNOX_STUB_FAIL') in keys:
        print('fnox: provider stub: not signed in', file=sys.stderr)
        out({'schema': schema, 'error': {'kind': 'resolution', 'message': 'provider stub: not signed in'}}, 1)
    sets, files, missing, leases = {}, {}, [], set()
    for k in keys:
        if k in LEASES:
            sets[k] = LEASES[k][1]
            leases.add(LEASES[k][0])
        elif SECRETS[k]['value'] is None:
            missing.append(k)
        elif SECRETS[k].get('as_file'):
            files[k] = SECRETS[k]['value']
        else:
            sets[k] = SECRETS[k]['value']
    if mode == 'nul' and 'DEPLOY_KEY' in sets:
        sets['DEPLOY_KEY'] = 'ab\u0000-s3cr3t-0001'
    if mode == 'extra':
        sets['EXTRA_KEY'] = 'extra-s3cr3t-0010'
        sets['SIGNING_KEY'] = SECRETS['SIGNING_KEY']['value']
    hidden_all = [k for k, s in SECRETS.items() if s['env'] is False]
    remove = [k for k in SCRUB + hidden_all if k not in sets and k not in files]
    out({'schema': schema, 'fnox_version': '1.38.0', 'scope': 'exec', 'profile': profile, 'set': sets,
         'files': files, 'remove': remove, 'missing': missing, 'leases': sorted(leases)})


sys.exit(main(sys.argv[1:]))
