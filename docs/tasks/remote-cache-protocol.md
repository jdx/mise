# Remote Build Cache Protocol

> [!WARNING]
> Remote build caching is experimental. This document defines protocol version 1 for mise task
> artifact caching.

The protocol is a secure, content-addressed cache protocol for task executions and their outputs. It
does not expose mise's local cache directories, manifests, or archive formats. Local storage is an
implementation detail and may use archives or packs without changing the remote protocol.

Version 1 stores immutable build data:

- **Content-addressable storage (CAS)** contains blobs and directory objects identified by their
  digest.
- **Action results** map the digest of a canonical build action to its output directory and logs.
  This model deduplicates content between actions, permits partial and parallel transfers, and
  allows a server to verify all referenced content before publishing a cache hit.

## Terminology

- **Namespace**: an opaque authorization and isolation scope, normally representing an organization,
  repository, branch, pull request, or user.
- **Action**: the typed canonical description of a build operation and every input that affects its
  result.
- **Action result**: the immutable record published after an action completes successfully.
- **Blob**: uninterpreted bytes in CAS.
- **Directory object**: canonical JSON in CAS describing files, subdirectories, and symbolic links.
- **Digest**: an algorithm, lowercase hexadecimal hash, and uncompressed byte length.
- **Commit**: publication of an action result after every referenced CAS object has been verified.

## Transport and versioning

Version 1 uses HTTPS and HTTP semantics. Requests carrying authorization credentials require HTTPS,
except for loopback development servers (`localhost`, `127.0.0.0/8`, and `::1`). Clients may connect
to an unauthenticated non-loopback HTTP service after emitting a visible warning. This mode provides
neither confidentiality nor server authenticity: an on-path attacker can replace an action result
and its internally consistent CAS graph. Implementations may use HTTP/1.1, HTTP/2, or HTTP/3.

Every API request sends:

| Header                 | Value                                                          |
| ---------------------- | -------------------------------------------------------------- |
| `mise-cache-protocol`  | `1`                                                            |
| `mise-cache-namespace` | The namespace for the operation, except on discovery endpoints |

The URL prefix `/v1` is the protocol's major version. Compatible additions are advertised as
capabilities and do not require a new URL prefix. An incompatible wire or integrity change requires
a new major protocol; version 1 must not be used as an alias for an incompatible implementation.

Servers must ignore unknown JSON response fields. Clients must not send unknown request fields
unless a negotiated capability permits them.

GitHub Actions protected-branch `push` jobs and GitLab protected-branch push pipelines may use the
configured write mode. Pull requests, tags/releases, unprotected branches, unknown CI systems, and
local runs are restricted to reads; a configured write-only client disables its remote rather than
silently broadening to read access.

## Digests

The JSON representation of a digest is:

```json
{
  "algorithm": "blake3",
  "hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "size": 1234
}
```

Protocol version 1 defines `blake3` and `sha256`. Servers advertise the algorithms they accept and
must support BLAKE3. Action descriptors and action-result keys always use BLAKE3; SHA-256 may be used
for other CAS objects when the server advertises it. A digest always covers the exact, uncompressed
bytes and includes their length. A server must reject malformed hashes, unsupported algorithms,
negative sizes, and content that does not match its declared digest.

Digest URL components use `/v1/blobs/{algorithm}/{hash}/{size}`. The algorithm and hash must match
the JSON representation, and `size` is an unsigned decimal integer.

## Capabilities

`GET /v1/capabilities` requires no namespace and returns the protocol and server limits:

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

Each `action_kinds` entry advertises the action-descriptor and client-metadata schema versions the
server validates for that kind. Clients must not read or publish a non-`task` action unless the
server advertises the kind and the exact schema versions the client implements. Servers must reject
unadvertised kinds and unsupported schema versions. Compatible protocol additions may add kinds or
new schema versions without changing the major protocol version.

Clients must honor advertised limits and fall back from optional features. Servers return `426
Upgrade Required` for unsupported major versions and include their supported major version in
`mise-cache-protocol`.

`GET /v1/status` is an operational health endpoint. A successful response means the API process is
live; it is not a substitute for capability negotiation or an authorization check.

## Canonical objects

Protocol JSON objects use UTF-8 and the JSON Canonicalization Scheme (RFC 8785) whenever their bytes
are hashed. Duplicate object keys, invalid UTF-8, non-canonical encodings, and values that
cannot be represented by the declared schema must be rejected.

### Action descriptor

An action descriptor contains a stable action kind and everything declared to affect its result.
Action schema version 1 defines `task`:

```json
{
  "version": 1,
  "kind": "task",
  "task": "build",
  "phase": "normal",
  "run": [{ "task": "cargo build --release" }],
  "args": [],
  "shell": null,
  "outputs": ["target/release/widget"],
  "root": "crates/widget",
  "source_hash": "blake3:...",
  "dependency_keys": [],
  "environment": { "PROFILE": "release" },
  "command_inputs": [],
  "vars": {},
  "tools": ["core:rust@1.92.0"],
  "os": "linux",
  "arch": "x86_64"
}
```

Arrays whose order has no action meaning must be sorted by the field defined by their schema.
Version strings are opaque and are never semantically ordered. Secrets must not appear in an action
descriptor. A task includes environment variables only when it declares them as cache inputs. Every
action kind defines its own canonical fields and cacheability rules; a server may reject kinds it
does not advertise.

`source_hash` binds the declared source paths and contents without uploading task inputs that are not
needed for cache-only operation. The canonical descriptor is stored in CAS. Its digest is the action
digest and the action-result URL key. Two clients that describe the same action must produce
identical canonical bytes.

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

Each node list is sorted by the UTF-8 bytes of `name`. Names must be a single path component and
must not be empty, `.`, `..`, contain a slash or NUL, or collide with another node. Absolute symlink
targets and targets that escape the declared output root must be rejected during restoration.

The portable metadata set is file contents, directory structure, symbolic links, executable state,
and the portable permission bits represented by `mode`. Owners, groups, timestamps, devices,
sockets, FIFOs, platform ACLs, and extended attributes are not restored. Hard links may be restored
as independent files. Unsupported source objects make the task result ineligible for remote caching
rather than being silently changed.

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

Only successful, cacheable action executions may be published. `output_root` is absent when an
action has no output files. `metadata` references canonical
`application/vnd.mise.cache-client-metadata.v1+json` containing typed client metadata. Task metadata
contains output roots, captured output, task identity, restored-byte estimate, and execution
duration. The metadata schema is part of the remote protocol and is independent of mise's local
cache manifests.

```json
{
  "version": 1,
  "kind": "task",
  "task_identity": "build:crates/widget",
  "roots": ["target/release/widget"],
  "output": [{ "stream": "stdout", "line": "built widget" }],
  "restored_bytes": 123456,
  "execution_duration_ns": 900000000
}
```

Each metadata kind has a versioned schema. Task root paths use forward slashes, are relative to the
task working directory, and must satisfy the same path-safety rules as directory nodes. Task output
entries preserve their declared order.

The metadata `kind` must equal the referenced action descriptor's `kind`. Servers reject a commit
with mismatched kinds before publication, even when both objects independently satisfy their schemas.

The action descriptor and every object reachable from the result must exist and validate before the
result becomes readable.

Retention, last-access time, quota accounting, internal storage location, and server annotations are
not part of the immutable action result.

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

The server must not disclose whether objects exist outside the request's readable namespaces or CAS
visibility domain.

### Read a blob

`GET /v1/blobs/{algorithm}/{hash}/{size}` returns `200 OK`, or `404 Not Found` when the caller cannot
read the object. The response includes `Digest` and `Content-Length` metadata. Servers may honor
`Range` and may return a negotiated `Content-Encoding: zstd`; the URL digest always describes the
uncompressed bytes.

A server advertising delegated transfers may return `307 Temporary Redirect` to a short-lived HTTPS
URL. The redirect must grant access only to the requested immutable object. Clients must not forward
the cache service's `Authorization` header to the delegated host.

Clients verify the complete uncompressed digest before using downloaded content. A mismatch is a
cache miss, emits a visible integrity warning, and must be reported to server telemetry when the
reporting capability is enabled.

### Read a blob pack

Servers advertising `features.blob_packs` accept `POST /v1/blobs:pack` with the same
`application/vnd.mise.cache-digests.v1+json` body as `blobs:missing`. The aggregate declared size
must not exceed `limits.max_pack_bytes`, and the number of digests must not exceed
`limits.max_batch_items`. Servers return `400 Bad Request` when the item limit is exceeded and
`413 Content Too Large` when the aggregate declared size exceeds the byte limit.

A successful response uses `application/vnd.mise.cache-blob-pack.v1` and begins with the eight-byte
ASCII magic `MISEPK01`. The remainder is a stream of frames in request order:

| Field     | Encoding                                    |
| --------- | ------------------------------------------- |
| Algorithm | one byte: `1` for BLAKE3 or `2` for SHA-256 |
| Hash      | raw 32-byte digest                          |
| Size      | unsigned big-endian 64-bit byte length      |
| Content   | exactly `size` bytes                        |

The server omits missing and unauthorized blobs and emits duplicate requests once. Clients reject
unrequested or duplicate frames, stream each frame to bounded temporary storage, verify its full
digest, and only then admit it to local CAS. Clients fall back to ordinary single-blob reads when
the capability is absent, a digest exceeds the advertised pack limit, or an expected blob is
omitted. A pack is a transfer optimization only; its framing does not change CAS identity or action
semantics.

### Upload blobs

Small blobs may be sent directly with
`PUT /v1/blobs/{algorithm}/{hash}/{size}` and `If-None-Match: *`. The server returns:

- `201 Created` after verifying and publishing new content;
- `204 No Content` when identical verified content already exists;
- `400 Bad Request` when the bytes do not match the digest;
- `412 Precondition Failed` when an immutable precondition fails;
- `413 Content Too Large` when an advertised limit is exceeded.

Large or resumable uploads use an upload session:

1. `POST /v1/uploads` declares one or more digests.
2. The server returns an upload ID, expiry, offsets, and server or delegated upload URLs.
3. The client uploads chunks and resumes from server-confirmed offsets when necessary.
4. `POST /v1/uploads/{id}/finalize` verifies complete content and promotes it into CAS.

Delegated uploads always target an isolated staging key, never a readable CAS key. A presigned S3
upload is therefore insufficient by itself: finalization must validate the declared digest before
publication. Expired or abandoned staging objects are removed asynchronously.

## Action-result operations

`GET /v1/action-results/{algorithm}/{hash}/{size}` returns a committed action result or `404 Not
Found`. The namespace identifies the single read scope for that request. Clients configured with
multiple read scopes query them in policy order rather than sending an ambiguous multi-namespace
request.

`PUT /v1/action-results/{algorithm}/{hash}/{size}` commits an action result. It requires
`If-None-Match: *`. The server must atomically:

1. authorize writes to the namespace;
2. verify that the URL digest matches the result and stored action descriptor;
3. validate the result and referenced metadata schemas;
4. verify that the action descriptor and client metadata kinds match;
5. verify the complete reachable directory and blob graph;
6. publish the immutable mapping.

The response is `201 Created`, `204 No Content` for an identical committed result, `409 Conflict`
when a different result already owns the action key, or `412 Precondition Failed` when the immutable
precondition is absent or fails. Concurrent valid writers may upload identical CAS data, but only one
action-result commit wins.

Ordinary cache writers do not receive delete permission. Administrative deletion uses a separately
authorized endpoint and must remove the action-result mapping before unreachable CAS data is garbage
collected. A client-side cache clear operation must not imply authority to delete shared remote data.

## Authentication and namespace policy

The protocol supports bearer tokens, OIDC-derived tokens, mTLS, and trusted reverse-proxy identity.
Authentication mechanism discovery is deployment configuration rather than CAS object metadata.
Credentials must be scoped and redacted from diagnostics.

Servers authorize reads and writes independently. The standard CI policy is one shared namespace:
protected branches may write it, while pull-request jobs may only read it. The server enforces this
from verified OIDC claims such as repository, ref, event, and workflow identity; a client-provided
remote mode is defense in depth, not the authorization boundary.

Immutable storage does not prevent cache poisoning by the first writer. OIDC-backed namespace
authorization is therefore required even when the backing object store rejects overwrites. A single
bucket credential shared by trusted and untrusted jobs is not a conforming security boundary.

## Failure and retry behavior

- `401 Unauthorized` means authentication is missing or invalid.
- `403 Forbidden` means the identity lacks permission for the requested namespace or operation.
- `404 Not Found` is a cache miss and must not reveal inaccessible objects.
- `409 Conflict` is an immutable action-result conflict.
- `412 Precondition Failed` is a missing or failed conditional-write requirement.
- `422 Unprocessable Content` is a validly encoded object with an invalid reference graph.
- `426 Upgrade Required` is a major-version mismatch.
- `429 Too Many Requests` and `5xx` responses may be retried with bounded exponential backoff and
  jitter, honoring `Retry-After`.

Cache unavailability, malformed objects, missing referenced objects, and integrity failures normally
degrade to a cache miss so local task execution can continue. Authentication, authorization, and
integrity failures must still produce visible warnings; clients must not silently label them as
ordinary misses. Deployments may enable a strict mode that makes selected failures fatal.

Idempotency keys may be sent for upload-session creation and other retryable `POST` operations.
Servers must bound their retention and scope them to the authenticated identity and namespace.

## Self-hosted storage requirements

A conforming self-hosted server may use a filesystem, S3-compatible object storage, or another blob
store. Clients communicate with the cache service rather than receiving general object-store
credentials.

The official reference server is maintained separately at
[`jdx/mise-cache`](https://github.com/jdx/mise-cache). It provides filesystem and S3-compatible blob
storage, PostgreSQL metadata, namespace-scoped authorization, Docker Compose, and a Helm chart. The
server remains a separate deployment and release lifecycle from the mise client while this document
is the canonical protocol specification.

A server using S3 should:

- keep action metadata, authorization, access times, references, and quotas in a
  transactional metadata store;
- store CAS bytes under digest-derived immutable keys;
- use random staging keys for delegated uploads;
- use conditional object creation and deny ordinary overwrite and delete permissions;
- finalize an action result only after verifying every reachable object;
- garbage-collect expired staging uploads and unreachable CAS objects;
- support short-lived workload credentials and encryption at rest.

Object-store versioning, retention locks, and encryption are useful defense in depth but do not
replace application authorization or digest verification.

## Conformance

The repository's compatibility suite is the executable definition of required version 1 behavior.
It must cover capability negotiation, canonical object validation, namespace isolation, independent
read/write authorization, missing-blob batches, streamed and resumable transfers, digest rejection,
atomic action-result commits, immutable conflicts, delegated-transfer credential isolation,
corruption handling, and retry semantics.

Servers may implement additional administrative, metrics, and health APIs outside `/v1`. Those APIs
must not weaken the version 1 cache invariants.
