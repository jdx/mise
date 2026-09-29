use super::*;

pub mod sources {
    pub use crate::{ArtifactRef, AttestationSource};

    pub mod github {
        pub use crate::GitHubSource;
    }
}

#[derive(Debug, Clone)]
pub struct GitHubSource {
    client: AttestationClient,
    owner: String,
    repo: String,
}

impl GitHubSource {
    pub fn new(owner: &str, repo: &str, token: Option<&str>) -> Result<Self> {
        let mut builder = AttestationClient::builder();
        if let Some(token) = token {
            builder = builder.github_token(token);
        }
        Ok(Self {
            client: builder.build()?,
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }

    pub fn with_base_url(
        owner: &str,
        repo: &str,
        token: Option<&str>,
        base_url: &str,
    ) -> Result<Self> {
        let mut builder = AttestationClient::builder().base_url(base_url);
        if let Some(token) = token {
            builder = builder.github_token(token);
        }
        Ok(Self {
            client: builder.build()?,
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }
}

#[async_trait]
impl AttestationSource for GitHubSource {
    async fn fetch_attestations(&self, artifact: &ArtifactRef) -> Result<Vec<Attestation>> {
        self.client
            .fetch_attestations(FetchParams {
                owner: self.owner.clone(),
                repo: Some(format!("{}/{}", self.owner, self.repo)),
                digest: artifact.digest.clone(),
                limit: 30,
                predicate_type: None,
            })
            .await
    }
}

#[derive(Debug, Clone)]
pub struct AttestationClient {
    client: reqwest::Client,
    base_url: String,
    github_token: Option<String>,
    max_attempts: usize,
    backoff_base: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct AttestationClientBuilder {
    base_url: Option<String>,
    github_token: Option<String>,
    timeout: Option<Duration>,
    retries: Option<usize>,
    backoff_base: Option<Duration>,
}

impl AttestationClientBuilder {
    pub fn base_url(mut self, url: &str) -> Self {
        self.base_url = Some(url.trim_end_matches('/').to_string());
        self
    }

    pub fn github_token(mut self, token: &str) -> Self {
        self.github_token = Some(token.to_string());
        self
    }

    /// Per-request timeout (defaults to [`DEFAULT_TIMEOUT`]).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Number of retries on transient failures (defaults to [`DEFAULT_RETRIES`];
    /// total attempts = `retries + 1`). Set to 0 to disable retries.
    pub fn retries(mut self, retries: usize) -> Self {
        self.retries = Some(retries);
        self
    }

    /// Override the attempt-1 retry backoff (defaults to [`DEFAULT_BACKOFF_BASE`]).
    /// Mainly an injection point for tests, which set it to zero so the suite
    /// doesn't pay real wall-clock backoff between retries.
    pub fn backoff_base(mut self, base: Duration) -> Self {
        self.backoff_base = Some(base);
        self
    }

    /// Apply a full [`RetryConfig`] (timeout + retries + backoff) at once. Used
    /// by the embedding crate to pass through mise's `http_*` settings.
    pub fn retry_config(self, config: RetryConfig) -> Self {
        self.timeout(config.timeout)
            .retries(config.retries)
            .backoff_base(config.backoff_base)
    }

    pub fn build(self) -> Result<AttestationClient> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(self.timeout.unwrap_or(DEFAULT_TIMEOUT))
            .build()?;

        Ok(AttestationClient {
            client,
            base_url: self.base_url.unwrap_or_else(|| GITHUB_API_URL.to_string()),
            github_token: self.github_token,
            max_attempts: self.retries.unwrap_or(DEFAULT_RETRIES) + 1,
            backoff_base: self.backoff_base.unwrap_or(DEFAULT_BACKOFF_BASE),
        })
    }
}

/// A fully-read HTTP response. The body is buffered inside the retry loop so a
/// transient failure mid-body-read is retried like a failed send, rather than
/// surfacing after the retry boundary.
pub(crate) struct HttpResponse {
    status: reqwest::StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct FetchParams {
    pub owner: String,
    pub repo: Option<String>,
    pub digest: String,
    pub limit: usize,
    pub predicate_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AttestationsResponse {
    attestations: Vec<Attestation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Attestation {
    pub(crate) bundle: Option<serde_json::Value>,
    bundle_url: Option<String>,
}

impl Attestation {
    pub fn has_inline_bundle(&self) -> bool {
        self.bundle.is_some()
    }
}

impl AttestationClient {
    pub fn builder() -> AttestationClientBuilder {
        AttestationClientBuilder::default()
    }

    fn github_headers(&self, url: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        let base_with_slash = format!("{}/", self.base_url);
        if url == self.base_url || url.starts_with(&base_with_slash) {
            if let Some(token) = &self.github_token {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {token}"))
                        .map_err(|e| AttestationError::Api(e.to_string()))?,
                );
            }
            headers.insert(
                "x-github-api-version",
                HeaderValue::from_static("2022-11-28"),
            );
        }
        Ok(headers)
    }

    pub(crate) fn attestations_url(&self, params: &FetchParams) -> Result<reqwest::Url> {
        let url = if let Some(repo) = &params.repo {
            format!(
                "{}/repos/{repo}/attestations/{}",
                self.base_url, params.digest
            )
        } else {
            format!(
                "{}/orgs/{}/attestations/{}",
                self.base_url, params.owner, params.digest
            )
        };

        let mut query_params = vec![("per_page", params.limit.to_string())];
        if let Some(predicate_type) = &params.predicate_type {
            query_params.push(("predicate_type", predicate_type.clone()));
        }
        reqwest::Url::parse_with_params(&url, query_params)
            .map_err(|e| AttestationError::Api(format!("Invalid GitHub attestations URL: {e}")))
    }

    /// Send a request and read its body, retrying transient failures (5xx, 429,
    /// timeouts, connection errors, and mid-body-read errors) with exponential
    /// backoff. A `429`'s `Retry-After` header is honored in preference to the
    /// computed backoff. The body is buffered here so a transient failure during
    /// the body read is retried too, rather than escaping the retry boundary.
    ///
    /// The request must have no streaming body so it can be cloned per attempt —
    /// true for all GET calls here. Non-transient responses (incl. 4xx like 404)
    /// are returned as-is for the caller to interpret.
    async fn send_with_retry(&self, request: reqwest::RequestBuilder) -> Result<HttpResponse> {
        let mut attempt = 1;
        loop {
            let req = request
                .try_clone()
                .expect("attestation requests must not have a streaming body");
            let last = attempt >= self.max_attempts;

            // A labeled block so the `reqwest::Response` is dropped before the
            // backoff sleep — holding it would pin its body/connection for the
            // whole delay. Each attempt either returns, errors out, or breaks
            // with the delay to wait before the next attempt.
            let delay = 'attempt: {
                match req.send().await {
                    Ok(response) => {
                        let status = response.status();
                        if !last && is_retryable_status(status) {
                            break 'attempt retry_after_delay(response.headers())
                                .unwrap_or_else(|| backoff_delay(self.backoff_base, attempt));
                        }
                        let headers = response.headers().clone();
                        match response.bytes().await {
                            Ok(body) => {
                                return Ok(HttpResponse {
                                    status,
                                    headers,
                                    body: body.to_vec(),
                                });
                            }
                            Err(err) if !last && is_retryable_error(&err) => {
                                break 'attempt backoff_delay(self.backoff_base, attempt);
                            }
                            Err(err) => return Err(AttestationError::Http(err)),
                        }
                    }
                    Err(err) if !last && is_retryable_error(&err) => {
                        break 'attempt backoff_delay(self.backoff_base, attempt);
                    }
                    Err(err) => return Err(AttestationError::Http(err)),
                }
            };

            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }

    pub async fn fetch_attestations(&self, params: FetchParams) -> Result<Vec<Attestation>> {
        let url = self.attestations_url(&params)?;

        let request = self
            .client
            .get(url.clone())
            .headers(self.github_headers(url.as_str())?);
        let response = self.send_with_retry(request).await?;

        if response.status == reqwest::StatusCode::NOT_FOUND {
            return Ok(vec![]);
        }
        if !response.status.is_success() {
            let body = String::from_utf8_lossy(&response.body);
            return Err(AttestationError::Api(format!(
                "GitHub API returned {}: {body}",
                response.status
            )));
        }

        let parsed: AttestationsResponse = serde_json::from_slice(&response.body)?;
        let mut attestations = Vec::new();
        for attestation in parsed.attestations {
            if attestation.bundle.is_some() {
                attestations.push(attestation);
            } else if let Some(bundle_url) = &attestation.bundle_url {
                let bundle = self.fetch_bundle_url(bundle_url).await?;
                attestations.push(Attestation {
                    bundle: Some(bundle),
                    bundle_url: Some(bundle_url.clone()),
                });
            }
        }
        Ok(attestations)
    }

    async fn fetch_bundle_url(&self, bundle_url: &str) -> Result<serde_json::Value> {
        let request = self
            .client
            .get(bundle_url)
            .headers(self.github_headers(bundle_url)?);
        let response = self.send_with_retry(request).await?;
        if !response.status.is_success() {
            return Err(AttestationError::Api(format!(
                "bundle URL returned {}",
                response.status
            )));
        }
        if is_snappy_content_type(&response.headers) {
            let decompressed = snap::raw::Decoder::new()
                .decompress_vec(&response.body)
                .map_err(|e| AttestationError::Api(format!("Snappy decompression failed: {e}")))?;
            serde_json::from_slice(&decompressed).map_err(AttestationError::Json)
        } else {
            serde_json::from_slice(&response.body).map_err(AttestationError::Json)
        }
    }
}
