//! Trust roots for HTTPS clients.
//!
//! With rustls, reqwest verifies certificates against the operating system's
//! trust store and refuses to build a client when that store has no roots, for
//! example in a minimal container without `ca-certificates`, or when
//! `SSL_CERT_FILE` points at a missing file. [`build_client`] retries such a
//! build with Mozilla's root certificates. Roots that load from the system are
//! never supplemented, so `SSL_CERT_FILE` and `SSL_CERT_DIR` keep replacing the
//! system store.

use reqwest::{Client, ClientBuilder};

/// Builds the client `make` describes. If that fails, builds it again with
/// Mozilla's root certificates added and returns the original error when the
/// second build fails too.
///
/// `make` runs once per attempt because [`ClientBuilder`] cannot be cloned.
pub fn build_client(make: impl Fn() -> ClientBuilder) -> reqwest::Result<Client> {
    match make().build() {
        Ok(client) => Ok(client),
        Err(err) => with_bundled_roots(make, err),
    }
}

#[cfg(all(
    any(feature = "rustls", feature = "rustls-native-roots"),
    unix,
    not(target_os = "android"),
    not(target_vendor = "apple")
))]
fn with_bundled_roots(
    make: impl Fn() -> ClientBuilder,
    err: reqwest::Error,
) -> reqwest::Result<Client> {
    static WARN: std::sync::Once = std::sync::Once::new();
    let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .filter_map(|cert| reqwest::Certificate::from_der(cert.as_ref()).ok());
    match make().tls_certs_merge(roots).build() {
        Ok(client) => {
            WARN.call_once(|| {
                log::warn!(
                    "no CA certificates could be loaded from the system, so mise is trusting its bundled Mozilla roots. Install ca-certificates, or check SSL_CERT_FILE and SSL_CERT_DIR"
                );
            });
            Ok(client)
        }
        Err(_) => Err(err),
    }
}

/// The platform verifier on this target has no empty-store failure to recover
/// from, or this build does not use rustls.
#[cfg(not(all(
    any(feature = "rustls", feature = "rustls-native-roots"),
    unix,
    not(target_os = "android"),
    not(target_vendor = "apple")
)))]
fn with_bundled_roots(
    _make: impl Fn() -> ClientBuilder,
    err: reqwest::Error,
) -> reqwest::Result<Client> {
    Err(err)
}
