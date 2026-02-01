use mlua::Table;
use mlua::prelude::*;

use super::get_or_create_loaded;

pub fn mod_semver(lua: &Lua) -> LuaResult<()> {
    let loaded: Table = get_or_create_loaded(lua)?;
    let semver = lua.create_table_from(vec![
        ("compare", lua.create_function(compare)?),
        ("parse", lua.create_function(parse)?),
        ("sort", lua.create_function(sort)?),
        ("sort_by", lua.create_function(sort_by)?),
    ])?;
    loaded.set("semver", semver.clone())?;
    loaded.set("vfox.semver", semver)?;
    Ok(())
}

/// Parse a version string into a table of numeric parts
/// e.g., "1.2.3" -> {1, 2, 3}
fn parse(_lua: &Lua, version: String) -> LuaResult<Vec<i64>> {
    Ok(parse_version(&version))
}

/// Compare two version strings
/// Returns -1 if v1 < v2, 0 if v1 == v2, 1 if v1 > v2
fn compare(_lua: &Lua, (v1, v2): (String, String)) -> LuaResult<i32> {
    Ok(compare_versions(&v1, &v2))
}

/// Sort a list of version strings in ascending order
fn sort(_lua: &Lua, versions: Vec<String>) -> LuaResult<Vec<String>> {
    let mut versions = versions;
    versions.sort_by(|a, b| compare_versions(a, b).cmp(&0));
    Ok(versions)
}

/// Sort a list of tables by a version field in ascending order
/// e.g., sort_by({{version = "1.2"}, {version = "1.1"}}, "version")
fn sort_by(_lua: &Lua, (arr, field): (Vec<Table>, String)) -> LuaResult<Vec<Table>> {
    let mut items: Vec<(Table, String)> = arr
        .into_iter()
        .map(|t| {
            let version: String = t.get(field.as_str()).unwrap_or_default();
            (t, version)
        })
        .collect();

    items.sort_by(|a, b| compare_versions(&a.1, &b.1).cmp(&0));

    Ok(items.into_iter().map(|(t, _)| t).collect())
}

fn parse_version(version: &str) -> Vec<i64> {
    version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

fn compare_versions(v1: &str, v2: &str) -> i32 {
    let parts1 = parse_version(v1);
    let parts2 = parse_version(v2);

    let max_len = parts1.len().max(parts2.len());
    for i in 0..max_len {
        let p1 = parts1.get(i).copied().unwrap_or(0);
        let p2 = parts2.get(i).copied().unwrap_or(0);
        if p1 != p2 {
            return if p1 < p2 { -1 } else { 1 };
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("1.2.3"), vec![1, 2, 3]);
        assert_eq!(parse_version("10.20.30"), vec![10, 20, 30]);
        assert_eq!(parse_version("1.2"), vec![1, 2]);
        assert_eq!(parse_version("1"), vec![1]);
        assert_eq!(parse_version("v1.2.3"), vec![1, 2, 3]);
        assert_eq!(parse_version("1.2.3-beta"), vec![1, 2, 3]);
    }

    #[test]
    fn test_compare_versions() {
        assert_eq!(compare_versions("1.2.3", "1.2.3"), 0);
        assert_eq!(compare_versions("1.2.3", "1.2.4"), -1);
        assert_eq!(compare_versions("1.2.4", "1.2.3"), 1);
        assert_eq!(compare_versions("1.2", "1.2.0"), 0);
        assert_eq!(compare_versions("9.6.9", "9.6.24"), -1);
        assert_eq!(compare_versions("10.0", "9.6.24"), 1);
        assert_eq!(compare_versions("1.08", "1.09"), -1);
    }

    #[test]
    fn test_semver_lua() {
        let lua = Lua::new();
        mod_semver(&lua).unwrap();
        lua.load(mlua::chunk! {
            local semver = require("semver")

            -- Test compare
            assert(semver.compare("1.2.3", "1.2.3") == 0, "equal versions")
            assert(semver.compare("1.2.3", "1.2.4") == -1, "less than")
            assert(semver.compare("1.2.4", "1.2.3") == 1, "greater than")
            assert(semver.compare("9.6.9", "9.6.24") == -1, "numeric comparison")
            assert(semver.compare("10.0", "9.6.24") == 1, "major version")

            -- Test parse
            local parts = semver.parse("1.2.3")
            assert(parts[1] == 1 and parts[2] == 2 and parts[3] == 3, "parse")

            -- Test sort
            local versions = semver.sort({"1.2", "1.10", "1.1", "2.0"})
            assert(versions[1] == "1.1", "sort[1]")
            assert(versions[2] == "1.2", "sort[2]")
            assert(versions[3] == "1.10", "sort[3]")
            assert(versions[4] == "2.0", "sort[4]")
        })
        .exec()
        .unwrap();
    }
}
