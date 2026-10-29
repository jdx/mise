//! Trust roots for HTTPS clients.
//!
//! With rustls, reqwest verifies certificates against the operating system's
//! trust store and refuses to build a client when that store has no roots, for
//! example in a minimal container without `ca-certificates`, or when
//! `SSL_CERT_FILE` points at a missing file. [`build_client`] retries such a
//! build with Mozilla's root certificates. Roots that load from the system are
//! never supplemented, so `SSL_CERT_FILE` and `SSL_CERT_DIR` keep replacing the
//! system store.
//!
//! [`set_extra_ca_certs`] adds the certificates in a PEM bundle to whatever
//! roots a client trusts, on every platform. It is how mise trusts a
//! traffic-inspecting proxy whose CA is not in the system store, such as one
//! that generates a new CA for each run.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use reqwest::{Client, ClientBuilder};

static IPV4_ONLY: AtomicBool = AtomicBool::new(false);

/// Makes every client built from now on connect over IPv4 only. Clients that
/// already exist keep the address family they were built with.
pub fn set_ipv4_only(ipv4_only: bool) {
    IPV4_ONLY.store(ipv4_only, Ordering::Relaxed);
}

/// Whether [`set_ipv4_only`] is on.
pub fn ipv4_only() -> bool {
    IPV4_ONLY.load(Ordering::Relaxed)
}

/// The bundle [`set_extra_ca_certs`] last read, and the certificates in it or
/// why it cannot be used.
#[derive(Default)]
struct ExtraCaCerts {
    path: Option<PathBuf>,
    error: Option<String>,
    /// Whether a client build has reported `error` yet.
    warned: bool,
    #[cfg(any(
        feature = "native-tls",
        feature = "rustls",
        feature = "rustls-native-roots"
    ))]
    certs: Vec<reqwest::Certificate>,
}

static EXTRA_CA_CERTS: Mutex<Option<ExtraCaCerts>> = Mutex::new(None);

/// Makes every client built from now on trust the certificates in the PEM
/// bundle at `path` as well as its usual roots, or only its usual roots when
/// `path` is `None`. Clients that already exist keep the roots they were built
/// with.
///
/// A bundle that is not an absolute path, cannot be read or holds no
/// certificates leaves clients with their usual roots, and the first client
/// built afterwards logs a warning saying so. A relative path is refused
/// because it would resolve against the working directory, which may be a
/// repository that should not choose which CAs mise trusts. Setting the bundle
/// already set does not read it again.
pub fn set_extra_ca_certs(path: Option<&Path>) {
    let mut extra = EXTRA_CA_CERTS.lock().unwrap_or_else(|e| e.into_inner());
    if extra
        .as_ref()
        .is_some_and(|extra| extra.path.as_deref() == path)
    {
        return;
    }
    let mut next = ExtraCaCerts {
        path: path.map(Path::to_path_buf),
        ..Default::default()
    };
    if let Some(path) = path {
        let loaded = if path.is_absolute() {
            load_extra_ca_certs(&mut next, path)
        } else {
            Err("it is not an absolute path".to_string())
        };
        next.error = loaded.err();
    }
    *extra = Some(next);
}

/// Why the bundle [`set_extra_ca_certs`] was given cannot be used, if it cannot.
#[cfg(all(
    test,
    any(
        feature = "native-tls",
        feature = "rustls",
        feature = "rustls-native-roots"
    )
))]
fn extra_ca_certs_error() -> Option<String> {
    let extra = EXTRA_CA_CERTS.lock().unwrap_or_else(|e| e.into_inner());
    extra.as_ref().and_then(|extra| extra.error.clone())
}

/// Logs, once per bundle, that the bundle [`set_extra_ca_certs`] was given
/// cannot be used. The settings that name the bundle load before logging is
/// set up, while clients are built after.
fn warn_unusable_extra_ca_certs(extra: &mut ExtraCaCerts) {
    if let (Some(path), Some(err), false) = (&extra.path, &extra.error, extra.warned) {
        log::warn!(
            "ignoring extra_ca_certs {}: {err}. mise is trusting its usual root certificates alone",
            path.display()
        );
        extra.warned = true;
    }
}

#[cfg(any(
    feature = "native-tls",
    feature = "rustls",
    feature = "rustls-native-roots"
))]
fn load_extra_ca_certs(extra: &mut ExtraCaCerts, path: &Path) -> Result<(), String> {
    let pem = std::fs::read(path).map_err(|err| err.to_string())?;
    let certs = reqwest::Certificate::from_pem_bundle(&pem).map_err(|err| err.to_string())?;
    if certs.is_empty() {
        return Err("it holds no PEM certificates".to_string());
    }
    extra.certs = certs;
    Ok(())
}

#[cfg(not(any(
    feature = "native-tls",
    feature = "rustls",
    feature = "rustls-native-roots"
)))]
fn load_extra_ca_certs(_extra: &mut ExtraCaCerts, _path: &Path) -> Result<(), String> {
    Err("this build of mise has no TLS support".to_string())
}

/// Adds the certificates [`set_extra_ca_certs`] read to `builder`'s roots.
#[cfg(any(
    feature = "native-tls",
    feature = "rustls",
    feature = "rustls-native-roots"
))]
fn with_extra_ca_certs(builder: ClientBuilder) -> ClientBuilder {
    let mut extra = EXTRA_CA_CERTS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(extra) = extra.as_mut() else {
        return builder;
    };
    warn_unusable_extra_ca_certs(extra);
    if extra.certs.is_empty() {
        builder
    } else {
        builder.tls_certs_merge(extra.certs.clone())
    }
}

#[cfg(not(any(
    feature = "native-tls",
    feature = "rustls",
    feature = "rustls-native-roots"
)))]
fn with_extra_ca_certs(builder: ClientBuilder) -> ClientBuilder {
    let mut extra = EXTRA_CA_CERTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(extra) = extra.as_mut() {
        warn_unusable_extra_ca_certs(extra);
    }
    builder
}

/// Builds the client `make` describes. If that fails, builds it again with
/// Mozilla's root certificates added and returns the original error when the
/// second build fails too.
///
/// `make` runs once per attempt because [`ClientBuilder`] cannot be cloned.
///
/// Every client built here leaves out the `Referer` header. reqwest would
/// otherwise send the previous URL, with any token in its path or query
/// (`?private_token=`, a signed URL), to every host a request is redirected
/// to, and mise has no use for the header.
///
/// With [`set_ipv4_only`] on, the client connects over IPv4 only, and it trusts
/// the certificates [`set_extra_ca_certs`] read as well as its usual roots.
pub fn build_client(make: impl Fn() -> ClientBuilder) -> reqwest::Result<Client> {
    let make = || {
        let builder = with_extra_ca_certs(make().referer(false));
        // Binding the unspecified IPv4 address leaves only IPv4 candidates
        // for the connector to try.
        if ipv4_only() {
            builder.local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        } else {
            builder
        }
    };
    let build = || match make().build() {
        Ok(client) => Ok(client),
        Err(err) => with_bundled_roots(make, err),
    };
    match build() {
        // With rustls a certificate is only checked when a client is built, so
        // a bundle that loaded can still be what fails the build.
        Err(err) if discard_extra_ca_certs(&err) => build(),
        result => result,
    }
}

/// Drops the extra CA certificates, if there are any, after `err` failed a
/// client build with them, so the next build has the usual roots alone and
/// warns that the bundle was ignored. Returns whether there were any to drop.
fn discard_extra_ca_certs(err: &reqwest::Error) -> bool {
    let mut extra = EXTRA_CA_CERTS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(extra) = extra.as_mut() else {
        return false;
    };
    #[cfg(any(
        feature = "native-tls",
        feature = "rustls",
        feature = "rustls-native-roots"
    ))]
    if !extra.certs.is_empty() {
        extra.certs.clear();
        let mut message = err.to_string();
        let mut source = std::error::Error::source(err);
        while let Some(cause) = source {
            message.push_str(&format!(": {cause}"));
            source = cause.source();
        }
        extra.error = Some(message);
        return true;
    }
    let _ = (extra, err);
    false
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

/// Held while a test changes the process environment or the extra CA
/// certificates, or builds a client, since building one reads `SSL_CERT_FILE`,
/// `SSL_CERT_DIR` and the extra CA certificates.
#[cfg(test)]
static CLIENT_BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(all(
    test,
    any(feature = "rustls", feature = "rustls-native-roots"),
    unix,
    not(target_os = "android"),
    not(target_vendor = "apple")
))]
mod tests {
    use super::*;

    /// Holds `CLIENT_BUILD_LOCK`, so no other test builds a client while the
    /// process environment changes.
    #[test]
    fn falls_back_to_bundled_roots_when_the_system_store_is_empty() {
        let _lock = CLIENT_BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: no other thread reads or writes the environment here.
        unsafe {
            std::env::remove_var("SSL_CERT_FILE");
            std::env::set_var("SSL_CERT_DIR", "/nonexistent/mise-tls-test");
        }
        // Pinned to rustls: with `native-tls` also enabled (`--all-features`), the
        // default backend would not read this environment at all.
        assert!(
            Client::builder().tls_backend_rustls().build().is_err(),
            "reqwest should refuse to build with no system roots"
        );
        assert!(build_client(|| Client::builder().tls_backend_rustls()).is_ok());
    }
}

#[cfg(all(
    test,
    any(
        feature = "native-tls",
        feature = "rustls",
        feature = "rustls-native-roots"
    )
))]
mod extra_ca_certs_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::Arc;

    fn temp_file(name: &str, contents: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mise-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// Serves HTTPS on a loopback port with a certificate signed by a new CA
    /// that nothing trusts. Returns that CA as PEM and the server's URL.
    fn serve_https() -> (String, String) {
        let now = time::OffsetDateTime::now_utc();
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::default();
        ca_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "mise-tls test CA");
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
        ];
        ca_params.not_before = now - time::Duration::days(1);
        ca_params.not_after = now + time::Duration::days(1);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let issuer = rcgen::Issuer::new(ca_params, ca_key);

        // macOS rejects server certificates valid for more than 825 days or
        // without the serverAuth usage, even from a trusted CA.
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.subject_alt_names = vec![rcgen::SanType::IpAddress(Ipv4Addr::LOCALHOST.into())];
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
        params.not_before = now - time::Duration::days(1);
        params.not_after = now + time::Duration::days(1);
        let cert = params.signed_by(&key, &issuer).unwrap();

        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )
        .unwrap();
        let config = Arc::new(config);
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let url = format!("https://{}/", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let Ok(conn) = rustls::ServerConnection::new(config.clone()) else {
                    continue;
                };
                let mut tls = rustls::StreamOwned::new(conn, stream);
                // A client that rejects the certificate fails the handshake here.
                if tls.read(&mut [0; 1024]).is_err() {
                    continue;
                }
                let _ = tls.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                tls.conn.send_close_notify();
                let _ = tls.flush();
            }
        });
        (ca.pem(), url)
    }

    /// A TLS backend's name and a client builder that uses it.
    type Backend = (&'static str, fn() -> ClientBuilder);

    /// The TLS backends this build has, each as a client builder that ignores
    /// any proxy in the environment.
    fn backends() -> Vec<Backend> {
        let mut backends: Vec<Backend> = Vec::new();
        #[cfg(feature = "native-tls")]
        backends.push(("native-tls", || {
            Client::builder().no_proxy().tls_backend_native()
        }));
        #[cfg(any(feature = "rustls", feature = "rustls-native-roots"))]
        backends.push(("rustls", || {
            Client::builder().no_proxy().tls_backend_rustls()
        }));
        backends
    }

    /// Holds `CLIENT_BUILD_LOCK`, so no other test builds a client while the
    /// extra CA certificates are set.
    #[tokio::test]
    async fn trusts_a_server_signed_by_the_extra_ca() {
        let (ca, url) = serve_https();
        let ca = temp_file("ca.pem", ca.as_bytes());
        let clients = {
            let _lock = CLIENT_BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let clients = backends()
                .into_iter()
                .map(|(name, make)| {
                    set_extra_ca_certs(None);
                    let without = build_client(make);
                    set_extra_ca_certs(Some(&ca));
                    let with = build_client(make);
                    (name, without.unwrap(), with.unwrap())
                })
                .collect::<Vec<_>>();
            set_extra_ca_certs(None);
            clients
        };
        assert!(!clients.is_empty());
        for (name, without, with) in clients {
            assert!(
                without.get(&url).send().await.is_err(),
                "{name}: the test CA must not be trusted by default"
            );
            let response = with.get(&url).send().await;
            assert!(
                response.is_ok(),
                "{name}: the extra CA should be trusted: {:?}",
                response.err()
            );
        }
    }

    #[test]
    fn a_bundle_it_cannot_use_leaves_the_usual_roots() {
        let empty = temp_file("empty.pem", b"not a certificate\n");
        let missing = Path::new("/nonexistent/mise-tls.pem");
        let _lock = CLIENT_BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_extra_ca_certs(Some(&empty));
        assert_eq!(
            extra_ca_certs_error().as_deref(),
            Some("it holds no PEM certificates")
        );
        assert!(build_client(Client::builder).is_ok());
        set_extra_ca_certs(Some(missing));
        assert!(extra_ca_certs_error().is_some());
        assert!(build_client(Client::builder).is_ok());
        // A PEM block whose contents are not a certificate fails only when a
        // rustls client is built, and that build falls back to the usual roots.
        let malformed = temp_file(
            "malformed.pem",
            b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
        );
        set_extra_ca_certs(Some(&malformed));
        for (name, make) in backends() {
            assert!(build_client(make).is_ok(), "{name}");
        }
        assert!(extra_ca_certs_error().is_some());
        // Refused before it is opened, whatever the working directory holds.
        set_extra_ca_certs(Some(Path::new("certs/proxy.pem")));
        assert_eq!(
            extra_ca_certs_error().as_deref(),
            Some("it is not an absolute path")
        );
        assert!(build_client(Client::builder).is_ok());
        set_extra_ca_certs(None);
        assert!(extra_ca_certs_error().is_none());
    }
}

#[cfg(test)]
mod ipv4_only_tests {
    use super::*;
    use std::io::{Read, Write};

    /// The only test that sets the flag, and it clears it before any assertion
    /// that could end the test early.
    #[tokio::test]
    async fn ipv4_only_refuses_an_ipv6_destination() {
        // A machine without IPv6 loopback has nothing to test.
        let Ok(listener) = std::net::TcpListener::bind("[::1]:0") else {
            return;
        };
        let url = format!("http://{}/", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let _ = stream.read(&mut [0; 1024]);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });

        let get = |client: Client| {
            let url = url.clone();
            async move { client.get(url).send().await }
        };
        let make = || Client::builder().no_proxy();
        let (default, pinned) = {
            let _lock = CLIENT_BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let default = build_client(make).unwrap();
            set_ipv4_only(true);
            let pinned = build_client(make);
            set_ipv4_only(false);
            (default, pinned.unwrap())
        };

        assert!(get(default).await.is_ok(), "IPv6 is reachable by default");
        assert!(
            get(pinned).await.is_err(),
            "an IPv4-only client must not connect to an IPv6 address"
        );
    }
}
