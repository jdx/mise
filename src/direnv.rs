use std::collections::HashMap;
use std::env::{join_paths, split_paths};
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::env::PATH_KEY;
use base64::prelude::*;
use eyre::Result;
use flate2::Compression;
use flate2::write::{ZlibDecoder, ZlibEncoder};
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DirenvDiff {
    #[serde(default, rename = "p")]
    pub old: HashMap<String, String>,
    #[serde(default, rename = "n")]
    pub new: HashMap<String, String>,
}

impl DirenvDiff {
    pub fn parse(input: &str) -> Result<DirenvDiff> {
        // let bytes = BASE64_URL_SAFE.decode(input)?;
        // let uncompressed = inflate_bytes_zlib(&bytes).unwrap();
        // Ok(serde_json::from_slice(&uncompressed[..])?)
        let mut writer = Vec::new();
        let mut decoder = ZlibDecoder::new(writer);
        let bytes = BASE64_URL_SAFE.decode(input)?;
        decoder.write_all(&bytes[..])?;
        writer = decoder.finish()?;
        Ok(serde_json::from_slice(&writer[..])?)
    }

    pub fn new_path(&self) -> Vec<PathBuf> {
        let path = self.new.get(&*PATH_KEY);
        match path {
            Some(path) => split_paths(path).collect(),
            None => vec![],
        }
    }

    pub fn old_path(&self) -> Vec<PathBuf> {
        let path = self.old.get(&*PATH_KEY);
        match path {
            Some(path) => split_paths(path).collect(),
            None => vec![],
        }
    }

    /// this adds a directory to both the old and new path in DIRENV_DIFF
    /// the purpose is to trick direnv into thinking that this path has always been there
    /// that way it does not remove it when it modifies PATH
    /// it returns the old and new paths as vectors
    pub fn add_path_to_old_and_new(&mut self, path: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
        let mut old = self.old_path();
        let mut new = self.new_path();

        old.insert(0, path.into());
        new.insert(0, path.into());

        self.old.insert(
            PATH_KEY.to_string(),
            join_paths(&old)?.into_string().unwrap(),
        );
        self.new.insert(
            PATH_KEY.to_string(),
            join_paths(&new)?.into_string().unwrap(),
        );

        Ok((old, new))
    }

    pub fn remove_path_from_old_and_new(
        &mut self,
        path: &Path,
    ) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
        let mut old = self.old_path();
        let mut new = self.new_path();

        // remove the path from both old and new but only once
        old.iter().position(|p| p == path).map(|i| old.remove(i));
        new.iter().position(|p| p == path).map(|i| new.remove(i));

        self.old.insert(
            PATH_KEY.to_string(),
            join_paths(&old)?.into_string().unwrap(),
        );
        self.new.insert(
            PATH_KEY.to_string(),
            join_paths(&new)?.into_string().unwrap(),
        );

        Ok((old, new))
    }

    pub fn dump(&self) -> Result<String> {
        let mut gz = ZlibEncoder::new(Vec::new(), Compression::fast());
        gz.write_all(&serde_json::to_vec(self)?)?;
        Ok(BASE64_URL_SAFE.encode(gz.finish()?))
    }
}

impl Display for DirenvDiff {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let print_sorted = |hashmap: &HashMap<String, String>| {
            hashmap
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .sorted()
                .collect::<Vec<_>>()
        };
        f.debug_struct("DirenvDiff")
            .field("old", &print_sorted(&self.old))
            .field("new", &print_sorted(&self.new))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::*;
    use insta::assert_snapshot;

    #[tokio::test]
    async fn test_parse() {
        let _config = Config::get().await.unwrap();
        let input = r#"eJys0c1yojAAwPF3ybmWaLB-zPSAGCqIQCGgeGGIELDlM2BEOr77zs7szr7AXv-H3-X_Axqw_gGabYM1qPk1A88XUP1OW93FVhBtdReswURq-FXEfSqJmEusLpKUdxLspALRJY1Yt2Bifk8aLhf5iiZIhhDCjEtE6svmteGuSJVHAV7-qppuYrAG_0WVXtNK8Ms__KgQdYc9sAapMXRj1-9XW8VX7A16UA4NPIs9xCK5WO51XnvfwWBT1R9N7zIcHvvJbZF5g8pk0V2c5CboIw8_NjOUWDK5qcxIcaFrp3anhwdr5FeKJmfd9stgqvuVZqcXsXHYJ-kSGWpoxyZLzf0a0LUcMgv17exenXXunfOTZZfybiVmb9OAhjDtHEcOk0lrRWG84OrRobW6IgGGZqwelglTq8UmJrbP9p0x9pTW5t3L21P1mZfL7_pMtIW599v-Cx_dmzEdCcZ1TAzkz7dvfO4QAefO6Y4VxYmijzgP_Oz9Hbz8uU5jDp7PXwEAAP__wB6qKg=="#;
        let diff = DirenvDiff::parse(input).unwrap();
        assert_snapshot!(diff);
    }

    #[tokio::test]
    async fn test_dump() {
        let _config = Config::get().await.unwrap();
        let diff = DirenvDiff {
            old: HashMap::from([("a".to_string(), "b".to_string())]),
            new: HashMap::from([("c".to_string(), "d".to_string())]),
        };
        let output = diff.dump().unwrap();
        assert_snapshot!(&output);
        let diff = DirenvDiff::parse(&output).unwrap();
        assert_snapshot!(diff);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_add_path_to_old_and_new() {
        let _config = Config::get().await.unwrap();
        let mut diff = DirenvDiff {
            old: HashMap::from([("PATH".to_string(), "/foo:/tmp:/bar:/old".to_string())]),
            new: HashMap::from([("PATH".to_string(), "/foo:/bar:/new".to_string())]),
        };
        let path = PathBuf::from("/tmp");
        diff.add_path_to_old_and_new(&path).unwrap();
        assert_snapshot!(diff.old.get("PATH").unwrap());
        assert_snapshot!(diff.new.get("PATH").unwrap());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_null_path() {
        let _config = Config::get().await.unwrap();
        let mut diff = DirenvDiff {
            old: HashMap::from([]),
            new: HashMap::from([]),
        };
        let path = PathBuf::from("/tmp");
        diff.add_path_to_old_and_new(&path).unwrap();
        assert_snapshot!(diff.old.get("PATH").unwrap());
        assert_snapshot!(diff.new.get("PATH").unwrap());
    }
}
