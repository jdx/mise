# Remote Task Cache Protocol

> [!WARNING]
> Remote task caching is experimental and is not yet configurable. This document defines protocol
> version 1 for client, server, and self-hosted implementation work.

The remote cache stores the same task-cache manifests and compressed artifact archives as the local
cache. Protocol versions, cache-store contract versions, manifest format versions, and artifact
checksum format versions are independent. A change to one does not implicitly change the others.

## Version 1

Protocol version 1 is an HTTP protocol. All cache requests send these headers:

| Header                 | Value                                                     |
| ---------------------- | --------------------------------------------------------- |
| `Mise-Cache-Protocol`  | `1`                                                       |
| `Mise-Cache-Namespace` | An opaque, non-empty repository or organization namespace |
| `Accept`               | The media type documented for the requested object        |

Servers must isolate entries by both namespace and cache key. Namespace values are header values,
not URL path fragments, and must not affect routing or filesystem paths without safe encoding.

Cache keys are lowercase, 64-character hexadecimal BLAKE3 values. A server must reject malformed
keys with `400 Bad Request`.

### Discovery

`GET /v1/status` checks protocol compatibility before cache operations. The namespace header is not
required for this endpoint.

Successful servers return `200 OK` and:

```json
{
  "protocol": 1,
  "store": 1
}
```

`protocol` is this HTTP protocol version. `store` is the cache-store contract version implemented
by the server. Unknown request protocol versions return `426 Upgrade Required`; the response should
include its supported version in `Mise-Cache-Protocol`.

### Manifest object

The manifest endpoint is `/v1/cache/{key}`. Its media type is
`application/vnd.mise.task-cache-manifest.v2+json`.

- `GET` returns the exact manifest bytes with `200 OK`, or `404 Not Found` for a cache miss.
- `PUT` atomically publishes the request body and returns `201 Created` for a new entry or
  `204 No Content` when identical bytes were already stored.
- A key that already exists with different bytes returns `409 Conflict`.

The manifest is the commit record for an entry. Servers must not make a newly uploaded artifact
visible as a cache hit until the corresponding manifest `PUT` succeeds.

### Artifact object

The artifact endpoint is `/v1/cache/{key}/artifact`. Its media type is
`application/vnd.mise.task-cache-artifact.v1+zstd`. The body is the existing zstd-compressed tar
archive without an additional protocol envelope.

- `GET` returns the artifact with `200 OK`, or `404 Not Found` when it is absent.
- `PUT` atomically publishes the body and returns `201 Created` or `204 No Content` for identical
  content.
- A key that already has different artifact bytes returns `409 Conflict`.

Clients upload the artifact before the manifest. Entries whose manifest declares no output roots do
not have an artifact object. Clients must stream artifact bodies and servers must accept either a
known `Content-Length` or HTTP chunked transfer encoding.

### Reads and corruption

A client reads the manifest first. When it declares output roots, the client then reads the artifact.
Any of these conditions make the remote lookup a cache miss and must not prevent local task
execution:

- the manifest returns `404`;
- an artifact required by the manifest returns `404`;
- either object is malformed or truncated;
- the manifest cache key, format, or artifact checksum does not validate.

Servers may delete incomplete uploads and uncommitted artifacts asynchronously. Clients may retry
idempotent `GET` and `PUT` requests. Retry timing and offline fallback are client policy rather than
protocol semantics.

### HTTP behavior

- Requests and responses may stream; implementations must not require complete artifacts in memory.
- Redirects are not part of version 1 and clients need not follow them.
- `401 Unauthorized` and `403 Forbidden` are authentication or authorization failures, not misses.
- `429 Too Many Requests` and `5xx` responses are transient server failures.
- Other `4xx` responses are permanent request failures.
- Servers must treat `(namespace, key)` as immutable and make object replacement atomic.

Production deployments should use HTTPS. Authentication credentials, namespace selection, remote
cache modes, integrity policy, and secret-handling requirements are specified by the corresponding
experimental client configuration rather than embedded in version 1 object paths.
