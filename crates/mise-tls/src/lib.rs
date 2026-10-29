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
//! macOS verifies against the keychain, which ignores `SSL_CERT_FILE`, so
//! [`build_client`] adds the certificates in that file to the keychain roots
//! there. Tools that run mise behind a traffic-inspecting proxy, such as Socket
//! Firewall, point `SSL_CERT_FILE` at a CA they generate for each run, which no
//! keychain entry can cover.

use std::net::{IpAddr, Ipv4Addr};
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
/// With [`set_ipv4_only`] on, the client connects over IPv4 only.
pub fn build_client(make: impl Fn() -> ClientBuilder) -> reqwest::Result<Client> {
    let make = || {
        let builder = make().referer(false);
        // Binding the unspecified IPv4 address leaves only IPv4 candidates
        // for the connector to try.
        if ipv4_only() {
            builder.local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        } else {
            builder
        }
    };
    #[cfg(all(
        target_vendor = "apple",
        any(
            feature = "native-tls",
            feature = "rustls",
            feature = "rustls-native-roots"
        )
    ))]
    if let Some(client) = with_ssl_cert_file(make) {
        return Ok(client);
    }
    match make().build() {
        Ok(client) => Ok(client),
        Err(err) => with_bundled_roots(make, err),
    }
}

/// Builds the client with the certificates in `SSL_CERT_FILE` added to the
/// keychain roots. Returns `None` when the variable is unset, or warns once and
/// returns `None` when its certificates cannot be used, so the caller builds
/// with the keychain alone.
#[cfg(all(
    target_vendor = "apple",
    any(
        feature = "native-tls",
        feature = "rustls",
        feature = "rustls-native-roots"
    )
))]
fn with_ssl_cert_file(make: impl Fn() -> ClientBuilder) -> Option<Client> {
    static WARN: std::sync::Once = std::sync::Once::new();
    let path = std::env::var_os("SSL_CERT_FILE").filter(|path| !path.is_empty())?;
    let path = std::path::Path::new(&path);
    let built = certs_from_file(path).and_then(|certs| {
        make()
            .tls_certs_merge(certs)
            .build()
            .map_err(|err| err.to_string())
    });
    match built {
        Ok(client) => Some(client),
        Err(err) => {
            WARN.call_once(|| {
                log::warn!(
                    "ignoring SSL_CERT_FILE {}: {err}. mise is trusting the keychain alone",
                    path.display()
                );
            });
            None
        }
    }
}

/// The certificates in the PEM bundle at `path`, or why there are none.
#[cfg(all(
    any(test, target_vendor = "apple"),
    any(
        feature = "native-tls",
        feature = "rustls",
        feature = "rustls-native-roots"
    )
))]
fn certs_from_file(path: &std::path::Path) -> Result<Vec<reqwest::Certificate>, String> {
    let pem = std::fs::read(path).map_err(|err| err.to_string())?;
    let certs = reqwest::Certificate::from_pem_bundle(&pem).map_err(|err| err.to_string())?;
    if certs.is_empty() {
        return Err("it holds no PEM certificates".to_string());
    }
    Ok(certs)
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

/// Held while a test changes the process environment or builds a client, since
/// building one reads `SSL_CERT_FILE` and `SSL_CERT_DIR`.
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
mod ssl_cert_file_tests {
    use super::*;

    /// A self-signed CA that nothing else trusts.
    const TEST_CA: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIBjDCCATOgAwIBAgIUVbPJq8yZLTrD7b6LnjSCd74GtaIwCgYIKoZIzj0EAwIw\n\
GzEZMBcGA1UEAwwQbWlzZS10bHMgdGVzdCBDQTAgFw0yNjEwMjkxNjQwMzlaGA8y\n\
MTI2MTAwNTE2NDAzOVowGzEZMBcGA1UEAwwQbWlzZS10bHMgdGVzdCBDQTBZMBMG\n\
ByqGSM49AgEGCCqGSM49AwEHA0IABPQed437vHYum31giiyasubjKtaAWzsXo/Xy\n\
qdnJFVjnzEH8obt4rsuMNjvL+GhHkBYGhaPpBoc7KmfwE65GhX2jUzBRMB0GA1Ud\n\
DgQWBBTzcT7B5cYB99uU5FwcBbyQHNiKLDAfBgNVHSMEGDAWgBTzcT7B5cYB99uU\n\
5FwcBbyQHNiKLDAPBgNVHRMBAf8EBTADAQH/MAoGCCqGSM49BAMCA0cAMEQCICUf\n\
CLpV/tzrPQzYoJdl9OQOZfmO5sXnPU7cjqX8MfUkAiBpCVZC4A7jcIz2+BAvMeNs\n\
Xt7cNjbenv1c97l0t1O79g==\n\
-----END CERTIFICATE-----\n";

    fn write(name: &str, contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mise-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn reads_every_certificate_in_a_bundle() {
        let path = write("bundle.pem", &format!("{TEST_CA}{TEST_CA}"));
        assert_eq!(certs_from_file(&path).unwrap().len(), 2);
    }

    #[test]
    fn a_file_without_certificates_is_an_error() {
        let path = write("empty.pem", "not a certificate\n");
        assert!(certs_from_file(&path).is_err());
        assert!(certs_from_file(std::path::Path::new("/nonexistent/mise-tls.pem")).is_err());
    }

    /// The keychain ignores `SSL_CERT_FILE`, so the client must still build
    /// with the file's CA added, and with the keychain alone when the file is
    /// unusable.
    #[cfg(target_vendor = "apple")]
    #[test]
    fn builds_with_ssl_cert_file_on_macos() {
        let ca = write("ca.pem", TEST_CA);
        let _lock = CLIENT_BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for path in [
            ca.as_path(),
            std::path::Path::new("/nonexistent/mise-tls.pem"),
        ] {
            // SAFETY: no other thread reads or writes the environment here.
            unsafe { std::env::set_var("SSL_CERT_FILE", path) };
            let built = build_client(Client::builder);
            unsafe { std::env::remove_var("SSL_CERT_FILE") };
            assert!(built.is_ok(), "{}: {:?}", path.display(), built.err());
        }
        assert!(with_ssl_cert_file(Client::builder).is_none());
        unsafe { std::env::set_var("SSL_CERT_FILE", &ca) };
        let merged = with_ssl_cert_file(Client::builder);
        unsafe { std::env::remove_var("SSL_CERT_FILE") };
        assert!(merged.is_some(), "the CA in SSL_CERT_FILE should be added");
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
