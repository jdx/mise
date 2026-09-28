use pretty_assertions::assert_eq;

use super::*;

#[cfg(windows)]
#[test]
fn atomic_persist_retries_windows_sharing_violations() {
    assert!(should_retry_atomic_persist(
        &std::io::Error::from_raw_os_error(32)
    ));
}

fn utf16le(s: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

#[test]
fn test_decode_text_honours_a_byte_order_mark() {
    // the encoding that broke #5399 — PowerShell shipped hashes.sha256 as UTF-16LE
    assert_eq!(decode_text(&utf16le("abc\n")).unwrap(), "abc\n");

    let mut be = vec![0xfe, 0xff];
    be.extend("abc\n".encode_utf16().flat_map(u16::to_be_bytes));
    assert_eq!(decode_text(&be).unwrap(), "abc\n");

    // a UTF-8 BOM is stripped rather than left to poison the first token
    assert_eq!(decode_text(b"\xef\xbb\xbfabc\n").unwrap(), "abc\n");

    // no BOM: unchanged from plain `read_to_string`
    assert_eq!(decode_text(b"abc\n").unwrap(), "abc\n");
    assert_eq!(decode_text(b"").unwrap(), "");
}

#[test]
fn test_decode_text_rejects_what_it_cannot_decode() {
    // invalid UTF-8 with no BOM still fails, but says why rather than "stream did not
    // contain valid UTF-8"
    let err = decode_text(b"\xff\x00abc").unwrap_err().to_string();
    assert!(err.contains("byte-order mark"), "{err}");

    // an odd trailing byte cannot be a whole UTF-16 code unit
    let mut truncated = utf16le("abc");
    truncated.pop();
    let err = decode_text(&truncated).unwrap_err().to_string();
    assert!(err.contains("truncated UTF-16LE"), "{err}");

    // an unpaired surrogate is well-formed UTF-16 bytes but not a valid string
    let err = decode_text(&[0xff, 0xfe, 0x00, 0xd8])
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalid UTF-16LE"), "{err}");
}

#[test]
fn test_read_to_string_bom_decodes_a_utf16_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("hashes.sha256");
    fs::write(&path, utf16le("deadbeef *tool.tar.gz\n")).unwrap();

    assert_eq!(
        read_to_string_bom(&path).unwrap(),
        "deadbeef *tool.tar.gz\n"
    );
    // the plain reader is what #5399 hit, and is deliberately left alone
    assert!(read_to_string(&path).is_err());
}

#[test]
fn test_is_plain_file_name() {
    for ok in ["tool", "my-tool.exe", "tool.tar.gz", "..hidden", "a b"] {
        assert!(is_plain_file_name(ok), "should accept {ok:?}");
    }
    for bad in [
        "",
        ".",
        "..",
        "../tool",
        "a/b",
        "/abs/tool",
        "..\\tool",
        "a\\b",
        "C:\\tool",
    ] {
        assert!(!is_plain_file_name(bad), "should reject {bad:?}");
    }
}

#[test]
fn test_is_safe_relative_path() {
    for ok in ["tool", "bin/tool", "nested/path/tool.exe", "a b/tool"] {
        assert!(is_safe_relative_path(ok), "should accept {ok:?}");
    }
    for bad in [
        "",
        ".",
        "..",
        "../tool",
        "bin/../../tool",
        "/abs/tool",
        "..\\tool",
        "C:\\tool",
        "\\\\server\\share\\tool",
    ] {
        assert!(!is_safe_relative_path(bad), "should reject {bad:?}");
    }
}

#[tokio::test]
async fn test_run_blocking_current_thread_runtime() {
    // #[tokio::test] uses a current-thread runtime, where
    // tokio::task::block_in_place would panic — the guard must fall back
    // to running the closure inline
    assert_eq!(run_blocking(|| 42), 42);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_run_blocking_multi_thread_runtime() {
    // matches mise's actual runtime (see main.rs) — takes the real
    // tokio::task::block_in_place path
    assert_eq!(run_blocking(|| 42), 42);
}
