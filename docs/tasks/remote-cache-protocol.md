---
description: "Implement a server or client for version 1 of the protocol behind mise's remote task cache."
---

# Remote cache protocol <Badge type="warning" text="experimental" />

This page specifies version 1 of the protocol between mise and a remote task
cache. It is for people who implement a cache server or client. To use a remote
cache, see [Remote task cache](/tasks/remote-cache.html). The reference server
is `mbx-cache`, from
[`jdx/mr-boxington-cache`](https://github.com/jdx/mr-boxington-cache).
It is developed separately from mise; this page is the authoritative protocol
definition.

::: warning Experimental
The remote cache is experimental. The mise client uses it only with
`experimental = true`.
:::

In this specification, the lowercase words must, must not, should, and may state
requirement levels as defined in RFC 2119.

Version 1 has two stores: content-addressable storage (CAS) for blobs and
directory objects, keyed by digest, and action results, which map an action's
digest to its outputs and log. Splitting them lets clients deduplicate content
across actions and transfer blobs in parallel or in part, and lets a server
verify every referenced object before it publishes a hit. The protocol does not
expose mise's local cache directories, manifests, or archive formats; a client
may store entries locally in any form.

## Terminology

| Term                  | Meaning                                                                                                         |
| --------------------- | --------------------------------------------------------------------------------------------------------------- |
| Namespace             | An opaque authorization and isolation scope, usually an organization, repository, branch, pull request, or user |
| Action                | The typed canonical description of a build operation and every input that affects its result                    |
| Action result         | The immutable record published after an action completes successfully                                           |
| Blob                  | Uninterpreted bytes in CAS                                                                                      |
| Directory object      | Canonical JSON in CAS that describes files, subdirectories, and symbolic links                                  |
| Digest                | An algorithm, a lowercase hexadecimal hash, and the uncompressed byte length                                    |
| Commit                | Publication of an action result after every CAS object it references has been verified                          |
| Verified CAS          | CAS content whose bytes the server has checked against their digest                                             |
| CAS visibility domain | The CAS objects that a request's namespace is allowed to see                                                    |
| Read scope            | The single namespace that a read request queries                                                                |

## Transport and versioning

Version 1 uses HTTPS and HTTP semantics. Requests that carry authorization
credentials require HTTPS, except to loopback development servers (`localhost`,
`127.0.0.0/8`, and `::1`). Clients may connect to an unauthenticated
non-loopback HTTP service after they print a visible warning. That mode provides
neither confidentiality nor server authenticity: an on-path attacker can replace
an action result and its internally consistent CAS graph. Implementations may
use HTTP/1.1, HTTP/2, or HTTP/3.

Requests carry these headers:

| Header                | Value                                                                                                       |
| --------------------- | ----------------------------------------------------------------------------------------------------------- |
| `mbx-cache-protocol`  | `1`                                                                                                         |
| `mbx-cache-namespace` | The namespace for the operation. Servers must not require it on `GET /v1/capabilities` and `GET /v1/status` |
| `Authorization`       | `Bearer <token>` when the deployment uses bearer or OIDC tokens                                             |

The URL prefix `/v1` is the protocol's major version. Compatible additions are
advertised as capabilities and do not need a new URL prefix. An incompatible
wire or integrity change requires a new major version; version 1 must not be
used as an alias for an incompatible implementation.

Servers must not send unknown JSON response fields, and clients must not send
unknown request fields, unless a negotiated capability permits them.

## Endpoints

The last column shows which endpoints mise's task cache calls today.

| Endpoint                                           | Purpose                          | Used by the mise client |
| -------------------------------------------------- | -------------------------------- | ----------------------- |
| `GET /v1/capabilities`                             | Protocol version and features    | No                      |
| `GET /v1/status`                                   | Health check                     | No                      |
| `POST /v1/blobs:missing`                           | Find blobs the server lacks      | No                      |
| `GET /v1/blobs/{algorithm}/{hash}/{size}`          | Read a blob                      | Yes                     |
| `POST /v1/blobs:pack`                              | Read several blobs in one stream | No                      |
| `PUT /v1/blobs/{algorithm}/{hash}/{size}`          | Upload a blob                    | Yes                     |
| `POST /v1/uploads`                                 | Start an upload session          | No                      |
| `GET /v1/action-results/{algorithm}/{hash}/{size}` | Read an action result            | Yes                     |
| `PUT /v1/action-results/{algorithm}/{hash}/{size}` | Commit an action result          | Yes                     |

## Digests

The JSON representation of a digest is:

```json
{
  "algorithm": "blake3",
  "hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "size": 1234
}
```

Protocol version 1 defines `blake3` and `sha256`. Servers advertise the
algorithms they accept and must support BLAKE3. Action descriptors and
action-result keys always use BLAKE3; other CAS objects may use SHA-256 when the
server advertises it. A digest always covers the exact, uncompressed bytes and
includes their length. A server must reject malformed hashes, unsupported
algorithms, negative sizes, and content that does not match its declared digest.

Digest URL components use `/v1/blobs/{algorithm}/{hash}/{size}`. The algorithm
and hash must match the JSON representation, and `size` is an unsigned decimal
integer.

## Capabilities

`GET /v1/capabilities` requires no namespace and returns the protocol version
and server limits:

```json
{
  "protocol": { "major": 1, "minor": 0 },
  "digest_algorithms": ["blake3", "sha256"],
  "compressors": ["identity", "zstd"],
  "action_kinds": {
    "task": { "action_schema": 1, "metadata_schema": 1 }
  },
  "features": {
    "batch": true,
    "blob_packs": true,
    "resumable_uploads": true,
    "delegated_transfers": true
  },
  "limits": {
    "max_batch_items": 1000,
    "max_inline_blob_bytes": 1048576,
    "max_blob_bytes": 107374182400,
    "max_pack_bytes": 107374182400
  }
}
```

Each `action_kinds` entry advertises the action-descriptor and client-metadata
schema versions the server validates for that kind. Clients must not read or
publish a non-`task` action unless the server advertises the kind and the exact
schema versions the client implements. Servers must reject unadvertised kinds
and unsupported schema versions. Compatible protocol additions may add kinds or
schema versions without changing the major version.

Clients must honor advertised limits and fall back when an optional feature is
absent. Servers return
`426 Upgrade Required` for unsupported major versions and include their
supported major version in `mbx-cache-protocol`.

`GET /v1/status` is an operational health endpoint. A successful response means
the API process is live; it does not replace capability negotiation or an
authorization check.

## Canonical objects

Protocol JSON objects use UTF-8 and the JSON Canonicalization Scheme (RFC 8785)
whenever their bytes are hashed. Duplicate object keys, invalid UTF-8,
non-canonical encodings, and values the declared schema cannot represent must be
rejected. The examples on this page are indented for reading; canonical bytes
have no whitespace and sorted keys.

### Action descriptor

An action descriptor contains a stable action kind and everything declared to
affect its result. Action schema version 1 defines `task`:

```json
{
  "version": 1,
  "kind": "task",
  "task": "build",
  "phase": "normal",
  "run": ["cargo build --release"],
  "args": [],
  "shell": null,
  "outputs": ["target/release/widget"],
  "root": "crates/widget",
  "source_hash": "3efccd93f9ade4f470d8902c431ae261a20c8f38191becd8906d21c4aed4ba8a",
  "environment": { "PROFILE": "release" },
  "vars": {},
  "tools": ["core:rust@1.92.0"],
  "os": "linux",
  "arch": "x86_64"
}
```

| Field             | JSON type        | Contents                                                                                                                                                     |
| ----------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `version`         | number           | `1`                                                                                                                                                          |
| `kind`            | string           | `task`                                                                                                                                                       |
| `task`            | string           | The task name                                                                                                                                                |
| `phase`           | string           | `normal`, or `post` when the task runs as a `depends_post` dependency                                                                                        |
| `run`             | array            | The task's `run` entries in declaration order. A script is a string; a task reference is an object such as `{ "task": "lint" }` or `{ "tasks": ["a", "b"] }` |
| `args`            | array of strings | The task's arguments in order                                                                                                                                |
| `shell`           | string or null   | The task's shell override                                                                                                                                    |
| `outputs`         | array of strings | Output patterns in declaration order, including `!` exclusions                                                                                               |
| `root`            | string           | The task directory relative to the outermost config root; `""` for a task at that root                                                                       |
| `source_hash`     | string           | A 64-character lowercase hexadecimal BLAKE3 digest over the source paths and contents, with no algorithm prefix                                              |
| `dependency_keys` | array of strings | Cache keys of cached dependencies, sorted and deduplicated. Omitted when empty                                                                               |
| `environment`     | object           | Variable name to value, or `null` for an unset variable                                                                                                      |
| `command_inputs`  | array of objects | One `{ "command", "stdout_hash", "stderr_hash" }` object per command input, in declaration order. Omitted when empty                                         |
| `vars`            | object           | mise variable name to value                                                                                                                                  |
| `tools`           | array of strings | Resolved tools as `backend:name@version`, sorted                                                                                                             |
| `os`              | string           | The client's operating system, such as `linux` or `macos`                                                                                                    |
| `arch`            | string           | The client's architecture, such as `x86_64` or `aarch64`                                                                                                     |

Version strings are opaque and are never semantically ordered. Every action kind
defines its own canonical fields and cacheability rules; a server may reject
kinds it does not advertise.

Secrets must not appear in an action descriptor. mise's client writes the
resolved values of the task's `env` entries, of every variable named in
`cache.env` or `task_config.global_env`, and of every mise variable visible to
the task (config-level `[vars]` as well as task `vars`) into the descriptor
verbatim, and uploads it to the remote CAS. `redactions` and `redact = true` are
not applied. Only tasks that receive `secrets` grants are excluded from caching.
Keep secret values out of those fields for any remotely cached task, and use
`pass_through_env` for credentials.

`source_hash` binds the declared source paths and contents without uploading
task inputs that cache-only operation does not need. The canonical descriptor is
stored in CAS. Its digest is the action digest and the action-result URL key.
Two clients that describe the same action must produce identical canonical
bytes.

### Directory object

A directory object has media type `application/vnd.mise.cache-directory.v1+json`:

```json
{
  "version": 1,
  "directories": [
    {
      "name": "assets",
      "digest": { "algorithm": "blake3", "hash": "...", "size": 321 },
      "mode": 493
    }
  ],
  "files": [
    {
      "name": "widget",
      "digest": { "algorithm": "blake3", "hash": "...", "size": 123456 },
      "executable": true,
      "mode": 493
    }
  ],
  "symlinks": [{ "name": "current", "target": "widget", "mode": 511 }]
}
```

Each node list is sorted by the UTF-8 bytes of `name`. A name must be a single
path component and must not be empty, `.`, or `..`, contain a slash or NUL, or
collide with another node. Absolute symlink targets and targets that escape the
declared output root must be rejected during restoration.

The portable metadata set is file contents, directory structure, symbolic links,
executable state, and the portable permission bits in `mode`. Owners, groups,
timestamps, devices, sockets, FIFOs, platform ACLs, and extended attributes are
not restored. Hard links may be restored as independent files. A task result
that contains an unsupported object is not eligible for remote caching; the
object is never silently changed.

### Action result

An action-result response and commit body have media type
`application/vnd.mise.cache-action-result.v1+json`:

```json
{
  "version": 1,
  "action": { "algorithm": "blake3", "hash": "...", "size": 789 },
  "output_root": { "algorithm": "blake3", "hash": "...", "size": 456 },
  "metadata": { "algorithm": "blake3", "hash": "...", "size": 234 }
}
```

Only successful, cacheable action executions may be published. `output_root` is
absent when an action has no output files. `metadata` references canonical
`application/vnd.mise.cache-client-metadata.v1+json` that contains typed client
metadata. Task metadata contains the output roots, the captured log, the task
identity, an estimate of the restored bytes, and the execution duration. The
metadata schema is part of the remote protocol and independent of mise's local
cache manifests.

```json
{
  "version": 1,
  "kind": "task",
  "task_identity": "f8ad8c7267ce409520fd6d9d242a6341bbc62bd8ef817317f1f8e1128297daf6",
  "roots": ["target/release/widget"],
  "output": [{ "stream": "stdout", "line": "built widget" }],
  "restored_bytes": 123456,
  "execution_duration_ns": 900000000
}
```

`task_identity` is an opaque digest that the client defines; servers must not
interpret it. Each metadata kind has a versioned schema. Task root paths use
forward slashes, are relative to the task working directory, and must satisfy
the same path-safety rules as directory nodes. Task output entries keep their
order.

The metadata `kind` must equal the referenced action descriptor's `kind`.
Servers reject a commit with mismatched kinds before publication, even when both
objects satisfy their schemas on their own.

The action descriptor and every object reachable from the result must exist and
validate before the result becomes readable.

Retention, last-access time, quota accounting, internal storage location, and
server annotations are not part of the immutable action result.

## CAS operations

### Find missing blobs

`POST /v1/blobs:missing` accepts `application/vnd.mise.cache-digests.v1+json`:

```json
{ "digests": [{ "algorithm": "blake3", "hash": "...", "size": 1234 }] }
```

It returns `200 OK` with the subset not present in verified CAS:

```json
{ "missing": [{ "algorithm": "blake3", "hash": "...", "size": 1234 }] }
```

The server must not disclose whether objects exist outside the request's
readable namespaces or CAS visibility domain.

### Read a blob

`GET /v1/blobs/{algorithm}/{hash}/{size}` returns `200 OK`, or `404 Not Found`
when the caller cannot read the object. The response includes `Content-Length`.
Clients verify the bytes against the digest in the URL, so servers need not send
a digest header. Servers may honor `Range` and may return a negotiated
`Content-Encoding: zstd`; the URL digest always describes the uncompressed
bytes.

A server that advertises delegated transfers may return
`307 Temporary Redirect` to a short-lived HTTPS URL. The redirect must grant
access only to the requested immutable object. Clients must not forward the
cache service's `Authorization` header to the delegated host.

Clients verify the complete uncompressed digest before they use downloaded
content. A mismatch is a cache miss and produces a visible integrity warning.

### Read a blob pack

Servers that advertise `features.blob_packs` accept `POST /v1/blobs:pack` with
the same `application/vnd.mise.cache-digests.v1+json` body as `blobs:missing`.
The aggregate declared size must not exceed `limits.max_pack_bytes`, and the
number of digests must not exceed `limits.max_batch_items`. Servers return
`400 Bad Request` when the item limit is exceeded and `413 Content Too Large`
when the aggregate declared size exceeds the byte limit.

A successful response uses `application/vnd.mise.cache-blob-pack.v1` and begins
with the eight-byte ASCII magic `MISEPK01`. The rest is a stream of frames in
request order:

| Field     | Encoding                                    |
| --------- | ------------------------------------------- |
| Algorithm | one byte: `1` for BLAKE3 or `2` for SHA-256 |
| Hash      | raw 32-byte digest                          |
| Size      | unsigned big-endian 64-bit byte length      |
| Content   | exactly `size` bytes                        |

A server may also send `mbx-cache-pack-blobs` (the number of frames) and
`mbx-cache-pack-bytes` (the sum of the frame content sizes). When they are
present, they and `Content-Length` must match the decoded stream; clients reject
a pack whose metadata disagrees.

The server omits missing and unauthorized blobs and sends a blob requested more
than once only once. Clients reject unrequested or duplicate frames, stream each
frame to bounded temporary storage, verify its full digest, and only then admit
it to local CAS. Clients fall back to single-blob reads when the capability is
absent, a digest exceeds the advertised pack limit, or an expected blob is
omitted. A pack is only a transfer optimization; its framing does not change CAS
identity or action semantics.

### Upload blobs

Small blobs may be sent directly with `PUT /v1/blobs/{algorithm}/{hash}/{size}`
and `If-None-Match: *`. The server returns:

- `201 Created` after it verifies and publishes new content;
- `204 No Content` when identical verified content already exists;
- `400 Bad Request` when the bytes do not match the digest;
- `412 Precondition Failed` when an immutable precondition fails;
- `413 Content Too Large` when an advertised limit is exceeded.

The mise client uploads every blob with a single `PUT` and treats a `412`
response like `204`.

Large or resumable uploads use an upload session:

1. `POST /v1/uploads` declares one or more digests.
2. The server returns an upload ID, expiry, offsets, and server or delegated
   upload URLs.
3. The client uploads chunks and resumes from server-confirmed offsets when
   necessary.
4. `POST /v1/uploads/{id}/finalize` verifies the complete content and promotes
   it into CAS.

Version 1 does not yet define the request and response bodies for upload
sessions, and the mise client does not use them.

Delegated uploads always target an isolated staging key, never a readable CAS
key. A presigned S3 upload is therefore not enough by itself: finalization must
validate the declared digest before publication. Expired or abandoned staging
objects are removed asynchronously.

## Action-result operations

`GET /v1/action-results/{algorithm}/{hash}/{size}` returns a committed action
result or `404 Not Found`. The namespace identifies the single read scope for
that request. Clients configured with several read scopes query them in policy
order rather than sending an ambiguous multi-namespace request.

`PUT /v1/action-results/{algorithm}/{hash}/{size}` commits an action result. It
requires `If-None-Match: *`. The server must atomically:

1. authorize writes to the namespace;
2. verify that the URL digest matches the result and the stored action
   descriptor;
3. validate the result and referenced metadata schemas;
4. verify that the action descriptor and client metadata kinds match;
5. verify the complete reachable directory and blob graph;
6. publish the immutable mapping.

The response is `201 Created`, `204 No Content` for an identical committed
result, `409 Conflict` when a different result already owns the action key, or
`412 Precondition Failed` when the immutable precondition is absent or fails.
Concurrent valid writers may upload identical CAS data, but only one
action-result commit wins.

Ordinary cache writers do not receive delete permission. Administrative deletion
uses a separately authorized endpoint and must remove the action-result mapping
before unreachable CAS data is garbage collected. A client-side cache clear must
not imply authority to delete shared remote data.

## Authentication and namespace policy

The protocol supports bearer tokens, OIDC-derived tokens, mTLS, and trusted
reverse-proxy identity. How a client discovers the authentication mechanism is
deployment configuration, not CAS object metadata. Credentials must be scoped
and redacted from diagnostics.

Servers authorize reads and writes independently. The standard CI policy is one
shared namespace: protected branches may write to it, while pull-request jobs
may only read it. The server enforces this from verified OIDC claims such as
repository, ref, event, and workflow identity. The mise client also refuses to
write outside protected-branch push jobs in GitHub Actions and GitLab CI, but
that is defense in depth: a client-provided mode is not the authorization
boundary.

Immutable storage does not prevent cache poisoning by the first writer.
OIDC-backed namespace authorization is therefore required even when the backing
object store rejects overwrites. A single bucket credential shared by trusted and
untrusted jobs is not a conforming security boundary.

Operators should issue short-lived, least-privilege credentials, restrict each
credential to the namespaces it needs, keep authorization headers out of logs,
and encrypt or otherwise protect stored objects according to their sensitivity
and retention requirements. Cached logs, outputs, and action descriptors can
contain sensitive values; see
[What an entry contains](/tasks/remote-cache.html#what-an-entry-contains).

## Failure and retry behavior

- `401 Unauthorized` means authentication is missing or invalid.
- `403 Forbidden` means the identity lacks permission for the namespace or
  operation.
- `404 Not Found` is a cache miss and must not reveal inaccessible objects.
- `409 Conflict` is an immutable action-result conflict.
- `412 Precondition Failed` is a missing or failed conditional-write
  requirement.
- `422 Unprocessable Content` is a validly encoded object with an invalid
  reference graph.
- `426 Upgrade Required` is a major-version mismatch.
- `429 Too Many Requests` and `5xx` responses may be retried with bounded
  exponential backoff and jitter, honoring `Retry-After`.

Version 1 does not define an error response body; clients act on the status
code.

Cache unavailability, malformed objects, missing referenced objects, and
integrity failures normally degrade to a cache miss so that local task execution
can continue. Authentication, authorization, and integrity failures must still
produce visible warnings; clients must not report them as ordinary misses.
Deployments may enable a strict mode that makes selected failures fatal.

Idempotency keys may be sent for upload-session creation and other retryable
`POST` operations. Servers must bound how long they keep them and scope them to
the authenticated identity and namespace. Version 1 does not yet name the header
for them, and the mise client does not send one.

## Self-hosted storage requirements

A conforming self-hosted server may use a filesystem, S3-compatible object
storage, or another blob store. Clients talk to the cache service and never
receive general object-store credentials.

A server that uses S3 should:

- keep action metadata, authorization, access times, references, and quotas in a
  transactional metadata store;
- store CAS bytes under digest-derived immutable keys;
- use random staging keys for delegated uploads;
- use conditional object creation and deny ordinary overwrite and delete
  permissions;
- finalize an action result only after verifying every reachable object;
- garbage-collect expired staging uploads and unreachable CAS objects;
- support short-lived workload credentials and encryption at rest.

Object-store versioning, retention locks, and encryption add defense in depth
but do not replace application authorization or digest verification.

## Conformance

To check a server, run
[`scripts/task-cache-remote-compat.sh`](https://github.com/jdx/mise/blob/main/scripts/task-cache-remote-compat.sh)
from the mise repository against a disposable namespace:

```sh
MISE_TASK_CACHE_COMPAT_TOKEN=... scripts/task-cache-remote-compat.sh https://cache.example.com scratch-ns
```

The URL and namespace can also come from `MISE_TASK_CACHE_COMPAT_URL` and
`MISE_TASK_CACHE_COMPAT_NAMESPACE`. The token is optional. The script needs
`b3sum`, `curl`, `jq`, and `sha256sum`. It checks:

- `/v1/status` and `/v1/capabilities` (protocol major 1, `sha256`, task schema
  1);
- `blobs:missing` before and after an upload;
- conditional blob uploads with both `sha256` and `blake3` digests;
- an action-result commit (`201`), an identical re-commit (`204`), and a read of
  that action result;
- a CAS download, following a `307` to an absolute HTTPS URL when the server
  delegates;
- namespace isolation (`403` or `404` from another namespace);
- `412` for an immutable write without `If-None-Match: *`.

The script does not test upload sessions, blob packs, digest or corruption
rejection, conflicting commits, independent read and write authorization,
credential isolation for delegated transfers, or retry behavior. A server meets
those requirements only by following this specification.

Servers may implement additional administrative, metrics, and health APIs
outside `/v1`. Those APIs must not weaken the version 1 cache invariants.
