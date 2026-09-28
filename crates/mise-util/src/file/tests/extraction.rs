use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_extraction_format_from_file_name() {
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tar.gz"),
        ExtractionFormat::TarGz
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tgz"),
        ExtractionFormat::TarGz
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tar.xz"),
        ExtractionFormat::TarXz
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.txz"),
        ExtractionFormat::TarXz
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tar.bz2"),
        ExtractionFormat::TarBz2
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tbz2"),
        ExtractionFormat::TarBz2
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tbz"),
        ExtractionFormat::TarBz2
    );
    assert_eq!(
        ExtractionFormat::from_ext("tbz"),
        Some(ExtractionFormat::TarBz2)
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tar.zst"),
        ExtractionFormat::TarZst
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tzst"),
        ExtractionFormat::TarZst
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tar"),
        ExtractionFormat::Tar
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.zip"),
        ExtractionFormat::Zip
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.vsix"),
        ExtractionFormat::Zip
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.7z"),
        ExtractionFormat::SevenZip
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tar.br"),
        ExtractionFormat::TarBr
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tbr"),
        ExtractionFormat::TarBr
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.br"),
        ExtractionFormat::Br
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tar.lz4"),
        ExtractionFormat::TarLz4
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tlz4"),
        ExtractionFormat::TarLz4
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.lz4"),
        ExtractionFormat::Lz4
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tar.sz"),
        ExtractionFormat::TarSz
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.tsz"),
        ExtractionFormat::TarSz
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.sz"),
        ExtractionFormat::Sz
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.rar"),
        ExtractionFormat::Rar
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.gz"),
        ExtractionFormat::Gz
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.xz"),
        ExtractionFormat::Xz
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.bz2"),
        ExtractionFormat::Bz2
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.zst"),
        ExtractionFormat::Zst
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo"),
        ExtractionFormat::Raw
    );
    assert_eq!(
        ExtractionFormat::from_file_name("foo.txt"),
        ExtractionFormat::Raw
    );
}

#[test]
fn test_unsupported_extraction_formats_are_classified() {
    for (ext, expected) in [
        ("tar.br", ExtractionFormat::TarBr),
        ("tbr", ExtractionFormat::TarBr),
        ("br", ExtractionFormat::Br),
        ("tar.lz4", ExtractionFormat::TarLz4),
        ("tlz4", ExtractionFormat::TarLz4),
        ("lz4", ExtractionFormat::Lz4),
        ("tar.sz", ExtractionFormat::TarSz),
        ("tsz", ExtractionFormat::TarSz),
        ("sz", ExtractionFormat::Sz),
        ("rar", ExtractionFormat::Rar),
    ] {
        assert_eq!(ExtractionFormat::from_ext(ext), Some(expected));
    }
    assert_eq!(ExtractionFormat::from_ext("unknown"), None);

    assert!(ExtractionFormat::TarBr.is_archive());
    assert!(ExtractionFormat::TarLz4.is_archive());
    assert!(ExtractionFormat::TarSz.is_archive());
    assert!(ExtractionFormat::Rar.is_archive());
    assert!(ExtractionFormat::Br.is_compressed_file());
    assert!(ExtractionFormat::Lz4.is_compressed_file());
    assert!(ExtractionFormat::Sz.is_compressed_file());
}

#[test]
fn test_extraction_format_extension_uses_canonical_display() {
    for (format, expected) in [
        (ExtractionFormat::TarGz, Some("tar.gz")),
        (ExtractionFormat::TarXz, Some("tar.xz")),
        (ExtractionFormat::TarBz2, Some("tar.bz2")),
        (ExtractionFormat::TarZst, Some("tar.zst")),
        (ExtractionFormat::TarBr, Some("tar.br")),
        (ExtractionFormat::TarLz4, Some("tar.lz4")),
        (ExtractionFormat::TarSz, Some("tar.sz")),
        (ExtractionFormat::Zip, Some("zip")),
        (ExtractionFormat::Raw, None),
    ] {
        assert_eq!(format.extension().as_deref(), expected);
    }
}

#[test]
fn test_decompress_file() {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let src_path = dir.path().join("test.gz");
    let dest_path = dir.path().join("test-out");

    let file = File::create(&src_path).unwrap();
    let mut encoder = GzEncoder::new(file, Compression::default());
    encoder.write_all(b"hello world").unwrap();
    encoder.finish().unwrap();

    decompress_file(&src_path, &dest_path, ExtractionFormat::Gz).unwrap();

    assert!(dest_path.exists());
    assert!(dest_path.is_file());
    let content = std::fs::read_to_string(&dest_path).unwrap();
    assert_eq!(content, "hello world");
}

#[test]
fn test_decompress_file_creates_parent_dir() {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let src_path = dir.path().join("test.gz");
    let dest_path = dir.path().join("missing").join("test-out");

    let file = File::create(&src_path).unwrap();
    let mut encoder = GzEncoder::new(file, Compression::default());
    encoder.write_all(b"hello world").unwrap();
    encoder.finish().unwrap();

    decompress_file(&src_path, &dest_path, ExtractionFormat::Gz).unwrap();

    assert!(dest_path.exists());
    assert!(dest_path.is_file());
    let content = std::fs::read_to_string(&dest_path).unwrap();
    assert_eq!(content, "hello world");
}

#[test]
fn test_extract_archive_zip() {
    use std::io::Write;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let src_path = dir.path().join("test.zip");
    let dest_dir = dir.path().join("out_dir");

    let file = File::create(&src_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("pkg/tool", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"hello world").unwrap();
    zip.finish().unwrap();

    extract_archive(
        &src_path,
        &dest_dir,
        ExtractionFormat::Zip,
        &ExtractOptions::default(),
    )
    .unwrap();

    let extracted_path = dest_dir.join("pkg").join("tool");
    assert!(extracted_path.exists());
    assert!(extracted_path.is_file());
    let content = std::fs::read_to_string(&extracted_path).unwrap();
    assert_eq!(content, "hello world");
}

#[test]
fn test_extract_archive_7z() {
    use std::io::Cursor;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src");
    let pkg_dir = src_dir.join("pkg");
    let archive_path = dir.path().join("test.7z");
    let dest_dir = dir.path().join("out_dir");
    let stripped_dest_dir = dir.path().join("stripped_out_dir");
    let backslash_archive_path = dir.path().join("backslash.7z");
    let backslash_dest_dir = dir.path().join("backslash_out_dir");
    let traversal_archive_path = dir.path().join("traversal.7z");
    let traversal_dest_dir = dir.path().join("traversal_out_dir");
    let traversal_target_path = dir.path().join("traversal_target");
    let absolute_archive_path = dir.path().join("absolute.7z");
    let absolute_dest_dir = dir.path().join("absolute_out_dir");
    let absolute_target_path = dir.path().join("absolute_target");

    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(pkg_dir.join("tool"), "hello world").unwrap();
    sevenz_rust2::compress_to_path(&src_dir, &archive_path).unwrap();

    let contents = inspect_7z_contents(&archive_path).unwrap();
    assert!(contents.contains(&("pkg".to_string(), true)));
    assert!(should_strip_components(&archive_path, ExtractionFormat::SevenZip).unwrap());

    extract_archive(
        &archive_path,
        &dest_dir,
        ExtractionFormat::SevenZip,
        &ExtractOptions::default(),
    )
    .unwrap();

    let extracted_path = dest_dir.join("pkg").join("tool");
    assert!(extracted_path.exists());
    assert!(extracted_path.is_file());
    let content = std::fs::read_to_string(&extracted_path).unwrap();
    assert_eq!(content, "hello world");

    extract_archive(
        &archive_path,
        &stripped_dest_dir,
        ExtractionFormat::SevenZip,
        &ExtractOptions {
            strip_components: 1,
            ..Default::default()
        },
    )
    .unwrap();

    let stripped_path = stripped_dest_dir.join("tool");
    assert!(stripped_path.exists());
    assert!(stripped_path.is_file());
    assert!(!stripped_dest_dir.join("pkg").exists());
    let content = std::fs::read_to_string(&stripped_path).unwrap();
    assert_eq!(content, "hello world");

    let mut backslash_archive =
        sevenz_rust2::ArchiveWriter::create(&backslash_archive_path).unwrap();
    backslash_archive
        .push_archive_entry(
            sevenz_rust2::ArchiveEntry::new_file("pkg\\tool"),
            Some(Cursor::new(b"hello world")),
        )
        .unwrap();
    backslash_archive.finish().unwrap();

    let contents = inspect_7z_contents(&backslash_archive_path).unwrap();
    assert!(contents.contains(&("pkg".to_string(), true)));
    assert!(should_strip_components(&backslash_archive_path, ExtractionFormat::SevenZip).unwrap());

    extract_archive(
        &backslash_archive_path,
        &backslash_dest_dir,
        ExtractionFormat::SevenZip,
        &ExtractOptions {
            strip_components: 1,
            ..Default::default()
        },
    )
    .unwrap();

    let backslash_stripped_path = backslash_dest_dir.join("tool");
    assert!(backslash_stripped_path.exists());
    assert!(backslash_stripped_path.is_file());
    assert!(!backslash_dest_dir.join("pkg").exists());
    let content = std::fs::read_to_string(&backslash_stripped_path).unwrap();
    assert_eq!(content, "hello world");

    let mut traversal_archive =
        sevenz_rust2::ArchiveWriter::create(&traversal_archive_path).unwrap();
    traversal_archive
        .push_archive_entry(
            sevenz_rust2::ArchiveEntry::new_file("../traversal_target"),
            Some(Cursor::new(b"malicious")),
        )
        .unwrap();
    traversal_archive.finish().unwrap();

    let err = extract_archive(
        &traversal_archive_path,
        &traversal_dest_dir,
        ExtractionFormat::SevenZip,
        &ExtractOptions::default(),
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("escapes"), "{err:#}");
    assert!(!traversal_target_path.exists());

    let mut absolute_archive = sevenz_rust2::ArchiveWriter::create(&absolute_archive_path).unwrap();
    absolute_archive
        .push_archive_entry(
            sevenz_rust2::ArchiveEntry::new_file(&absolute_target_path.to_string_lossy()),
            Some(Cursor::new(b"malicious")),
        )
        .unwrap();
    absolute_archive.finish().unwrap();

    let err = extract_archive(
        &absolute_archive_path,
        &absolute_dest_dir,
        ExtractionFormat::SevenZip,
        &ExtractOptions::default(),
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("escapes"), "{err:#}");
    assert!(!absolute_target_path.exists());
}

#[test]
fn test_extract_archive_ignores_malformed_non_sparse_pax_metadata() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let archive_path = dir.path().join("pax-xattr.tar");
    let dest_dir = dir.path().join("out");
    let mut builder = jdx_tar::Builder::new(Vec::new());

    let key = "LIBARCHIVE.xattr.com.apple.cs.CodeSignature";
    let value = b"signature\nmetadata";
    let rest_len = 3 + key.len() + value.len();
    let mut digits = 1;
    while (rest_len + digits).to_string().len() != digits {
        digits += 1;
    }
    let record_len = rest_len + digits;
    let mut pax = format!("{record_len} {key}=").into_bytes();
    pax.extend_from_slice(value);
    pax.push(b'\n');
    // Build the raw PAX record under a neutral extension flag, then rewrite
    // the type byte and checksum. jdx-tar's logical writer deliberately
    // rejects raw extension headers, while this test specifically needs a
    // malformed raw PAX fixture to exercise the reader.
    let mut pax_header = jdx_tar::Header::new_gnu(EntryType::Other(b'X'));
    pax_header.set_size(pax.len() as u64);
    builder
        .append_data(&mut pax_header, "pax-xattr", pax.as_slice())
        .unwrap();

    let contents = b"hello world";
    let mut header = jdx_tar::Header::new_gnu(EntryType::File);
    header.set_size(contents.len() as u64);
    header.set_mode(0o755);
    builder
        .append_data(&mut header, "tool", contents.as_slice())
        .unwrap();
    let mut archive = builder.into_inner().unwrap();

    archive[156] = b'x';
    archive[257..265].copy_from_slice(b"ustar\x0000");
    archive[148..156].fill(b' ');
    let checksum = archive[..512]
        .iter()
        .map(|byte| u64::from(*byte))
        .sum::<u64>();
    archive[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
    std::fs::write(&archive_path, archive).unwrap();

    extract_archive(
        &archive_path,
        &dest_dir,
        ExtractionFormat::Tar,
        &ExtractOptions::default(),
    )
    .unwrap();

    assert_eq!(std::fs::read(dest_dir.join("tool")).unwrap(), contents);
}

#[test]
#[cfg(unix)]
fn test_extract_archive_handles_pax_sparse_tar() {
    use std::io::{Seek, SeekFrom, Write};
    use std::process::Command;
    use tempfile::tempdir;

    if Command::new("tar").arg("--version").output().is_err() {
        return;
    }

    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src").join("pkg");
    let archive_path = dir.path().join("sparse.tar");
    let dest_dir = dir.path().join("out");
    let disk_path = src_dir.join("disk.img");

    std::fs::create_dir_all(&src_dir).unwrap();
    let mut disk = File::create(&disk_path).unwrap();
    disk.write_all(b"begin").unwrap();
    disk.seek(SeekFrom::Start(10 * 1024 * 1024 - 3)).unwrap();
    disk.write_all(b"end").unwrap();
    disk.flush().unwrap();

    let status = Command::new("tar")
        .arg("--sparse")
        .arg("--format=posix")
        .arg("-cf")
        .arg(&archive_path)
        .arg("-C")
        .arg(dir.path().join("src"))
        .arg("pkg")
        .status()
        .unwrap();

    if !status.success() {
        return;
    }

    let archive_contents = std::fs::read(&archive_path).unwrap();
    if !archive_contents
        .windows(b"GNU.sparse.".len())
        .any(|window| window == b"GNU.sparse.")
    {
        return;
    }

    extract_archive(
        &archive_path,
        &dest_dir,
        ExtractionFormat::Tar,
        &ExtractOptions::default(),
    )
    .unwrap();

    let extracted_disk = dest_dir.join("pkg").join("disk.img");
    assert_eq!(
        std::fs::metadata(&extracted_disk).unwrap().len(),
        10 * 1024 * 1024
    );
    let mut extracted = File::open(&extracted_disk).unwrap();
    let mut buf = [0; 5];
    extracted.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"begin");
    extracted
        .seek(SeekFrom::Start(10 * 1024 * 1024 - 3))
        .unwrap();
    let mut buf = [0; 3];
    extracted.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"end");
    assert!(!WalkDir::new(&dest_dir).into_iter().any(|entry| {
        entry
            .ok()
            .and_then(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with("GNUSparseFile."))
            })
            .is_some_and(|matches| matches)
    }));
}

#[test]
fn test_untar_rejects_single_file_compression() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let src_path = dir.path().join("test.gz");
    let dest_path = dir.path().join("test-out");
    let err = untar(
        &src_path,
        &dest_path,
        ExtractionFormat::Gz,
        &ExtractOptions::default(),
    )
    .unwrap_err();

    assert!(
        format!("{err:#}").contains("untar only supports tar formats"),
        "{err:#}"
    );
}

#[test]
fn test_unsupported_extraction_formats_error_clearly() {
    use tempfile::NamedTempFile;
    use tempfile::tempdir;

    let archive = NamedTempFile::new().unwrap();
    let dest = tempdir().unwrap();

    let err = extract_archive(
        archive.path(),
        dest.path(),
        ExtractionFormat::TarBr,
        &ExtractOptions::default(),
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("tar.br format not supported"));

    let err = extract_archive(
        archive.path(),
        dest.path(),
        ExtractionFormat::Rar,
        &ExtractOptions::default(),
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("rar format not supported"));

    let err = decompress_file(
        archive.path(),
        dest.path().join("tool").as_path(),
        ExtractionFormat::Lz4,
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("lz4 format not supported"));
}
