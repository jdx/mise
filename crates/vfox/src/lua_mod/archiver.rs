use crate::error::Result;
use mlua::{ExternalResult, Lua, MultiValue, Table};
use std::path::PathBuf;

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
    let archive: PathBuf = PathBuf::from(paths[0].to_string()?);
    let destination: PathBuf = PathBuf::from(paths[1].to_string()?);
    let filename = archive.file_name().unwrap().to_str().unwrap();
    if filename.ends_with(".zip") {
        xx::archive::unzip(&archive, &destination).into_lua_err()?;
    } else if filename.ends_with(".tar.gz") {
        xx::archive::untar_gz(&archive, &destination).into_lua_err()?;
    } else if filename.ends_with(".tar.xz") {
        xx::archive::untar_xz(&archive, &destination).into_lua_err()?;
    } else if filename.ends_with(".tar.bz2") {
        xx::archive::untar_bz2(&archive, &destination).into_lua_err()?;
    } else {
        unimplemented!("Unsupported archive format {:?}", archive);
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
}
