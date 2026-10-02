use super::*;
use std::sync::Mutex;

#[test]
fn slsa_detection_requires_signer_fields() {
    let attestation = PreInstallAttestation {
        github_owner: None,
        github_repo: None,
        github_signer_workflow: None,
        cosign_sig_or_bundle_path: None,
        cosign_public_key_path: None,
        cosign_certificate_identity: None,
        cosign_certificate_identity_regexp: None,
        cosign_certificate_oidc_issuer: None,
        slsa_provenance_path: Some(PathBuf::from("provenance.intoto.jsonl")),
        slsa_min_level: None,
        slsa_signer_identity: None,
        slsa_signer_issuer: None,
    };
    assert!(attestation_to_verified(attestation).is_none());
}

impl Vfox {
    pub fn test() -> Self {
        Self {
            runtime_version: "1.0.0".to_string(),
            plugin_dir: PathBuf::from("plugins"),
            cache_dir: PathBuf::from("test/cache"),
            download_dir: PathBuf::from("test/downloads"),
            install_dir: PathBuf::from("test/installs"),
            skip_verification: false,
            cmd_env: None,
            default_inline_shell: None,
            raw_stdio: false,
            terminal_lock: None,
            github_token: None,
            github_token_resolver: None,
            runtime_env_type: None,
            url_rewriter: None,
            http_headers_resolver: None,
            log_tx: None,
            log_handler: None,
        }
    }
}

/// Canonical single-block test vectors for the ASCII string `abc`: SHA-1 from FIPS 180-1,
/// MD5 from RFC 1321 appendix A.5.
const ABC: &[u8] = b"abc";
const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";
const ABC_MD5: &str = "900150983cd24fb0d6963f7d28e17f72";

#[test]
fn log_handler_receives_messages() {
    let messages = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::clone(&messages);
    let mut vfox = Vfox::test();
    vfox.set_log_handler(move |message| received.lock().unwrap().push(message));

    vfox.log_emit("download tool.tar.gz".to_string());

    assert_eq!(
        *messages.lock().unwrap(),
        vec!["download tool.tar.gz".to_string()]
    );
}

fn pre_install_with(sha1: Option<&str>, md5: Option<&str>) -> PreInstall {
    PreInstall {
        version: "1.0.0".to_string(),
        url: None,
        note: None,
        sha256: None,
        md5: md5.map(str::to_string),
        sha1: sha1.map(str::to_string),
        sha512: None,
        // no attestation, so `verify` returns as soon as the checksums are done
        attestation: None,
    }
}

async fn verify_abc(pre_install: PreInstall) -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("artifact.bin");
    std::fs::write(&file, ABC).unwrap();
    let vfox = Vfox::test();
    vfox.verify(&pre_install, &file).await.map(|_| ())
}

#[test]
fn url_rewriter_defaults_to_noop_and_can_be_set() {
    let mut vfox = Vfox::test();
    let original = Url::parse("https://upstream.example/tool.tar.gz").unwrap();
    let mut url = original.clone();
    vfox.rewrite_url(&mut url);
    assert_eq!(url, original);

    vfox.set_url_rewriter(|url| {
        url.set_host(Some("mirror.example")).unwrap();
    });
    vfox.rewrite_url(&mut url);
    assert_eq!(url.as_str(), "https://mirror.example/tool.tar.gz");
}

/// Both of these arms used to be `unimplemented!()`, so a plugin returning either checksum
/// aborted the process — reported as `task N panicked with message "not implemented: sha1"`
/// in #5283.
#[tokio::test]
async fn verify_accepts_sha1_and_md5_checksums() {
    verify_abc(pre_install_with(Some(ABC_SHA1), Some(ABC_MD5)))
        .await
        .unwrap();
    // upstream checksum files are not consistent about case
    verify_abc(pre_install_with(Some(&ABC_SHA1.to_uppercase()), None))
        .await
        .unwrap();
}

#[tokio::test]
async fn verify_rejects_a_mismatched_sha1() {
    let err = verify_abc(pre_install_with(Some(&"0".repeat(40)), None))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Checksum mismatch"), "{err}");
    assert!(err.contains(&format!("sha1:{ABC_SHA1}")), "{err}");
}

#[tokio::test]
async fn verify_rejects_a_mismatched_md5() {
    let err = verify_abc(pre_install_with(None, Some(&"0".repeat(32))))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Checksum mismatch"), "{err}");
    assert!(err.contains(&format!("md5:{ABC_MD5}")), "{err}");
}

#[tokio::test]
async fn test_env_keys() {
    let vfox = Vfox::test();
    // dummy plugin already exists in plugins/dummy, no need to install
    let keys = vfox
        .env_keys(
            "dummy",
            "1.0.0",
            serde_json::Value::Object(Default::default()),
        )
        .await
        .unwrap();
    // Asserted rather than snapshotted: the dummy plugin reports the install dir itself on
    // Windows and its `bin` subdirectory elsewhere (see test_env_keys_for_install_dir), and
    // the separators differ too, so one snapshot cannot describe both.
    let install_dir = vfox.install_dir.join("dummy").join("1.0.0");
    let expected = if cfg!(windows) {
        install_dir
    } else {
        install_dir.join("bin")
    };
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].key, "PATH");
    assert_eq!(keys[0].value, expected.to_string_lossy().into_owned());
}

#[tokio::test]
async fn test_env_keys_for_install_dir() {
    let vfox = Vfox::test();
    let install_dir = PathBuf::from("custom/installs/dummy/1.0.0");
    let keys = vfox
        .env_keys_for_install_dir(
            "dummy",
            "1.0.0",
            &install_dir,
            serde_json::Value::Object(Default::default()),
        )
        .await
        .unwrap();
    let expected = if cfg!(windows) {
        install_dir
    } else {
        install_dir.join("bin")
    };
    assert_eq!(keys[0].value, expected.to_string_lossy().into_owned());
}

#[test]
fn test_download_path_for_uses_download_dir() {
    let url = Url::parse("https://example.com/releases/tool.tar.gz").unwrap();
    let download_dir = PathBuf::from("custom/downloads/vfox-dummy/1.0.0");
    let path = Vfox::download_path_for(&download_dir, "dummy", "1.0.0", &url).unwrap();
    assert_eq!(
        path,
        PathBuf::from("custom/downloads/vfox-dummy/1.0.0/dummy-1.0.0/tool.tar.gz")
    );
}

#[tokio::test]
async fn test_download_resolves_headers_after_url_rewrite() {
    use reqwest::header::{AUTHORIZATION, HeaderValue};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mirror/tool.tar.gz"))
        .and(header("Authorization", "Basic bWlycm9yOnNlY3JldA=="))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"artifact"))
        .expect(1)
        .mount(&server)
        .await;

    let temp = TempDir::new().unwrap();
    let plugin_dir = temp.path().join("dummy");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let plugin = Plugin::from_dir(&plugin_dir).unwrap();
    let mut vfox = Vfox::test();
    let mirror_url = Url::parse(&format!("{}/mirror/tool.tar.gz", server.uri())).unwrap();
    vfox.set_url_rewriter({
        let mirror_url = mirror_url.clone();
        move |url| *url = mirror_url.clone()
    });
    vfox.set_http_headers_resolver(move |url| {
        assert_eq!(url, &mirror_url);
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Basic bWlycm9yOnNlY3JldA=="),
        );
        headers
    });

    let original_url = Url::parse("https://upstream.invalid/tool.tar.gz").unwrap();
    let downloaded = vfox
        .download(&original_url, &plugin, "1.0.0", temp.path())
        .await
        .unwrap();
    assert_eq!(std::fs::read(downloaded).unwrap(), b"artifact");
}

#[tokio::test]
async fn test_install_plugin() {
    let vfox = Vfox::test();
    // dummy plugin already exists in plugins/dummy, just verify it's there
    assert!(vfox.plugin_dir.join("dummy").exists());
    let plugin = Plugin::from_dir(&vfox.plugin_dir.join("dummy")).unwrap();
    assert_eq!(plugin.name, "dummy");
}

#[tokio::test]
async fn test_install() {
    let vfox = Vfox::test();
    let install_dir = vfox.install_dir.join("dummy").join("1.0.0");
    // dummy plugin already exists in plugins/dummy
    vfox.install("dummy", "1.0.0", &install_dir).await.unwrap();
    // dummy plugin doesn't actually install binaries, so we just check the directory
    assert!(vfox.install_dir.join("dummy").join("1.0.0").exists());
    assert_eq!(
        file::read_to_string(vfox.install_dir.join("dummy").join("1.0.0").join("VERSION")).unwrap(),
        "1.0.0"
    );
    vfox.uninstall("dummy", "1.0.0").unwrap();
    assert!(!vfox.install_dir.join("dummy").join("1.0.0").exists());
    file::remove_dir_all(vfox.install_dir).unwrap();
    file::remove_dir_all(vfox.download_dir).unwrap();
}

#[tokio::test]
async fn test_github_token_resolver_not_called_for_local_hooks() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    // env_keys and pre_uninstall on the dummy plugin do no network I/O,
    // so a lazy GitHub token resolver registered on Vfox must not be
    // invoked. This is the regression check for
    // https://github.com/jdx/mise/discussions/9797 — `mise hook-env` and
    // friends must not spawn `github.credential_command`.
    let temp_dir = tempfile::tempdir().unwrap();
    let mut vfox = Vfox::test();
    vfox.install_dir = temp_dir.path().join("installs");
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_inner = calls.clone();
    vfox.github_token_resolver = Some(Arc::new(move || {
        calls_inner.fetch_add(1, Ordering::SeqCst);
        None
    }));

    vfox.env_keys(
        "dummy",
        "1.0.0",
        serde_json::Value::Object(Default::default()),
    )
    .await
    .unwrap();

    let install_dir = vfox.install_dir.join("dummy").join("1.0.0");
    std::fs::create_dir_all(&install_dir).unwrap();
    vfox.pre_uninstall("dummy", "1.0.0", &install_dir)
        .await
        .unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_pre_uninstall() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut vfox = Vfox::test();
    vfox.install_dir = temp_dir.path().join("installs");
    let install_dir = vfox.install_dir.join("dummy").join("1.0.0");
    std::fs::create_dir_all(&install_dir).unwrap();

    vfox.pre_uninstall("dummy", "1.0.0", &install_dir)
        .await
        .unwrap();

    let marker = std::fs::read_to_string(install_dir.join("pre_uninstall_marker")).unwrap();
    assert_eq!(
        marker,
        format!(
            "dummy:1.0.0:{}",
            install_dir.to_string_lossy().replace('\\', "/")
        )
    );
}

#[tokio::test]
#[ignore] // disable for now
async fn test_install_cmake() {
    let vfox = Vfox::test();
    vfox.install_plugin("cmake").unwrap();
    let install_dir = vfox.install_dir.join("cmake").join("3.21.0");
    vfox.install("cmake", "3.21.0", &install_dir).await.unwrap();
    if cfg!(target_os = "linux") {
        assert!(
            vfox.install_dir
                .join("cmake")
                .join("3.21.0")
                .join("bin")
                .join("cmake")
                .exists()
        );
    } else if cfg!(target_os = "macos") {
        assert!(
            vfox.install_dir
                .join("cmake")
                .join("3.21.0")
                .join("CMake.app")
                .join("Contents")
                .join("bin")
                .join("cmake")
                .exists()
        );
    } else if cfg!(target_os = "windows") {
        assert!(
            vfox.install_dir
                .join("cmake")
                .join("3.21.0")
                .join("bin")
                .join("cmake.exe")
                .exists()
        );
    }
    vfox.uninstall_plugin("cmake").unwrap();
    assert!(!vfox.plugin_dir.join("cmake").exists());
    vfox.uninstall("cmake", "3.21.0").unwrap();
    assert!(!vfox.install_dir.join("cmake").join("3.21.0").exists());
    file::remove_dir_all(vfox.plugin_dir.join("cmake")).unwrap();
    file::remove_dir_all(vfox.install_dir).unwrap();
    file::remove_dir_all(vfox.download_dir).unwrap();
}

#[tokio::test]
async fn test_metadata() {
    let vfox = Vfox::test();
    // dummy plugin already exists in plugins/dummy
    let metadata = vfox.metadata("dummy").await.unwrap();
    let out = format!("{metadata:?}");
    assert_snapshot!(out);
}

#[cfg(unix)]
#[tokio::test]
async fn test_backend_list_versions_with_cmd_env() {
    let mut vfox = Vfox::test();
    let mut env = IndexMap::new();
    env.insert("MY_TEST_VAR".to_string(), "hello".to_string());
    env.insert(
        "PATH".to_string(),
        std::env::var("PATH").unwrap_or_default(),
    );
    vfox.cmd_env = Some(env);

    let versions = vfox
        .backend_list_versions("dummy-backend", "test-tool", IndexMap::new())
        .await
        .unwrap();
    assert_eq!(versions, vec!["hello".to_string()]);
}

#[tokio::test]
async fn test_backend_list_versions_without_cmd_env() {
    let vfox = Vfox::test();
    let versions = vfox
        .backend_list_versions("dummy-backend", "test-tool", IndexMap::new())
        .await
        .unwrap();
    assert_eq!(versions, vec!["fallback".to_string()]);
}

#[tokio::test]
async fn test_backend_search_tools() {
    let vfox = Vfox::test();
    let tools = vfox
        .backend_search_tools("dummy-backend", "dem".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        tools,
        vec![BackendTool {
            name: "search-dem".into(),
            description: Some("A dynamically discovered tool".into()),
        }]
    );
}

#[tokio::test]
async fn test_backend_list_tools() {
    let vfox = Vfox::test();
    let tools = vfox
        .backend_list_tools("dummy-backend")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tools[0].name, "demo");
}

#[tokio::test]
async fn test_backend_search_tools_is_optional() {
    let vfox = Vfox::test();
    assert_eq!(
        vfox.backend_search_tools("dummy", "dem".into())
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn test_backend_list_tools_is_optional() {
    let vfox = Vfox::test();
    assert_eq!(vfox.backend_list_tools("dummy").await.unwrap(), None);
}
