use super::*;

#[derive(Clone)]
pub(crate) enum RemoteCacheCredential {
    None,
    Static(HeaderValue),
    File(PathBuf),
    GithubActions(Arc<GithubActionsOidcCredential>),
}

pub(crate) struct GithubActionsOidcCredential {
    audience: String,
    pub(crate) request_url: Url,
    request_token: HeaderValue,
    client: reqwest::Client,
    retries: i64,
    cached: tokio::sync::Mutex<Option<CachedOidcToken>>,
}

pub(crate) struct CachedOidcToken {
    authorization: HeaderValue,
    expires_at: u64,
}

#[derive(Deserialize)]
pub(crate) struct GithubActionsOidcResponse {
    value: String,
}

#[derive(Deserialize)]
pub(crate) struct JwtExpiry {
    exp: u64,
}

pub(crate) fn remote_credential(
    config: &RemoteCacheConfig,
    client: reqwest::Client,
) -> Result<RemoteCacheCredential> {
    if let Some(authorization) = authorization_header(config.token.as_deref())? {
        return Ok(RemoteCacheCredential::Static(authorization));
    }
    if let Some(path) = &config.token_file {
        return Ok(RemoteCacheCredential::File(path.clone()));
    }
    let Some(audience) = config
        .oidc_audience
        .as_deref()
        .map(str::trim)
        .filter(|audience| !audience.is_empty())
    else {
        return Ok(RemoteCacheCredential::None);
    };
    Ok(RemoteCacheCredential::GithubActions(Arc::new(
        GithubActionsOidcCredential::from_env(audience, client, config.retries)?,
    )))
}

pub(crate) fn authorization_header(token: Option<&str>) -> Result<Option<HeaderValue>> {
    let Some(token) = token.map(str::trim).filter(|token| !token.is_empty()) else {
        return Ok(None);
    };
    let mut value = HeaderValue::from_str(&format!("Bearer {token}"))?;
    value.set_sensitive(true);
    Ok(Some(value))
}

impl RemoteCacheCredential {
    pub(crate) async fn authorization(&self) -> Result<Option<HeaderValue>> {
        match self {
            Self::None => Ok(None),
            Self::Static(value) => Ok(Some(value.clone())),
            Self::File(path) => {
                let token = tokio::fs::read_to_string(path).await.map_err(|err| {
                    eyre!(
                        "failed to read remote cache token file {}: {err}",
                        path.display()
                    )
                })?;
                authorization_header(Some(&token))?
                    .ok_or_else(|| eyre!("remote cache token file {} is empty", path.display()))
                    .map(Some)
            }
            Self::GithubActions(credential) => credential.authorization().await.map(Some),
        }
    }
}

impl GithubActionsOidcCredential {
    pub(crate) fn from_env(audience: &str, client: reqwest::Client, retries: i64) -> Result<Self> {
        let request_url = std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL").map_err(|_| {
            eyre!(
                "remote cache OIDC audience requires GitHub Actions OIDC; \
                 grant `id-token: write` or set MISE_TASK_CACHE_REMOTE_TOKEN"
            )
        })?;
        let request_token = std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN").map_err(|_| {
            eyre!(
                "remote cache OIDC audience requires GitHub Actions OIDC; \
                 ACTIONS_ID_TOKEN_REQUEST_TOKEN is missing"
            )
        })?;
        let request_url: Url = request_url
            .parse()
            .map_err(|err| eyre!("invalid GitHub Actions OIDC request URL: {err}"))?;
        Self::new(audience, request_url, &request_token, client, retries)
    }

    pub(crate) fn new(
        audience: &str,
        mut request_url: Url,
        request_token: &str,
        client: reqwest::Client,
        retries: i64,
    ) -> Result<Self> {
        validate_oidc_request_url(&request_url)?;
        let query = request_url
            .query_pairs()
            .filter(|(key, _)| key != "audience")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        request_url.set_query(None);
        request_url
            .query_pairs_mut()
            .extend_pairs(query)
            .append_pair("audience", audience);
        let request_token = authorization_header(Some(request_token))?
            .ok_or_else(|| eyre!("GitHub Actions OIDC request token is empty"))?;
        Ok(Self {
            audience: audience.to_string(),
            request_url,
            request_token,
            client,
            retries,
            cached: tokio::sync::Mutex::new(None),
        })
    }

    pub(crate) async fn authorization(&self) -> Result<HeaderValue> {
        const REFRESH_LEEWAY_SECONDS: u64 = 60;
        let mut cached = self.cached.lock().await;
        let now = unix_timestamp()?;
        if let Some(token) = cached.as_ref()
            && token.expires_at > now.saturating_add(REFRESH_LEEWAY_SECONDS)
        {
            return Ok(token.authorization.clone());
        }
        let response: GithubActionsOidcResponse =
            retry_async("GET", &self.request_url, self.retries, || async {
                Ok(self
                    .client
                    .get(self.request_url.clone())
                    .header(AUTHORIZATION, self.request_token.clone())
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?)
            })
            .await
            .map_err(|err| {
                eyre!(
                    "failed to acquire GitHub Actions OIDC token for audience {:?}: {err}",
                    self.audience
                )
            })?;
        let expires_at = jwt_expiry(&response.value)?;
        if expires_at <= now.saturating_add(REFRESH_LEEWAY_SECONDS) {
            bail!("GitHub Actions OIDC token expires too soon");
        }
        let authorization = authorization_header(Some(&response.value))?
            .ok_or_else(|| eyre!("GitHub Actions returned an empty OIDC token"))?;
        *cached = Some(CachedOidcToken {
            authorization: authorization.clone(),
            expires_at,
        });
        Ok(authorization)
    }
}

pub(crate) fn jwt_expiry(token: &str) -> Result<u64> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| eyre!("GitHub Actions returned a malformed OIDC token"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| eyre!("GitHub Actions returned a malformed OIDC token"))?;
    let claims: JwtExpiry = serde_json::from_slice(&payload)
        .map_err(|_| eyre!("GitHub Actions OIDC token is missing a valid expiry"))?;
    Ok(claims.exp)
}

pub(crate) fn unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| eyre!("system clock is before the Unix epoch: {err}"))?
        .as_secs())
}

pub(crate) fn validate_oidc_request_url(url: &Url) -> Result<()> {
    if url.scheme() == "https"
        || url.scheme() == "http"
            && url.host().is_some_and(|host| match host {
                Host::Domain(host) => host.eq_ignore_ascii_case("localhost"),
                Host::Ipv4(address) => address.is_loopback(),
                Host::Ipv6(address) => address.is_loopback(),
            })
    {
        Ok(())
    } else {
        bail!("GitHub Actions OIDC request URL must use HTTPS")
    }
}

pub(crate) fn validate_remote_url(base_url: &Url, authenticated: bool) -> Result<()> {
    if base_url.scheme() == "https" {
        return Ok(());
    }
    if base_url.scheme() != "http" {
        bail!("remote cache URL must use HTTPS");
    }
    let is_loopback = base_url.host().is_some_and(|host| match host {
        Host::Domain(host) => host.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
    });
    if !is_loopback && authenticated {
        bail!("remote cache URL must use HTTPS except for loopback development servers");
    }
    if !is_loopback {
        warn!(
            "using an unauthenticated remote build cache over plain HTTP; cache traffic can be read \
             or modified in transit"
        );
    }
    Ok(())
}

pub(crate) fn normalized_base_url(mut url: Url) -> Url {
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    url
}
