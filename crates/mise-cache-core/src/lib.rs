//! Shared remote cache protocol and client implementation.

#![deny(unreachable_pub)]

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use eyre::{Result, bail, eyre};
use futures_util::TryStreamExt as _;
use log::warn;
use reqwest::StatusCode;
use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderValue, IF_NONE_MATCH,
};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::{Host, Url};

mod blob_pack;
mod client;
mod credentials;
mod protocol;
mod retry;

pub use client::RemoteCacheClient;
pub use protocol::{
    ACTION_RESULT_MEDIA_TYPE, BLOB_MEDIA_TYPE, BLOB_PACK_MEDIA_TYPE, BlobSource, BlobUpload,
    CLIENT_METADATA_MEDIA_TYPE, CacheDigest, CacheDirectory, CacheDirectoryNode, CacheFileNode,
    CacheSymlinkNode, DIRECTORY_MEDIA_TYPE, PROTOCOL_VERSION, RemoteActionResult, RemoteBlobPack,
    RemoteCacheConfig, RemoteCacheMode, canonical_json,
};

use blob_pack::*;
use credentials::*;
use protocol::*;
use retry::*;

#[cfg(test)]
mod tests;
