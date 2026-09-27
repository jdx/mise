use super::*;

pub(crate) fn blob_pack_chunk(
    digests: &[CacheDigest],
    limits: BlobPackLimits,
) -> Result<Vec<CacheDigest>> {
    let mut seen = BTreeSet::new();
    let mut chunk = Vec::new();
    let mut chunk_bytes = 0_u64;
    for digest in digests {
        digest.validate()?;
        if !seen.insert(digest.clone()) || digest.size > limits.max_bytes {
            continue;
        }
        if chunk.len() == limits.max_items
            || chunk_bytes.saturating_add(digest.size) > limits.max_bytes
        {
            break;
        }
        chunk_bytes = chunk_bytes.saturating_add(digest.size);
        chunk.push(digest.clone());
    }
    Ok(chunk)
}

pub(crate) fn blob_pack_download_timeout(base: Duration, digests: &[CacheDigest]) -> Duration {
    let bytes = digests
        .iter()
        .fold(0_u64, |total, digest| total.saturating_add(digest.size));
    let byte_units = bytes.div_ceil(BLOB_PACK_TIMEOUT_BYTES_PER_UNIT);
    let item_units = digests.len().div_ceil(BLOB_PACK_TIMEOUT_ITEMS_PER_UNIT);
    let item_units = u64::try_from(item_units).unwrap_or(u64::MAX);
    let multiplier = byte_units.max(item_units).max(1);
    base.saturating_mul(u32::try_from(multiplier).unwrap_or(u32::MAX))
}

pub(crate) async fn decode_blob_pack(
    response: reqwest::Response,
    requested: &[CacheDigest],
    staging_dir: &Path,
) -> Result<DownloadedBlobPack> {
    let metadata = BlobPackResponseMetadata::from_headers(response.headers())?;
    let requested = requested.iter().cloned().collect::<BTreeSet<_>>();
    let stream = response.bytes_stream().map_err(std::io::Error::other);
    let mut reader = tokio_util::io::StreamReader::new(stream);
    let mut magic = [0_u8; BLOB_PACK_MAGIC.len()];
    reader.read_exact(&mut magic).await?;
    if &magic != BLOB_PACK_MAGIC {
        bail!("remote cache blob pack has invalid magic");
    }

    let directory = tempfile::tempdir_in(staging_dir)?;
    let mut seen = BTreeSet::new();
    let mut blobs = Vec::new();
    let mut payload_bytes = 0_u64;
    let mut framed_bytes = BLOB_PACK_MAGIC.len() as u64;
    loop {
        let mut algorithm = [0_u8; 1];
        if reader.read(&mut algorithm).await? == 0 {
            break;
        }
        let (algorithm, mut hasher) = match algorithm[0] {
            1 => (
                "blake3",
                BlobPackHasher::Blake3(Box::new(blake3::Hasher::new())),
            ),
            2 => ("sha256", BlobPackHasher::Sha256(sha2::Sha256::new())),
            _ => bail!("remote cache blob pack has an invalid digest algorithm"),
        };
        let mut hash = [0_u8; 32];
        reader.read_exact(&mut hash).await?;
        let mut size = [0_u8; 8];
        reader.read_exact(&mut size).await?;
        let digest = CacheDigest {
            algorithm: algorithm.into(),
            hash: hex::encode(hash),
            size: u64::from_be_bytes(size),
        };
        if !requested.contains(&digest) {
            bail!("remote cache blob pack returned an unrequested digest");
        }
        if !seen.insert(digest.clone()) {
            bail!("remote cache blob pack returned a duplicate digest");
        }
        framed_bytes = framed_bytes
            .checked_add(BLOB_PACK_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(digest.size))
            .ok_or_else(|| eyre!("remote cache blob pack is too large"))?;
        payload_bytes = payload_bytes
            .checked_add(digest.size)
            .ok_or_else(|| eyre!("remote cache blob pack payload is too large"))?;

        let path = directory.path().join(blobs.len().to_string());
        let mut output = tokio::fs::File::create(&path).await?;
        let mut remaining = digest.size;
        let mut buffer = [0_u8; 64 * 1024];
        while remaining > 0 {
            let limit = usize::try_from(remaining.min(buffer.len() as u64)).unwrap();
            let count = reader.read(&mut buffer[..limit]).await?;
            if count == 0 {
                bail!("remote cache blob pack ended before a blob was complete");
            }
            output.write_all(&buffer[..count]).await?;
            hasher.update(&buffer[..count]);
            remaining -= count as u64;
        }
        output.flush().await?;
        drop(output);
        if !hasher.matches(&digest.hash) {
            bail!("remote cache blob pack failed digest verification");
        }
        blobs.push((digest, path));
    }
    let blob_count = blobs.len().try_into().unwrap_or(u64::MAX);
    let metadata = metadata.validate(BlobPackResponseStats {
        blob_count,
        payload_bytes,
        framed_bytes,
    })?;
    Ok(DownloadedBlobPack {
        directory,
        blobs,
        metadata,
    })
}

pub(crate) enum BlobPackHasher {
    Blake3(Box<blake3::Hasher>),
    Sha256(sha2::Sha256),
}

impl BlobPackHasher {
    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Blake3(hasher) => {
                hasher.update(bytes);
            }
            Self::Sha256(hasher) => {
                hasher.update(bytes);
            }
        }
    }

    fn matches(self, expected: &str) -> bool {
        match self {
            Self::Blake3(hasher) => hasher.finalize().to_hex().as_str() == expected,
            Self::Sha256(hasher) => hex::encode(hasher.finalize()) == expected,
        }
    }
}
