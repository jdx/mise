use crate::{CacheDigest, LocalActionCache, LocalCas, RemoteActionResult};
use eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

pub const AGENT_PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentRequest {
    Hello {
        protocol: u8,
        client_version: String,
    },
    FindBlob {
        digest: CacheDigest,
    },
    StoreBlob {
        digest: CacheDigest,
        source: PathBuf,
    },
    StoreActionResult {
        result: RemoteActionResult,
    },
    IdentifyExecutable {
        executable: PathBuf,
        arguments: Vec<String>,
        environment: BTreeMap<String, Option<String>>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentResponse {
    Hello { protocol: u8, agent_version: String },
    Blob { path: Option<PathBuf> },
    Stored { path: PathBuf },
    ActionStored { path: PathBuf },
    ExecutableIdentity { stdout: Vec<u8> },
    Error { message: String },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentStats {
    pub lookups: u64,
    pub hits: u64,
    pub stores: u64,
    pub stored_bytes: u64,
}

#[derive(Default)]
struct AtomicAgentStats {
    lookups: AtomicU64,
    hits: AtomicU64,
    stores: AtomicU64,
    stored_bytes: AtomicU64,
}

/// Shared state for an agent hosted by the top-level `mise run` process.
///
/// Transport listeners deliberately live in mise so the task-run lifecycle owns
/// them. This type only contains ecosystem-independent CAS and protocol logic.
#[derive(Clone)]
pub struct CacheAgent {
    cas: LocalCas,
    actions: LocalActionCache,
    version: Arc<str>,
    write_locks: Arc<Mutex<BTreeMap<CacheDigest, Weak<tokio::sync::Mutex<()>>>>>,
    stats: Arc<AtomicAgentStats>,
    executable_identities: Arc<Mutex<BTreeMap<ExecutableIdentityKey, Vec<u8>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExecutableIdentityKey {
    executable: PathBuf,
    arguments: Vec<String>,
    environment: BTreeMap<String, Option<String>>,
}

impl CacheAgent {
    pub fn new(cache_dir: impl Into<PathBuf>, version: impl Into<Arc<str>>) -> Self {
        let cache_dir = cache_dir.into();
        Self {
            cas: LocalCas::new(cache_dir.clone()),
            actions: LocalActionCache::new(cache_dir),
            version: version.into(),
            write_locks: Arc::new(Mutex::new(BTreeMap::new())),
            stats: Arc::new(AtomicAgentStats::default()),
            executable_identities: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn stats(&self) -> AgentStats {
        AgentStats {
            lookups: self.stats.lookups.load(Ordering::Relaxed),
            hits: self.stats.hits.load(Ordering::Relaxed),
            stores: self.stats.stores.load(Ordering::Relaxed),
            stored_bytes: self.stats.stored_bytes.load(Ordering::Relaxed),
        }
    }

    fn write_lock(&self, digest: &CacheDigest) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.write_locks.lock().unwrap();
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(digest).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(digest.clone(), Arc::downgrade(&lock));
        lock
    }

    async fn respond(&self, request: AgentRequest) -> AgentResponse {
        let result = match request {
            AgentRequest::FindBlob { digest } => {
                self.stats.lookups.fetch_add(1, Ordering::Relaxed);
                self.cas.find(&digest).map(|path| {
                    if path.is_some() {
                        self.stats.hits.fetch_add(1, Ordering::Relaxed);
                    }
                    AgentResponse::Blob { path }
                })
            }
            AgentRequest::StoreBlob { digest, source } => {
                let lock = self.write_lock(&digest);
                let _guard = lock.lock().await;
                if let Ok(Some(path)) = self.cas.find(&digest) {
                    return AgentResponse::Stored { path };
                }
                self.cas.store_file(&digest, &source).map(|path| {
                    self.stats.stores.fetch_add(1, Ordering::Relaxed);
                    self.stats
                        .stored_bytes
                        .fetch_add(digest.size, Ordering::Relaxed);
                    AgentResponse::Stored { path }
                })
            }
            AgentRequest::StoreActionResult { result } => self
                .actions
                .store(&result)
                .map(|path| AgentResponse::ActionStored { path }),
            AgentRequest::IdentifyExecutable {
                executable,
                arguments,
                environment,
            } => self
                .identify_executable(executable, arguments, environment)
                .map(|stdout| AgentResponse::ExecutableIdentity { stdout }),
            AgentRequest::Hello { .. } => {
                Err(eyre::eyre!("hello is only valid as the first request"))
            }
        };
        result.unwrap_or_else(|error| AgentResponse::Error {
            message: error.to_string(),
        })
    }

    fn identify_executable(
        &self,
        executable: PathBuf,
        arguments: Vec<String>,
        environment: BTreeMap<String, Option<String>>,
    ) -> Result<Vec<u8>> {
        let key = ExecutableIdentityKey {
            executable: executable.clone(),
            arguments: arguments.clone(),
            environment: environment.clone(),
        };
        if let Some(stdout) = self
            .executable_identities
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
        {
            return Ok(stdout);
        }
        let mut command = std::process::Command::new(&executable);
        command.args(&arguments);
        for (name, value) in environment {
            if let Some(value) = value {
                command.env(name, value);
            } else {
                command.env_remove(name);
            }
        }
        let output = command.output()?;
        if !output.status.success() {
            bail!(
                "executable identity command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        self.executable_identities
            .lock()
            .unwrap()
            .insert(key, output.stdout.clone());
        Ok(output.stdout)
    }

    pub async fn handle_connection<S>(&self, stream: S) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();
        let hello = lines
            .next_line()
            .await?
            .ok_or_else(|| eyre::eyre!("connection closed before the agent handshake"))?;
        let request: AgentRequest = serde_json::from_str(&hello)?;
        match request {
            AgentRequest::Hello {
                protocol,
                client_version,
            } if protocol == AGENT_PROTOCOL_VERSION && client_version == self.version.as_ref() => {}
            AgentRequest::Hello { protocol, .. } if protocol != AGENT_PROTOCOL_VERSION => {
                send_response(
                    &mut writer,
                    &AgentResponse::Error {
                        message: format!(
                            "unsupported agent protocol {protocol}; expected {AGENT_PROTOCOL_VERSION}"
                        ),
                    },
                )
                .await?;
                return Ok(());
            }
            AgentRequest::Hello { client_version, .. } => {
                send_response(
                    &mut writer,
                    &AgentResponse::Error {
                        message: format!(
                            "cache client {client_version} does not match agent {}",
                            self.version
                        ),
                    },
                )
                .await?;
                return Ok(());
            }
            _ => bail!("the first agent request must be hello"),
        }
        send_response(
            &mut writer,
            &AgentResponse::Hello {
                protocol: AGENT_PROTOCOL_VERSION,
                agent_version: self.version.to_string(),
            },
        )
        .await?;

        while let Some(line) = lines.next_line().await? {
            let response = match serde_json::from_str(&line) {
                Ok(request) => self.respond(request).await,
                Err(error) => AgentResponse::Error {
                    message: format!("invalid agent request: {error}"),
                },
            };
            send_response(&mut writer, &response).await?;
        }
        Ok(())
    }
}

async fn send_response(
    writer: &mut (impl AsyncWrite + Unpin),
    response: &AgentResponse,
) -> Result<()> {
    let mut encoded = serde_json::to_vec(response)?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn handshake(stream: &mut (impl AsyncRead + AsyncWrite + Unpin), version: &str) {
        let request = AgentRequest::Hello {
            protocol: AGENT_PROTOCOL_VERSION,
            client_version: version.to_string(),
        };
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        stream.write_all(&encoded).await.unwrap();
        stream.flush().await.unwrap();
        let mut response = String::new();
        BufReader::new(stream)
            .read_line(&mut response)
            .await
            .unwrap();
        assert!(matches!(
            serde_json::from_str(&response).unwrap(),
            AgentResponse::Hello { .. }
        ));
    }

    #[tokio::test]
    async fn handshake_and_blob_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        std::fs::write(&source, b"cached object").unwrap();
        let digest = CacheDigest::blake3(b"cached object");
        let agent = CacheAgent::new(directory.path().join("cache"), "test-version");
        let (mut client, server) = tokio::io::duplex(16 * 1024);
        let server_agent = agent.clone();
        let task = tokio::spawn(async move { server_agent.handle_connection(server).await });

        handshake(&mut client, "test-version").await;
        let request = AgentRequest::StoreBlob {
            digest: digest.clone(),
            source,
        };
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        client.write_all(&encoded).await.unwrap();
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .await
            .unwrap();
        assert!(matches!(
            serde_json::from_str(&response).unwrap(),
            AgentResponse::Stored { .. }
        ));
        drop(client);
        task.await.unwrap().unwrap();
        assert_eq!(
            agent.stats(),
            AgentStats {
                stores: 1,
                stored_bytes: digest.size,
                ..AgentStats::default()
            }
        );
    }

    #[tokio::test]
    async fn publishes_a_complete_action_result() {
        let directory = tempfile::tempdir().unwrap();
        let agent = CacheAgent::new(directory.path().join("cache"), "test-version");
        let action = CacheDigest::blake3(b"action");
        let metadata = CacheDigest::blake3(b"metadata");
        let output_root = CacheDigest::blake3(b"directory");
        for (digest, contents) in [
            (&action, b"action".as_slice()),
            (&metadata, b"metadata".as_slice()),
            (&output_root, b"directory".as_slice()),
        ] {
            agent.cas.store_bytes(digest, contents).unwrap();
        }
        let response = agent
            .respond(AgentRequest::StoreActionResult {
                result: RemoteActionResult {
                    action: action.clone(),
                    metadata: Some(metadata),
                    output_root: Some(output_root),
                    version: 1,
                },
            })
            .await;
        assert!(matches!(response, AgentResponse::ActionStored { .. }));
        assert!(agent.actions.find(&action).unwrap().is_some());
    }

    #[tokio::test]
    async fn version_skew_is_a_handshake_miss() {
        let directory = tempfile::tempdir().unwrap();
        let agent = CacheAgent::new(directory.path(), "agent-version");
        let (mut client, server) = tokio::io::duplex(1024);
        let task = tokio::spawn(async move { agent.handle_connection(server).await });
        let request = AgentRequest::Hello {
            protocol: AGENT_PROTOCOL_VERSION,
            client_version: "other-version".into(),
        };
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        client.write_all(&encoded).await.unwrap();
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .await
            .unwrap();

        assert!(matches!(
            serde_json::from_str(&response).unwrap(),
            AgentResponse::Error { .. }
        ));
        task.await.unwrap().unwrap();
    }
}
