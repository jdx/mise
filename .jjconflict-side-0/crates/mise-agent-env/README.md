# mise-agent-env

`mise-agent-env` detects whether a command is running under an AI coding agent.
It is environment-only: detection does not inspect processes or the filesystem and
does not spawn subprocesses.

```rust
if mise_agent_env::is_agent() {
    // Prefer complete, machine-readable output.
}

if let Some(detection) = mise_agent_env::detect() {
    eprintln!("agent: {} (via {})", detection.agent, detection.signal);
}
```

Tests and embedded callers can provide their own lookup without changing the
process environment:

```rust
use std::ffi::OsString;

let detected = mise_agent_env::detect_with_env(|name| {
    (name == "CODEX_THREAD_ID").then(|| OsString::from("thread-1"))
});
assert_eq!(detected.unwrap().agent, mise_agent_env::Agent::Codex);
```

The provider signals are informed by the language-neutral
[`vercel/detect-agent`](https://github.com/vercel/detect-agent) definitions and
the agents' own implementations. Detection is intentionally conservative: an
editor or hosted development environment alone is not classified as an agent.
