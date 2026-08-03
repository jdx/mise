use crate::error::Result;
use mlua::{ExternalResult, Lua, MultiValue, Table, Value};
use std::path::{Path, PathBuf};

pub fn mod_archiver(lua: &Lua) -> Result<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    Ok(loaded.set(
        "archiver",
        lua.create_table_from(vec![(
            "decompress",
            lua.create_async_function(
                |_lua: mlua::Lua, input| async move { decompress(&_lua, input) },
            )?,
        )])?,
    )?)
}

fn decompress(_lua: &Lua, input: MultiValue) -> mlua::Result<()> {
    let paths: Vec<mlua::Value> = input.into_iter().collect();
    if paths.len() < 2 {
        return Err(mlua::Error::runtime(
            "archiver.decompress requires an archive and destination",
        ));
    }
    let archive: PathBuf = PathBuf::from(paths[0].to_string()?);
    let destination: PathBuf = PathBuf::from(paths[1].to_string()?);
    let strip_components = match paths.get(2) {
        None | Some(Value::Nil) => 0,
        Some(Value::Table(options)) => options
            .get::<Option<usize>>("strip_components")?
            .unwrap_or(0),
        Some(value) => {
            return Err(mlua::Error::runtime(format!(
                "archiver.decompress options must be a table, got {}",
                value.type_name()
            )));
        }
    };
    if strip_components > 1 {
        return Err(mlua::Error::runtime(
            "archiver.decompress only supports strip_components values of 0 or 1",
        ));
    }

    if strip_components == 0 {
        return decompress_archive(&archive, &destination);
    }

    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    xx::file::mkdirp(parent).into_lua_err()?;
    let temp_dir = tempfile::Builder::new()
        .prefix(".mise-extract-")
        .tempdir_in(parent)
        .into_lua_err()?;
    decompress_archive(&archive, temp_dir.path())?;
    xx::file::mkdirp(&destination).into_lua_err()?;
    strip_archive_path_components(temp_dir.path(), &destination)
}

/// Match mise's built-in archive extraction behavior: promote the contents of
/// top-level directories while retaining files that are already at the root.
fn strip_archive_path_components(extracted: &Path, destination: &Path) -> mlua::Result<()> {
    for entry in xx::file::ls(extracted).into_lua_err()? {
        if entry
            .symlink_metadata()
            .into_lua_err()?
            .file_type()
            .is_dir()
        {
            for child in xx::file::ls(&entry).into_lua_err()? {
                let file_name = child.file_name().ok_or_else(|| {
                    mlua::Error::runtime(format!("invalid archive entry: {}", child.display()))
                })?;
                xx::file::mv(&child, destination.join(file_name)).into_lua_err()?;
            }
        } else {
            let file_name = entry.file_name().ok_or_else(|| {
                mlua::Error::runtime(format!("invalid archive entry: {}", entry.display()))
            })?;
            xx::file::mv(&entry, destination.join(file_name)).into_lua_err()?;
        }
    }
    Ok(())
}

fn decompress_archive(archive: &Path, destination: &Path) -> mlua::Result<()> {
    let filename = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            mlua::Error::runtime(format!("invalid archive path: {}", archive.display()))
        })?;
    if filename.ends_with(".zip") {
        xx::archive::unzip(archive, destination).into_lua_err()?;
    } else if filename.ends_with(".tar.gz") {
        xx::archive::untar_gz(archive, destination).into_lua_err()?;
    } else if filename.ends_with(".tar.xz") {
        xx::archive::untar_xz(archive, destination).into_lua_err()?;
    } else if filename.ends_with(".tar.bz2") {
        xx::archive::untar_bz2(archive, destination).into_lua_err()?;
    } else {
        return Err(mlua::Error::runtime(format!(
            "unsupported archive format: {}",
            archive.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zip() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let dst_path = temp_dir.path().join("unzip");
        let dst_path_str = dst_path.to_string_lossy().to_string();
        let lua = Lua::new();
        mod_archiver(&lua).unwrap();
        lua.load(mlua::chunk! {
            local archiver = require("archiver")
            archiver.decompress("test/data/foo.zip", $dst_path_str)
        })
        .exec()
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dst_path.join("foo/test.txt")).unwrap(),
            "yep\n"
        );
        // TempDir automatically cleans up when dropped
    }

    #[test]
    fn test_zip_strip_components() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let dst_path = temp_dir.path().join("unzip");
        let dst_path_str = dst_path.to_string_lossy().to_string();
        let lua = Lua::new();
        mod_archiver(&lua).unwrap();
        lua.load(mlua::chunk! {
            local archiver = require("archiver")
            archiver.decompress("test/data/foo.zip", $dst_path_str, {strip_components = 1})
        })
        .exec()
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dst_path.join("test.txt")).unwrap(),
            "yep\n"
        );
        assert!(!dst_path.join("foo").exists());
    }

    #[test]
    fn test_strip_components_preserves_root_files() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let extracted = temp_dir.path().join("extracted");
        let destination = temp_dir.path().join("destination");
        std::fs::create_dir_all(extracted.join("pkg")).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(extracted.join("README"), "readme").unwrap();
        std::fs::write(extracted.join("pkg/tool"), "tool").unwrap();

        strip_archive_path_components(&extracted, &destination).unwrap();

        assert_eq!(
            std::fs::read_to_string(destination.join("README")).unwrap(),
            "readme"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("tool")).unwrap(),
            "tool"
        );
        assert!(!destination.join("pkg").exists());
    }

    #[test]
    fn test_unsupported_archive_returns_lua_error() {
        let lua = Lua::new();
        mod_archiver(&lua).unwrap();
        let err = lua
            .load(mlua::chunk! {
                local archiver = require("archiver")
                archiver.decompress("archive.rar", "destination")
            })
            .exec()
            .unwrap_err();
        assert!(err.to_string().contains("unsupported archive format"));
    }
}
