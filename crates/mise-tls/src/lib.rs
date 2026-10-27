//! Trust roots for HTTPS clients.
//!
//! With rustls, reqwest verifies certificates against the operating system's
//! trust store and refuses to build a client when that store has no roots, for
//! example in a minimal container without `ca-certificates`, or when
//! `SSL_CERT_FILE` points at a missing file. [`build_client`] retries such a
//! build with Mozilla's root certificates. Roots that load from the system are
//! never supplemented, so `SSL_CERT_FILE` and `SSL_CERT_DIR` keep replacing the
//! system store.

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
