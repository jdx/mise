//! Load an OCI image layout into the docker daemon via `docker load`.
//!
//! Docker can't consume an OCI layout directory directly (that's why skopeo
//! was previously required for `mise oci run --engine docker`). It can,
//! however, load a docker-archive tarball from stdin. This module converts
//! the layout on the fly — config + decompressed layers + `manifest.json` —
//! and streams it straight into `docker load` without materializing the
//! archive on disk.
//!
//! Layers are decompressed because the docker-archive format stores plain
//! tars: the daemon matches layer content against the config's
//! `rootfs.diff_ids`, which are digests of the *uncompressed* streams.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use eyre::{Context, Result, bail};
use jdx_tar::{Builder, EntryType, Header};
use serde::Serialize;

use crate::oci::layout::ImageLayout;
use crate::oci::manifest::{ImageIndex, ImageManifest};

#[derive(Serialize)]
struct ManifestEntry {
    #[serde(rename = "Config")]
    config: String,
    #[serde(rename = "RepoTags")]
    repo_tags: Vec<String>,
    #[serde(rename = "Layers")]
    layers: Vec<String>,
}

/// Stream the OCI layout at `image_dir` into `docker load`, tagging the
/// loaded image as `tag`.
pub fn load_into_docker(image_dir: &Path, tag: &str) -> Result<()> {
    let layout = ImageLayout {
        root: image_dir.to_path_buf(),
    };
    let index_bytes = crate::file::read(image_dir.join("index.json"))?;
    let index: ImageIndex = serde_json::from_slice(&index_bytes).wrap_err("parsing index.json")?;
    let manifest_desc = match index.manifests.as_slice() {
        [one] => one,
        _ => bail!(
            "{}: expected exactly one manifest in index.json",
            image_dir.display()
        ),
    };
    let manifest_bytes = layout.read_blob(&manifest_desc.digest)?;
    let manifest: ImageManifest =
        serde_json::from_slice(&manifest_bytes).wrap_err("parsing image manifest blob")?;
    let config_bytes = layout.read_blob(&manifest.config.digest)?;

    // Validate every layer digest before it's used as a path component in
    // `write_docker_archive` (which reads via `blob_path`, bypassing the
    // check `read_blob` performs) — a crafted `--image-dir` layout could
    // otherwise escape the blobs directory with `sha256:../…`.
    for layer in &manifest.layers {
        crate::oci::layout::validate_sha256_digest(&layer.digest)?;
    }

    let mut child = Command::new("docker")
        .args(["load", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .wrap_err("spawning `docker load`")?;
    let stdin = child.stdin.take().expect("stdin piped");

    // Write the archive on a separate thread so the parent can drain docker's
    // stdout+stderr concurrently via `wait_with_output`. Writing the whole
    // archive before draining would deadlock if docker hit a fatal mid-load
    // error (disk full, overlay failure) on a large image: docker would stop
    // reading stdin and block once its stderr pipe buffer (~64 KB) filled,
    // while we blocked writing stdin.
    let writer = {
        let root = layout.root.clone();
        let manifest = manifest.clone();
        let tag = tag.to_string();
        std::thread::spawn(move || {
            let layout = ImageLayout { root };
            write_docker_archive(stdin, &layout, &manifest, &config_bytes, &tag)
        })
    };

    let out = child
        .wait_with_output()
        .wrap_err("waiting for `docker load`")?;
    let write_result = writer
        .join()
        .map_err(|_| eyre::eyre!("docker archive writer thread panicked"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let mut msg = format!(
            "`docker load` failed ({}): {}. Ensure the docker daemon is running and \
             your user has access to the socket.",
            out.status,
            stderr.trim()
        );
        // A write error here is usually the broken pipe caused by docker
        // dying (so docker's stderr is the real cause), but if it's something
        // else — e.g. a layer failed to decompress — surface it too so the
        // truncated-archive error from docker doesn't mask the root cause.
        if let Err(e) = &write_result {
            msg.push_str(&format!("\n(while writing archive: {e})"));
        }
        bail!(msg);
    }
    // docker succeeded — don't swallow a writer error if one somehow occurred.
    write_result?;
    Ok(())
}

fn write_docker_archive<W: Write>(
    out: W,
    layout: &ImageLayout,
    manifest: &ImageManifest,
    config_bytes: &[u8],
    tag: &str,
) -> Result<()> {
    let mut builder = Builder::new(out);

    let config_name = format!("{}.json", hex_of(&manifest.config.digest));
    append_bytes(&mut builder, &config_name, config_bytes)?;

    let mut layer_names = Vec::new();
    for (i, layer) in manifest.layers.iter().enumerate() {
        let name = format!("{i}/layer.tar");
        let blob_path = layout.blob_path(&layer.digest);
        let mut tmp = tempfile::tempfile().wrap_err("creating temp file for layer")?;
        decompress_blob(&blob_path, &mut tmp)
            .wrap_err_with(|| format!("decompressing layer {}", layer.digest))?;
        let size = tmp.seek(SeekFrom::End(0))?;
        tmp.seek(SeekFrom::Start(0))?;
        let mut header = file_header(size);
        builder.append_data(&mut header, &name, &mut tmp)?;
        layer_names.push(name);
    }

    let entries = vec![ManifestEntry {
        config: config_name,
        repo_tags: vec![tag.to_string()],
        layers: layer_names,
    }];
    append_bytes(
        &mut builder,
        "manifest.json",
        &serde_json::to_vec(&entries)?,
    )?;

    builder.into_inner()?.flush()?;
    Ok(())
}

/// Decompress a layer blob into `dst`, sniffing the compression from magic
/// bytes rather than trusting the manifest media type — base images pass
/// their layers through byte-for-byte, so we can see OCI gzip, docker gzip,
/// zstd, or plain tar here.
fn decompress_blob(path: &Path, dst: &mut std::fs::File) -> Result<()> {
    let mut f = std::fs::File::open(path)?;
    let mut magic = [0u8; 4];
    let n = f.read(&mut magic)?;
    f.seek(SeekFrom::Start(0))?;
    if n >= 2 && magic[0] == 0x1f && magic[1] == 0x8b {
        // MultiGzDecoder: layer blobs are occasionally multi-member gzip
        // streams (pigz, some registries' recompression).
        std::io::copy(&mut flate2::read::MultiGzDecoder::new(f), dst)?;
    } else if n >= 4 && magic == [0x28, 0xb5, 0x2f, 0xfd] {
        zstd::stream::copy_decode(f, &mut *dst)?;
    } else {
        std::io::copy(&mut f, dst)?;
    }
    Ok(())
}

fn append_bytes<W: Write>(builder: &mut Builder<W>, name: &str, bytes: &[u8]) -> Result<()> {
    let mut header = file_header(bytes.len() as u64);
    builder.append_data(&mut header, name, bytes)?;
    Ok(())
}

fn file_header(size: u64) -> Header {
    let mut header = Header::new_gnu(EntryType::File);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(size);
    header
}

fn hex_of(digest: &str) -> &str {
    digest.trim_start_matches("sha256:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oci::manifest::{Descriptor, MEDIA_TYPE_OCI_CONFIG, MEDIA_TYPE_OCI_MANIFEST};
    use jdx_tar::Archive;

    fn descriptor(media_type: &str, digest: String, size: u64) -> Descriptor {
        Descriptor {
            media_type: media_type.to_string(),
            size,
            digest,
            annotations: Default::default(),
            platform: None,
        }
    }

    /// Build a tiny layout, write the docker archive to memory, and verify
    /// its structure: config json, decompressed layer, manifest.json.
    #[test]
    fn writes_valid_docker_archive() {
        let td = tempfile::tempdir().unwrap();
        let layout = ImageLayout::init(td.path()).unwrap();

        // A gzipped "layer" (content doesn't need to be a real tar for the
        // writer — docker validates that, not us).
        let layer_tar = b"fake layer tar bytes".to_vec();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&layer_tar).unwrap();
        let layer_gz = gz.finish().unwrap();
        let (layer_digest, layer_size) = layout.write_blob(&layer_gz).unwrap();

        let config_bytes =
            br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#
                .to_vec();
        let (config_digest, config_size) = layout.write_blob(&config_bytes).unwrap();

        let manifest = ImageManifest {
            schema_version: 2,
            media_type: MEDIA_TYPE_OCI_MANIFEST.to_string(),
            config: descriptor(MEDIA_TYPE_OCI_CONFIG, config_digest.clone(), config_size),
            layers: vec![descriptor(
                crate::oci::manifest::MEDIA_TYPE_OCI_LAYER_GZIP,
                layer_digest,
                layer_size,
            )],
            annotations: Default::default(),
        };

        let mut archive_bytes = Vec::new();
        write_docker_archive(
            &mut archive_bytes,
            &layout,
            &manifest,
            &config_bytes,
            "mise-oci:test",
        )
        .unwrap();

        let mut archive = Archive::new(archive_bytes.as_slice());
        let mut entries = std::collections::HashMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().to_string();
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents).unwrap();
            entries.insert(path, contents);
        }

        let config_name = format!("{}.json", hex_of(&config_digest));
        assert_eq!(entries[&config_name], config_bytes);
        // Layer must be the *decompressed* bytes.
        assert_eq!(entries["0/layer.tar"], layer_tar);
        let manifest_json: serde_json::Value =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        assert_eq!(manifest_json[0]["Config"], config_name.as_str());
        assert_eq!(manifest_json[0]["RepoTags"][0], "mise-oci:test");
        assert_eq!(manifest_json[0]["Layers"][0], "0/layer.tar");
    }

    #[test]
    fn decompress_sniffs_zstd_and_plain() {
        let td = tempfile::tempdir().unwrap();
        let data = b"plain tar-ish content".to_vec();

        let zst_path = td.path().join("blob.zst");
        std::fs::write(&zst_path, zstd::encode_all(data.as_slice(), 0).unwrap()).unwrap();
        let mut out = tempfile::tempfile().unwrap();
        decompress_blob(&zst_path, &mut out).unwrap();
        out.seek(SeekFrom::Start(0)).unwrap();
        let mut got = Vec::new();
        out.read_to_end(&mut got).unwrap();
        assert_eq!(got, data);

        let plain_path = td.path().join("blob.tar");
        std::fs::write(&plain_path, &data).unwrap();
        let mut out = tempfile::tempfile().unwrap();
        decompress_blob(&plain_path, &mut out).unwrap();
        out.seek(SeekFrom::Start(0)).unwrap();
        let mut got = Vec::new();
        out.read_to_end(&mut got).unwrap();
        assert_eq!(got, data);
    }
}
