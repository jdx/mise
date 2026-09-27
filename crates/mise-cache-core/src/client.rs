use super::*;

pub struct RemoteCacheClient {
    base_url: Url,
    namespace: String,
    client: reqwest::Client,
    credential: RemoteCacheCredential,
    download_timeout: Duration,
    retries: i64,
    capabilities: tokio::sync::OnceCell<Option<BlobPackLimits>>,
    blob_packs_disabled: AtomicBool,
}

impl RemoteCacheClient {
    pub fn new(config: RemoteCacheConfig) -> Result<Self> {
        let authenticated = config
            .token
            .as_deref()
            .is_some_and(|token| !token.trim().is_empty())
            || config.token_file.is_some()
            || config
                .oidc_audience
                .as_deref()
                .is_some_and(|audience| !audience.trim().is_empty());
        validate_remote_url(&config.base_url, authenticated)?;
        let client = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .read_timeout(config.read_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let credential = remote_credential(&config, client.clone())?;
        Ok(Self {
            base_url: normalized_base_url(config.base_url),
            namespace: config.namespace,
            client,
            credential,
            download_timeout: config.download_timeout,
            retries: config.retries,
            capabilities: tokio::sync::OnceCell::new(),
            blob_packs_disabled: AtomicBool::new(false),
        })
    }

    pub(crate) fn action_result_endpoint(&self, action: &CacheDigest) -> Result<Url> {
        action.validate()?;
        if action.algorithm != "blake3" {
            bail!("remote cache action keys must use blake3");
        }
        Ok(self.base_url.join(&format!(
            "v{PROTOCOL_VERSION}/action-results/{}/{}/{}",
            action.algorithm, action.hash, action.size
        ))?)
    }

    fn blob_endpoint(&self, digest: &CacheDigest) -> Result<Url> {
        digest.validate()?;
        Ok(self.base_url.join(&format!(
            "v{PROTOCOL_VERSION}/blobs/{}/{}/{}",
            digest.algorithm, digest.hash, digest.size
        ))?)
    }

    fn capabilities_endpoint(&self) -> Result<Url> {
        Ok(self
            .base_url
            .join(&format!("v{PROTOCOL_VERSION}/capabilities"))?)
    }

    fn blob_pack_endpoint(&self) -> Result<Url> {
        Ok(self
            .base_url
            .join(&format!("v{PROTOCOL_VERSION}/blobs:pack"))?)
    }

    async fn request(
        &self,
        method: reqwest::Method,
        url: Url,
        media_type: &'static str,
    ) -> Result<reqwest::RequestBuilder> {
        let request = self
            .client
            .request(method, url)
            .header(PROTOCOL_HEADER, u16::from(PROTOCOL_VERSION))
            .header(NAMESPACE_HEADER, &self.namespace)
            .header(ACCEPT, media_type);
        if let Some(authorization) = self.credential.authorization().await? {
            Ok(request.header(AUTHORIZATION, authorization))
        } else {
            Ok(request)
        }
    }

    async fn blob_pack_limits(&self) -> Result<Option<BlobPackLimits>> {
        self.capabilities
            .get_or_try_init(|| async {
                let url = self.capabilities_endpoint()?;
                let response = self
                    .request(reqwest::Method::GET, url, "application/json")
                    .await?
                    .send()
                    .await?;
                if matches!(
                    response.status(),
                    StatusCode::NOT_FOUND
                        | StatusCode::METHOD_NOT_ALLOWED
                        | StatusCode::NOT_IMPLEMENTED
                ) {
                    return Ok(None);
                }
                let capabilities: RemoteCacheCapabilities =
                    response.error_for_status()?.json().await?;
                if capabilities.protocol.major != PROTOCOL_VERSION {
                    bail!(
                        "remote cache capability protocol {} is incompatible with client protocol {PROTOCOL_VERSION}",
                        capabilities.protocol.major
                    );
                }
                if !capabilities.features.blob_packs {
                    return Ok(None);
                }
                let max_items = usize::try_from(capabilities.limits.max_batch_items)
                    .ok()
                    .filter(|limit| *limit > 0)
                    .ok_or_else(|| {
                        eyre!("remote cache blob packs require a positive max_batch_items limit")
                    })?;
                if capabilities.limits.max_pack_bytes == 0 {
                    bail!("remote cache blob packs require a positive max_pack_bytes limit");
                }
                Ok(Some(BlobPackLimits {
                    max_items: max_items.min(MAX_STAGED_BLOB_PACK_ITEMS),
                    max_bytes: capabilities
                        .limits
                        .max_pack_bytes
                        .min(MAX_STAGED_BLOB_PACK_BYTES),
                }))
            })
            .await
            .copied()
    }

    /// Download verified CAS objects using the server's negotiated blob-pack extension.
    ///
    /// `None` means the server does not support blob packs. Objects omitted by a
    /// supported server are absent from `blobs`, so callers can retry them through
    /// the ordinary single-blob endpoint.
    pub async fn get_blob_pack(
        &self,
        digests: &[CacheDigest],
        staging_dir: &Path,
    ) -> Result<Option<RemoteBlobPack>> {
        if digests.is_empty() || self.blob_packs_disabled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let Some(limits) = self.blob_pack_limits().await? else {
            return Ok(None);
        };
        fs::create_dir_all(staging_dir)?;
        let chunk = blob_pack_chunk(digests, limits)?;
        if chunk.is_empty() {
            return Ok(Some(RemoteBlobPack {
                _directory: tempfile::tempdir_in(staging_dir)?,
                blobs: Vec::new(),
                requests: 0,
                requested: Vec::new(),
                blob_count: 0,
                payload_bytes: 0,
                framed_bytes: BLOB_PACK_MAGIC.len() as u64,
            }));
        }
        match self.download_blob_pack_chunk(&chunk, staging_dir).await? {
            Some(pack) => Ok(Some(RemoteBlobPack {
                _directory: pack.directory,
                blobs: pack.blobs,
                requests: 1,
                requested: chunk,
                blob_count: pack.metadata.blob_count,
                payload_bytes: pack.metadata.payload_bytes,
                framed_bytes: pack.metadata.framed_bytes,
            })),
            None => {
                self.blob_packs_disabled.store(true, Ordering::Relaxed);
                Ok(None)
            }
        }
    }

    async fn download_blob_pack_chunk(
        &self,
        digests: &[CacheDigest],
        staging_dir: &Path,
    ) -> Result<Option<DownloadedBlobPack>> {
        let url = self.blob_pack_endpoint()?;
        let body = serde_json::to_vec(&DigestList { digests })?;
        let download_timeout = blob_pack_download_timeout(self.download_timeout, digests);
        let download = retry_async("POST", &url, self.retries, || async {
            let response = self
                .request(reqwest::Method::POST, url.clone(), BLOB_PACK_MEDIA_TYPE)
                .await?
                .header(CONTENT_TYPE, DIGEST_LIST_MEDIA_TYPE)
                .body(body.clone())
                .send()
                .await?;
            if matches!(
                response.status(),
                StatusCode::NOT_FOUND
                    | StatusCode::METHOD_NOT_ALLOWED
                    | StatusCode::NOT_IMPLEMENTED
            ) {
                return Ok(None);
            }
            let response = response.error_for_status()?;
            let media_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .map(str::trim);
            if media_type != Some(BLOB_PACK_MEDIA_TYPE) {
                bail!("remote cache blob pack has an invalid content type");
            }
            Ok(Some(
                decode_blob_pack(response, digests, staging_dir).await?,
            ))
        });
        tokio::time::timeout(download_timeout, download)
            .await
            .map_err(|_| eyre!("remote cache blob pack download timed out for {url}"))?
    }

    pub async fn get_action_result(
        &self,
        action: &CacheDigest,
    ) -> Result<Option<RemoteActionResult>> {
        let url = self.action_result_endpoint(action)?;
        let result = retry_async("GET", &url, self.retries, || async {
            let response = self
                .request(reqwest::Method::GET, url.clone(), ACTION_RESULT_MEDIA_TYPE)
                .await?
                .send()
                .await?;
            if response.status() == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            Ok(Some(
                response
                    .error_for_status()?
                    .json::<RemoteActionResult>()
                    .await?,
            ))
        })
        .await?;
        if let Some(result) = &result
            && (result.version != 1 || result.action != *action)
        {
            bail!("remote action result does not match requested action");
        }
        Ok(result)
    }

    pub async fn put_action_result(&self, result: &RemoteActionResult) -> Result<()> {
        let url = self.action_result_endpoint(&result.action)?;
        let body = serde_json::to_vec(result)?;
        retry_async("PUT", &url, self.retries, || async {
            let response = self
                .request(reqwest::Method::PUT, url.clone(), ACTION_RESULT_MEDIA_TYPE)
                .await?
                .header(CONTENT_TYPE, ACTION_RESULT_MEDIA_TYPE)
                .header(IF_NONE_MATCH, "*")
                .body(body.clone())
                .send()
                .await?;
            if response.status() != StatusCode::PRECONDITION_FAILED {
                response.error_for_status()?;
            }
            Ok(())
        })
        .await
    }

    pub async fn get_blob(
        &self,
        digest: &CacheDigest,
        media_type: &'static str,
    ) -> Result<Vec<u8>> {
        digest.validate()?;
        let url = self.blob_endpoint(digest)?;
        retry_async("GET", &url, self.retries, || async {
            let response = self
                .request(reqwest::Method::GET, url.clone(), media_type)
                .await?
                .send()
                .await?
                .error_for_status()?;
            let bytes = response.bytes().await?.to_vec();
            if !digest.matches_bytes(&bytes)? {
                bail!("remote cache blob failed digest verification");
            }
            Ok(bytes)
        })
        .await
    }

    pub async fn get_blob_file(
        &self,
        digest: &CacheDigest,
        staging_dir: &Path,
    ) -> Result<tempfile::NamedTempFile> {
        let url = self.blob_endpoint(digest)?;
        let download = retry_async("GET", &url, self.retries, || async {
            let mut response = self
                .request(reqwest::Method::GET, url.clone(), BLOB_MEDIA_TYPE)
                .await?
                .send()
                .await?;
            response.error_for_status_ref()?;
            fs::create_dir_all(staging_dir)?;
            let temporary = tempfile::NamedTempFile::new_in(staging_dir)?;
            let mut output = tokio::fs::File::from_std(temporary.reopen()?);
            while let Some(chunk) = response.chunk().await? {
                output.write_all(&chunk).await?;
            }
            output.flush().await?;
            drop(output);
            if !digest.matches_file(temporary.path())? {
                bail!("remote cache blob failed digest verification");
            }
            Ok(temporary)
        });
        tokio::time::timeout(self.download_timeout, download)
            .await
            .map_err(|_| eyre!("remote cache blob download timed out for {url}"))?
    }

    pub async fn put_blob(&self, upload: &BlobUpload) -> Result<()> {
        let url = self.blob_endpoint(&upload.digest)?;
        retry_async("PUT", &url, self.retries, || async {
            let (length, body) = match &upload.source {
                BlobSource::Bytes(bytes) => {
                    (bytes.len() as u64, reqwest::Body::from(bytes.clone()))
                }
                BlobSource::File(file) => {
                    let file = tokio::fs::File::open(file.path()).await?;
                    let length = file.metadata().await?.len();
                    let stream = tokio_util::io::ReaderStream::new(file);
                    (length, reqwest::Body::wrap_stream(stream))
                }
                BlobSource::Path(path) => {
                    let file = tokio::fs::File::open(path).await?;
                    let length = file.metadata().await?.len();
                    let stream = tokio_util::io::ReaderStream::new(file);
                    (length, reqwest::Body::wrap_stream(stream))
                }
            };
            let response = self
                .request(reqwest::Method::PUT, url.clone(), BLOB_MEDIA_TYPE)
                .await?
                .header(CONTENT_TYPE, BLOB_MEDIA_TYPE)
                .header(CONTENT_LENGTH, length)
                .header(IF_NONE_MATCH, "*")
                .body(body)
                .send()
                .await?;
            if response.status() != StatusCode::PRECONDITION_FAILED {
                response.error_for_status()?;
            }
            Ok(())
        })
        .await
    }
}
