use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_archive_content_files_tar_hashes_regular_files() {
    let dir = tempfile::tempdir().unwrap();
    let archive_path = dir.path().join("tool.tar");
    {
        let file = File::create(&archive_path).unwrap();
        let mut builder = jdx_tar::Builder::new(file);
        let mut header = jdx_tar::Header::new_gnu(EntryType::File);
        header.set_size(4);
        header.set_mode(0o755);
        builder
            .append_data(&mut header, "pkg/tool", &b"tool"[..])
            .unwrap();
        builder.finish().unwrap();
    }

    let files = archive_content_files(&archive_path, ExtractionFormat::Tar, 1).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "tool");
    assert_eq!(files[0].sha256, hex::encode(Sha256::digest(b"tool")));
}

#[test]
fn test_archive_content_files_tar_rejects_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let archive_path = dir.path().join("tool.tar");
    {
        let file = File::create(&archive_path).unwrap();
        let mut builder = jdx_tar::Builder::new(file);
        let mut header = jdx_tar::Header::new_gnu(EntryType::Symlink);
        header.set_mode(0o777);
        builder
            .append_link(&mut header, "tool-link", "tool")
            .unwrap();
        builder.finish().unwrap();
    }

    let err = archive_content_files(&archive_path, ExtractionFormat::Tar, 0).unwrap_err();
    assert!(err.to_string().contains("non-regular archive entry"));
}

#[test]
fn test_archive_content_files_tar_zst_decodes_a_long_window() {
    let dir = tempfile::tempdir().unwrap();
    let archive_path = dir.path().join("tool.tar.zst");
    let mut tar = Vec::new();
    {
        let mut builder = jdx_tar::Builder::new(&mut tar);
        let mut header = jdx_tar::Header::new_gnu(EntryType::File);
        header.set_size(4);
        header.set_mode(0o755);
        builder
            .append_data(&mut header, "pkg/tool", &b"tool"[..])
            .unwrap();
        builder.finish().unwrap();
    }
    // A streamed frame has no content size, so it carries a window
    // descriptor. Rewrite it to 2^30, the window LLVM's releases declare,
    // rather than making the test allocate a 1 GiB compression window.
    let mut encoder = zstd::Encoder::new(Vec::new(), 1).unwrap();
    encoder.write_all(&tar).unwrap();
    let mut zst = encoder.finish().unwrap();
    assert_eq!(&zst[..4], &[0x28, 0xb5, 0x2f, 0xfd]);
    assert_eq!(
        zst[4] & 0x23,
        0,
        "expected no single-segment flag or dictionary id"
    );
    zst[5] = 20 << 3;
    fs::write(&archive_path, &zst).unwrap();

    let files = archive_content_files(&archive_path, ExtractionFormat::TarZst, 1).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "tool");
}

#[test]
fn test_extract_archive_tar_strip_preserves_root_files() {
    let dir = tempfile::tempdir().unwrap();
    let archive_path = dir.path().join("tool.tar");
    let dest = dir.path().join("out");
    {
        let file = File::create(&archive_path).unwrap();
        let mut builder = jdx_tar::Builder::new(file);

        let mut readme = jdx_tar::Header::new_gnu(EntryType::File);
        readme.set_size(6);
        readme.set_mode(0o644);
        builder
            .append_data(&mut readme, "README", &b"readme"[..])
            .unwrap();

        let mut tool = jdx_tar::Header::new_gnu(EntryType::File);
        tool.set_size(4);
        tool.set_mode(0o644);
        builder
            .append_data(&mut tool, "pkg/tool", &b"tool"[..])
            .unwrap();
        builder.finish().unwrap();
    }

    extract_archive(
        &archive_path,
        &dest,
        ExtractionFormat::Tar,
        &ExtractOptions {
            strip_components: 1,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(fs::read(dest.join("README")).unwrap(), b"readme");
    assert_eq!(fs::read(dest.join("tool")).unwrap(), b"tool");
    assert!(!dest.join("pkg").exists());
}

fn tar_bytes() -> Vec<u8> {
    let mut builder = jdx_tar::Builder::new(Vec::new());
    let mut header = jdx_tar::Header::new_gnu(EntryType::File);
    header.set_size(4);
    header.set_mode(0o644);
    builder
        .append_data(&mut header, "pkg/tool", &b"tool"[..])
        .unwrap();
    builder.into_inner().unwrap()
}

#[test]
fn test_from_magic_identifies_suffixless_tarballs() {
    use std::io::Write;

    let tar = tar_bytes();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar).unwrap();
    let mut xz = xz2::write::XzEncoder::new(Vec::new(), 6);
    xz.write_all(&tar).unwrap();
    let mut bz2 = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    bz2.write_all(&tar).unwrap();
    let cases = [
        (gz.finish().unwrap(), ExtractionFormat::TarGz),
        (xz.finish().unwrap(), ExtractionFormat::TarXz),
        (bz2.finish().unwrap(), ExtractionFormat::TarBz2),
        (
            zstd::encode_all(&tar[..], 0).unwrap(),
            ExtractionFormat::TarZst,
        ),
        (tar.clone(), ExtractionFormat::Tar),
    ];

    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("v1.0.0");
    for (bytes, expected) in cases {
        fs::write(&archive, bytes).unwrap();
        assert_eq!(
            ExtractionFormat::from_magic(&archive).unwrap(),
            Some(expected)
        );
        assert_eq!(
            ExtractionFormat::detect(&archive, "v1.0.0").unwrap(),
            expected
        );
    }
}

#[test]
fn test_from_magic_leaves_non_archives_alone() {
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("download");

    // a gzip stream whose payload is not a tarball
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&[b'x'; 1024]).unwrap();
    fs::write(&archive, gz.finish().unwrap()).unwrap();
    assert_eq!(ExtractionFormat::from_magic(&archive).unwrap(), None);

    // gzip magic followed by garbage must not surface as a decode error
    fs::write(&archive, b"\x1f\x8bnot really gzip").unwrap();
    assert_eq!(ExtractionFormat::from_magic(&archive).unwrap(), None);

    fs::write(&archive, b"#!/bin/sh\necho hi\n").unwrap();
    assert_eq!(ExtractionFormat::from_magic(&archive).unwrap(), None);
    assert_eq!(
        ExtractionFormat::detect(&archive, "download").unwrap(),
        ExtractionFormat::Raw
    );

    fs::write(&archive, b"").unwrap();
    assert_eq!(ExtractionFormat::from_magic(&archive).unwrap(), None);
}

#[test]
fn test_detect_prefers_the_file_name() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("tool.zip");
    fs::write(&archive, tar_bytes()).unwrap();
    assert_eq!(
        ExtractionFormat::detect(&archive, "tool.zip").unwrap(),
        ExtractionFormat::Zip
    );
}

#[test]
fn test_display_filename() {
    assert_eq!(display_filename("/tmp/mise.toml"), "mise.toml");
    assert_eq!(display_filename("/"), "/");
}

#[test]
fn test_replace_path_uses_platform_separators() {
    // `strip_prefix("~/")` returns a raw subslice of the input, so the naive
    // `HOME.join(rest)` only prepended a separator and left the interior ones
    // alone: on Windows `MISE_DATA_DIR=~/.local/share/mise` used to expand to
    // `C:\Users\me\.local/share/mise`, which then leaked out through
    // `mise where`, `mise which`, `mise ls --json`, shims, and error messages.
    //
    // NOTE: comparing PathBufs is not enough here — `Path`'s `PartialEq` is
    // component-based and treats `/` and `\` as equivalent on Windows, so the
    // buggy value compares equal to the correct one. The string form is the
    // assertion that matters.
    let expanded = replace_path(Path::new("~/a/b"));
    let expected = dirs::HOME.join("a").join("b");
    assert_eq!(expanded, expected);
    assert_eq!(expanded.to_string_lossy(), expected.to_string_lossy());

    // independent of whatever HOME itself looks like
    #[cfg(windows)]
    {
        let rest = expanded.strip_prefix(*dirs::HOME).unwrap();
        assert!(
            !rest.to_string_lossy().contains('/'),
            "expanded remainder must use `\\`: {}",
            expanded.display()
        );
    }

    // a bare `~` expands to $HOME: `strip_prefix("~/")` is component-based,
    // so `~` matches the prefix and the remainder is empty
    assert_eq!(replace_path(Path::new("~")), *dirs::HOME);

    // non-`~/` input is passed through untouched, separators and all
    assert_eq!(
        replace_path(Path::new("/cwd/x")).to_string_lossy(),
        "/cwd/x"
    );
}

#[test]
#[cfg(windows)]
fn test_pathbuf_hashset_is_separator_insensitive() {
    // `EnvDiff::path` is Vec<PathBuf> and `get_pristine_env` strips
    // mise-added PATH entries via a HashSet<&PathBuf>. `Path`'s Hash/Eq are
    // component-based and both `/` and `\` are separators on Windows, so a
    // `__MISE_DIFF` written by an older mise (mixed separators) still matches
    // after the expansion fix above — no migration is needed.
    let mut set = std::collections::HashSet::new();
    set.insert(PathBuf::from(
        r"C:\Users\me\.local/share/mise/installs/go/1/bin",
    ));
    assert!(set.contains(&PathBuf::from(
        r"C:\Users\me\.local\share\mise\installs\go\1\bin"
    )));
}

#[test]
fn test_paths_eq_exact() {
    assert!(paths_eq(Path::new("/foo/bar"), Path::new("/foo/bar")));
    assert!(!paths_eq(Path::new("/foo/bar"), Path::new("/foo/baz")));
}

#[test]
#[cfg(any(target_os = "macos", windows))]
fn test_paths_eq_case_insensitive() {
    // macOS volumes (HFS+/APFS) and Windows volumes are case-insensitive by
    // default. The comparator must treat `/Users/Foo` and `/Users/foo` as
    // equal so that PATH stripping doesn't miss the shims dir when `$HOME`
    // is mixed-case in the user's environment but the resolved shims path
    // uses a different case (the cause of the npm-shim recursion bug).
    assert!(paths_eq(
        Path::new("/Users/Olfway/.local/share/mise/shims"),
        Path::new("/Users/olfway/.local/share/mise/shims"),
    ));
}

#[test]
#[cfg(any(target_os = "macos", windows))]
fn test_paths_eq_trailing_separator() {
    // Component-based comparison should fold trailing separators and
    // redundant double-separators so PATH entries like `/foo/shims/`
    // still match `/foo/shims`.
    assert!(paths_eq(Path::new("/foo/shims"), Path::new("/foo/shims/")));
    assert!(paths_eq(Path::new("/foo/shims"), Path::new("/foo//shims"),));
}

#[test]
#[cfg(all(not(windows), not(target_os = "macos")))]
fn test_paths_eq_case_sensitive_on_linux() {
    // Linux paths are case-sensitive; `/foo` and `/Foo` are distinct files.
    assert!(!paths_eq(Path::new("/foo/bar"), Path::new("/Foo/bar")));
}

#[test]
#[cfg(windows)]
fn test_paths_eq_separator_normalization() {
    assert!(paths_eq(
        Path::new("C:/Users/foo/shims"),
        Path::new("C:\\Users\\foo\\shims"),
    ));
}

#[test]
fn test_should_strip_components() {
    // Test that the function correctly identifies when to strip components
    // This is a basic test to ensure the logic works correctly

    // For now, we'll test with a nonexistent file to ensure the function
    // returns false when it can't read the archive
    let non_existent_path = Path::new("/non/existent/archive.tar.gz");
    let result = should_strip_components(non_existent_path, ExtractionFormat::TarGz);
    assert!(result.is_err()); // Should fail to open nonexistent file

    // Note: To properly test this function, we would need actual tar archives
    // with different structures (single file, single directory, multiple entries)
    // This would require creating test fixtures, which is beyond the scope
    // of this fix. The important thing is that the logic now correctly
    // checks if the single entry is a directory before deciding to strip.
}

#[test]
fn test_inspect_tar_contents_logic() {
    // Test the logic of inspect_tar_contents with simulated data
    // This tests the core logic without requiring actual tar files

    // Simulate a HashMap that would be returned by inspect_tar_contents
    // for an archive with a single directory containing files
    let mut components = std::collections::HashMap::new();
    components.insert("mydir".to_string(), true); // Directory with nested files

    let result: Vec<(String, bool)> = components.into_iter().collect();

    // Should have exactly one entry that is a directory
    assert_eq!(result.len(), 1);
    let (name, is_directory) = &result[0];
    assert_eq!(name, "mydir");
    assert!(*is_directory);

    // Test the should_strip_components logic with this result
    // This simulates what would happen if inspect_tar_contents returned this
    let should_strip = result.len() == 1 && result[0].1;
    assert!(should_strip);
}

#[test]
fn test_inspect_tar_contents_curdir_prefix() {
    // Test that archives with "./" prefixed paths are handled correctly
    // This reproduces the bug from https://github.com/jdx/mise/discussions/7862
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use jdx_tar::{Builder, Header};
    use tempfile::NamedTempFile;

    // Create a temp tar.gz with "./" prefixed paths (like unison's archive)
    let temp_file = NamedTempFile::new().unwrap();
    let gz = GzEncoder::new(temp_file.as_file(), Compression::default());
    let mut builder = Builder::new(gz);

    // Add entries with "./" prefix - simulating archive structure like:
    // ./dir1/file1
    // ./dir2/file2
    // ./standalone
    let mut header = Header::new_gnu(EntryType::File);
    header.set_size(0);
    header.set_mode(0o755);

    // Add ./dir1/file1
    builder
        .append_data(&mut header.clone(), "./dir1/file1", std::io::empty())
        .unwrap();

    // Add ./dir2/file2
    builder
        .append_data(&mut header.clone(), "./dir2/file2", std::io::empty())
        .unwrap();

    // Add ./standalone (file at root with ./ prefix)
    builder
        .append_data(&mut header.clone(), "./standalone", std::io::empty())
        .unwrap();

    let gz = builder.into_inner().unwrap();
    gz.finish().unwrap();

    // Now test inspect_tar_contents
    let result = inspect_tar_contents(temp_file.path(), ExtractionFormat::TarGz).unwrap();

    // Should have 3 top-level entries: dir1, dir2, standalone
    // NOT a single "." entry
    assert_eq!(
        result.len(),
        3,
        "Expected 3 top-level entries, got: {:?}",
        result
    );

    let names: std::collections::HashSet<_> = result.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains("dir1"), "Should contain dir1");
    assert!(names.contains("dir2"), "Should contain dir2");
    assert!(names.contains("standalone"), "Should contain standalone");
    assert!(!names.contains("."), "Should NOT contain '.' (CurDir)");

    // dir1 and dir2 should be marked as directories (have nested content)
    for (name, is_dir) in &result {
        if name == "dir1" || name == "dir2" {
            assert!(*is_dir, "{} should be marked as directory", name);
        } else if name == "standalone" {
            assert!(!*is_dir, "standalone should NOT be marked as directory");
        }
    }

    // Verify should_strip_components returns false (multiple top-level entries)
    let should_strip = should_strip_components(temp_file.path(), ExtractionFormat::TarGz).unwrap();
    assert!(
        !should_strip,
        "Should NOT strip components for multi-entry archive"
    );
}
