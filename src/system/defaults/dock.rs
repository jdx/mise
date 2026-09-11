//! Application layout semantics for the friendly Dock setting. Raw defaults remain exact.
#[cfg(any(target_os = "macos", test))]
use std::path::Path;
use std::path::{Component, PathBuf};

use super::DefaultsValue;
use crate::result::Result;

pub(super) fn paths(value: &DefaultsValue) -> Result<Vec<PathBuf>> {
    let DefaultsValue::Array(values) = value else {
        eyre::bail!("[bootstrap.macos.dock].apps: expected an array of application paths");
    };
    let mut paths = Vec::new();
    for value in values {
        let DefaultsValue::Str(value) = value else {
            eyre::bail!("[bootstrap.macos.dock].apps: expected an array of application paths");
        };
        let path = if let Some(rest) = value.strip_prefix("~/") {
            crate::env::HOME.join(rest)
        } else {
            PathBuf::from(value)
        };
        eyre::ensure!(
            path.is_absolute()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
                && !path.components().any(|c| matches!(c, Component::ParentDir))
                && !value.contains('\0'),
            "[bootstrap.macos.dock].apps: expected an absolute or ~/ application path: {value}"
        );
        let path: PathBuf = path.components().collect();
        eyre::ensure!(
            !paths.contains(&path),
            "duplicate Dock application: {}",
            path.display()
        );
        paths.push(path);
    }
    Ok(paths)
}

fn app_path(tile: &plist::Value) -> Option<PathBuf> {
    let tile = tile.as_dictionary()?;
    if tile.get("tile-type")?.as_string()? != "file-tile" {
        return None;
    }
    let location = tile
        .get("tile-data")?
        .as_dictionary()?
        .get("file-data")?
        .as_dictionary()?
        .get("_CFURLString")?
        .as_string()?;
    let path = if location.starts_with('/') {
        PathBuf::from(location)
    } else {
        url::Url::parse(location).ok()?.to_file_path().ok()?
    };
    if !path.is_absolute() || !path.extension()?.eq_ignore_ascii_case("app") {
        return None;
    }
    Some(path.components().collect())
}

fn tiles(value: Option<&plist::Value>) -> Result<&[plist::Value]> {
    match value {
        None => Ok(&[]),
        Some(plist::Value::Array(tiles)) => Ok(tiles),
        Some(_) => eyre::bail!("com.apple.dock persistent-apps is not an array; left unchanged"),
    }
}

pub(super) fn matches(value: &DefaultsValue, current: &plist::Value) -> Result<bool> {
    Ok(paths(value)?
        == tiles(Some(current))?
            .iter()
            .filter_map(app_path)
            .collect::<Vec<_>>())
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn reconcile(
    value: &DefaultsValue,
    current: Option<&plist::Value>,
) -> Result<plist::Value> {
    let desired = paths(value)?;
    for path in &desired {
        eyre::ensure!(
            path.is_dir(),
            "Dock application is unavailable: {}",
            path.display()
        );
    }
    arrange(&desired, tiles(current)?)
}

#[cfg(any(target_os = "macos", test))]
fn new_tile(path: &Path, guid: u64) -> Result<plist::Value> {
    let url = url::Url::from_directory_path(path)
        .map_err(|_| eyre::eyre!("invalid Dock application path: {}", path.display()))?;
    let mut file = plist::Dictionary::new();
    file.insert("_CFURLString".into(), url.to_string().into());
    file.insert("_CFURLStringType".into(), 15u64.into());
    let mut data = plist::Dictionary::new();
    data.insert("file-data".into(), file.into());
    data.insert(
        "file-label".into(),
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    data.insert("file-type".into(), 41u64.into());
    let mut tile = plist::Dictionary::new();
    tile.insert("GUID".into(), guid.into());
    tile.insert("tile-type".into(), "file-tile".into());
    tile.insert("tile-data".into(), data.into());
    Ok(tile.into())
}

#[cfg(any(target_os = "macos", test))]
fn arrange(desired: &[PathBuf], original: &[plist::Value]) -> Result<plist::Value> {
    let mut used: std::collections::HashSet<u64> = original
        .iter()
        .filter_map(|tile| tile.as_dictionary()?.get("GUID")?.as_unsigned_integer())
        .collect();
    let mut apps = Vec::new();
    for path in desired {
        if let Some(tile) = original
            .iter()
            .find(|tile| app_path(tile).as_ref() == Some(path))
        {
            apps.push(tile.clone());
        } else {
            let guid = loop {
                let candidate = rand::random::<u32>() as u64;
                if candidate != 0 && used.insert(candidate) {
                    break candidate;
                }
            };
            apps.push(new_tile(path, guid)?);
        }
    }
    let last_app = original.iter().rposition(|tile| app_path(tile).is_some());
    let mut apps = apps.into_iter();
    let mut result = Vec::new();
    for (i, tile) in original.iter().enumerate() {
        if app_path(tile).is_some() {
            if let Some(app) = apps.next() {
                result.push(app);
            }
            if Some(i) == last_app {
                result.extend(apps.by_ref());
            }
        } else {
            result.push(tile.clone());
        }
    }
    result.extend(apps);
    Ok(plist::Value::Array(result))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn declaration(paths: &[&str]) -> DefaultsValue {
        DefaultsValue::Array(
            paths
                .iter()
                .map(|p| DefaultsValue::Str((*p).into()))
                .collect(),
        )
    }

    #[test]
    fn validate_application_paths() {
        for input in [
            vec!["relative.app"],
            vec!["/Applications"],
            vec!["/Applications/../Other.app"],
            vec!["/A.app", "/A.app/"],
        ] {
            assert!(paths(&declaration(&input)).is_err(), "{input:?}");
        }
        assert!(paths(&DefaultsValue::Bool(true)).is_err());
        assert!(paths(&DefaultsValue::Array(vec![DefaultsValue::Int(1)])).is_err());
        assert_eq!(
            paths(&declaration(&["~/Applications/Test App.app"])).unwrap(),
            vec![crate::env::HOME.join("Applications/Test App.app")]
        );
        assert!(paths(&declaration(&[])).unwrap().is_empty());
    }

    #[test]
    fn preserve_metadata_and_opaque_tiles_while_reordering() {
        let a = PathBuf::from("/Applications/A.app");
        let b = PathBuf::from("/Applications/B App.app");
        let c = PathBuf::from("/Applications/C.app");
        let mut first = new_tile(&a, 1).unwrap();
        let data = first
            .as_dictionary_mut()
            .unwrap()
            .get_mut("tile-data")
            .unwrap()
            .as_dictionary_mut()
            .unwrap();
        data.insert("book".into(), plist::Value::Data(vec![1, 2, 3]));
        data.insert("file-mod-date".into(), 123u64.into());
        let second = new_tile(&b, 2).unwrap();
        let mut spacer = plist::Dictionary::new();
        spacer.insert("tile-type".into(), "spacer-tile".into());
        let spacer = plist::Value::Dictionary(spacer);
        let original = vec![first.clone(), spacer.clone(), second.clone()];
        let desired = vec![b.clone(), a.clone(), c.clone()];
        let result = arrange(&desired, &original).unwrap();
        let tiles = result.as_array().unwrap();
        assert_eq!(tiles[0], second);
        assert_eq!(tiles[1], spacer);
        assert_eq!(tiles[2], first);
        assert_eq!(app_path(&tiles[3]), Some(c));
        assert_eq!(arrange(&desired, tiles).unwrap(), result);
        assert!(
            matches(
                &declaration(&[
                    "/Applications/B App.app",
                    "/Applications/A.app",
                    "/Applications/C.app"
                ]),
                &result
            )
            .unwrap()
        );
        assert!(
            !matches(
                &declaration(&["/Applications/A.app", "/Applications/B App.app"]),
                &result
            )
            .unwrap()
        );
        assert_eq!(
            arrange(&[], &original).unwrap(),
            plist::Value::Array(vec![spacer])
        );
        // A duplicate live app is drift and is reduced to the declared single pin.
        assert!(
            !matches(
                &declaration(&["/Applications/A.app"]),
                &plist::Value::Array(vec![first.clone(), first.clone()])
            )
            .unwrap()
        );
        assert_eq!(
            arrange(&[a], &[first.clone(), first.clone()]).unwrap(),
            plist::Value::Array(vec![first])
        );
    }

    #[test]
    fn refuse_unreadable_layout_and_missing_apps() {
        assert!(tiles(Some(&plist::Value::Boolean(true))).is_err());
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("Missing.app");
        let value = declaration(&[missing.to_str().unwrap()]);
        assert!(reconcile(&value, None).is_err());
        std::fs::create_dir(&missing).unwrap();
        let result = reconcile(&value, None).unwrap();
        assert!(matches(&value, &result).unwrap());
        assert!(reconcile(&value, Some(&plist::Value::Boolean(true))).is_err());
    }
}
